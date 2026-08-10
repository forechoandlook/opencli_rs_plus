import { beforeEach, describe, expect, it, vi } from 'vitest';

type Listener<T extends (...args: any[]) => void> = { addListener: (fn: T) => void };

type MockTab = {
  id: number;
  windowId: number;
  url?: string;
  title?: string;
  active?: boolean;
  status?: string;
};

class MockWebSocket {
  static OPEN = 1;
  static CONNECTING = 0;
  readyState = MockWebSocket.CONNECTING;
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;

  constructor(_url: string) {}
  send(_data: string): void {}
  close(): void {
    this.onclose?.();
  }
}

function createChromeMock() {
  let nextTabId = 10;
  const sessionStorage: Record<string, unknown> = {};
  const tabs: MockTab[] = [
    { id: 1, windowId: 1, url: 'https://automation.example', title: 'automation', active: true, status: 'complete' },
    { id: 2, windowId: 2, url: 'https://user.example', title: 'user', active: true, status: 'complete' },
    { id: 3, windowId: 1, url: 'chrome://extensions', title: 'chrome', active: false, status: 'complete' },
  ];

  const query = vi.fn(async (queryInfo: { windowId?: number } = {}) => {
    return tabs.filter((tab) => queryInfo.windowId === undefined || tab.windowId === queryInfo.windowId);
  });
  const create = vi.fn(async ({ windowId, url, active }: { windowId?: number; url?: string; active?: boolean }) => {
    const tab: MockTab = {
      id: nextTabId++,
      windowId: windowId ?? 999,
      url,
      title: url ?? 'blank',
      active: !!active,
      status: 'complete',
    };
    tabs.push(tab);
    return tab;
  });
  const update = vi.fn(async (tabId: number, updates: { active?: boolean; url?: string }) => {
    const tab = tabs.find((entry) => entry.id === tabId);
    if (!tab) throw new Error(`Unknown tab ${tabId}`);
    if (updates.active !== undefined) tab.active = updates.active;
    if (updates.url !== undefined) tab.url = updates.url;
    return tab;
  });

  const chrome = {
    tabs: {
      query,
      create,
      update,
      remove: vi.fn(async (_tabId: number) => {}),
      get: vi.fn(async (tabId: number) => {
        const tab = tabs.find((entry) => entry.id === tabId);
        if (!tab) throw new Error(`Unknown tab ${tabId}`);
        return tab;
      }),
      onRemoved: { addListener: vi.fn() } as Listener<(tabId: number) => void>,
      onUpdated: { addListener: vi.fn(), removeListener: vi.fn() } as Listener<(id: number, info: chrome.tabs.TabChangeInfo) => void>,
    },
    windows: {
      get: vi.fn(async (windowId: number) => ({ id: windowId })),
      create: vi.fn(async ({ url, focused, width, height, type }: any) => ({ id: 1, url, focused, width, height, type })),
      remove: vi.fn(async (_windowId: number) => {}),
      onRemoved: { addListener: vi.fn() } as Listener<(windowId: number) => void>,
    },
    alarms: {
      create: vi.fn(),
      onAlarm: { addListener: vi.fn() } as Listener<(alarm: { name: string }) => void>,
    },
    runtime: {
      onInstalled: { addListener: vi.fn() } as Listener<() => void>,
      onStartup: { addListener: vi.fn() } as Listener<() => void>,
      onMessage: { addListener: vi.fn() } as Listener<(message: unknown, sender: unknown, sendResponse: unknown) => void>,
    },
    storage: {
      session: {
        get: vi.fn(async (key: string) => ({ [key]: sessionStorage[key] })),
        set: vi.fn(async (values: Record<string, unknown>) => { Object.assign(sessionStorage, values); }),
      },
    },
    cookies: {
      getAll: vi.fn(async () => []),
    },
  };

  return { chrome, tabs, query, create, update };
}

describe('background tab isolation', () => {
  beforeEach(() => {
    vi.resetModules();
    vi.stubGlobal('WebSocket', MockWebSocket);
  });

  it('lists only automation-window web tabs', async () => {
    const { chrome } = createChromeMock();
    vi.stubGlobal('chrome', chrome);

    const mod = await import('./background');
    mod.__test__.setAutomationWindowId('site:twitter', 1);

    await mod.__test__.handleTabs({ id: 'new', action: 'tabs', op: 'new', workspace: 'site:twitter' }, 'site:twitter');

    const result = await mod.__test__.handleTabs({ id: '1', action: 'tabs', op: 'list', workspace: 'site:twitter' }, 'site:twitter');

    expect(result.ok).toBe(true);
    expect(result.data).toEqual([
      {
        index: 0,
        tabId: 10,
        url: 'data:text/html,<html></html>',
        title: 'data:text/html,<html></html>',
        active: false,
      },
    ]);
  });

  it('creates new tabs inside the automation window', async () => {
    const { chrome, create } = createChromeMock();
    vi.stubGlobal('chrome', chrome);

    const mod = await import('./background');
    mod.__test__.setAutomationWindowId('site:twitter', 1);

    const result = await mod.__test__.handleTabs({ id: '2', action: 'tabs', op: 'new', url: 'https://new.example', workspace: 'site:twitter' }, 'site:twitter');

    expect(result.ok).toBe(true);
    expect(create).toHaveBeenCalledWith({ windowId: 1, url: 'https://new.example', active: false });
  });

  it('never reuses a user tab as an automation target', async () => {
    const { chrome, create } = createChromeMock();
    vi.stubGlobal('chrome', chrome);

    const mod = await import('./background');
    mod.__test__.setAutomationWindowId('site:example', 1);

    const tabId = await mod.__test__.resolveTabId(undefined, 'site:example');

    expect(tabId).toBe(10);
    expect(create).toHaveBeenCalledWith({
      windowId: 1,
      url: 'data:text/html,<html></html>',
      active: false,
    });
  });

  it('borrows an exact user-page match only when navigation explicitly opts in', async () => {
    const { chrome, tabs, update } = createChromeMock();
    tabs[1].url = 'https://user.example/favorites';
    vi.stubGlobal('chrome', chrome);

    const mod = await import('./background');
    const result = await mod.__test__.handleNavigate({
      id: 'borrow',
      action: 'navigate',
      url: 'https://user.example/favorites',
      reuse_existing_tab: true,
      workspace: 'site:user',
    }, 'site:user');

    expect(result.ok).toBe(true);
    expect(result.data).toEqual(expect.objectContaining({ tabId: 2, reused: true }));
    expect(update).not.toHaveBeenCalled();
    await expect(mod.__test__.resolveTabId(2, 'site:user')).resolves.toBe(2);
  });

  it('creates a minimized dedicated window instead of using a user window', async () => {
    const { chrome } = createChromeMock();
    vi.stubGlobal('chrome', chrome);

    const mod = await import('./background');
    await mod.__test__.resolveTabId(undefined, 'site:example');

    expect(chrome.windows.create).toHaveBeenCalledWith({
      url: 'data:text/html,<html></html>',
      focused: false,
      state: 'minimized',
      type: 'normal',
    });
  });

  it('reports sessions per workspace', async () => {
    const { chrome } = createChromeMock();
    vi.stubGlobal('chrome', chrome);

    const mod = await import('./background');
    mod.__test__.setAutomationWindowId('site:twitter', 1);
    mod.__test__.setAutomationWindowId('site:zhihu', 1);

    const result = await mod.__test__.handleSessions({ id: '3', action: 'sessions' });
    expect(result.ok).toBe(true);
    expect(result.data).toEqual(expect.arrayContaining([
      expect.objectContaining({ workspace: 'site:twitter', windowId: 1 }),
      expect.objectContaining({ workspace: 'site:zhihu', windowId: 1 }),
    ]));
  });

  it('evicts the least recently used cached page after ten workspaces', async () => {
    const { chrome } = createChromeMock();
    vi.stubGlobal('chrome', chrome);

    const mod = await import('./background');
    for (let i = 0; i < 11; i++) {
      await mod.__test__.resolveTabId(undefined, `site:cache-${i}`);
    }

    const state = await mod.__test__.getAutomationState();
    expect(state.capacity).toBe(10);
    expect(state.count).toBe(10);
    expect(state.pages.map((page: { workspace: string }) => page.workspace)).not.toContain('site:cache-0');
    expect(chrome.tabs.remove).toHaveBeenCalledWith(10);
  });

  it('recognizes a cached page URL with an implicit trailing slash', async () => {
    const { chrome } = createChromeMock();
    vi.stubGlobal('chrome', chrome);

    const mod = await import('./background');
    expect(mod.__test__.isSameNavigationTarget('https://www.zhihu.com/', 'https://www.zhihu.com')).toBe(true);
    expect(mod.__test__.isSameNavigationTarget('https://www.zhihu.com/', 'https://www.zhihu.com/api')).toBe(false);
  });
});

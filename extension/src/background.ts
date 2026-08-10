/**
 * OpenCLI — Service Worker (background script).
 *
 * Connects to the opencli daemon via WebSocket, receives commands,
 * dispatches them to Chrome APIs (debugger/tabs/cookies), returns results.
 */

import type { Command, Result } from './protocol';
import {
  daemonWsUrl,
  DAEMON_PORT,
  detectDaemonPort,
  getStoredPortConfig,
  storePort,
  WS_RECONNECT_BASE_DELAY,
  WS_RECONNECT_MAX_DELAY,
} from './protocol';
import * as executor from './cdp';
import { runCurrentPageAction } from './page_actions';

let ws: WebSocket | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
let reconnectAttempts = 0;
let connectedPort: number | null = null;

type ConnectionState = {
  configuredPort: number;
  pinned: boolean;
  connectedPort: number | null;
  connected: boolean;
};

// ─── Console log forwarding ──────────────────────────────────────────
// Hook console.log/warn/error to forward logs to daemon via WebSocket.

const _origLog = console.log.bind(console);
const _origWarn = console.warn.bind(console);
const _origError = console.error.bind(console);

function forwardLog(level: 'info' | 'warn' | 'error', args: unknown[]): void {
  if (!ws || ws.readyState !== WebSocket.OPEN) return;
  try {
    const msg = args.map(a => typeof a === 'string' ? a : JSON.stringify(a)).join(' ');
    ws.send(JSON.stringify({ type: 'log', level, msg, ts: Date.now() }));
  } catch { /* don't recurse */ }
}

console.log = (...args: unknown[]) => { _origLog(...args); forwardLog('info', args); };
console.warn = (...args: unknown[]) => { _origWarn(...args); forwardLog('warn', args); };
console.error = (...args: unknown[]) => { _origError(...args); forwardLog('error', args); };

// ─── WebSocket connection ────────────────────────────────────────────

async function getConnectionState(): Promise<ConnectionState> {
  const { port: savedPort, pinned } = await getStoredPortConfig();
  return {
    configuredPort: savedPort ?? DAEMON_PORT,
    pinned,
    connectedPort,
    connected: ws?.readyState === WebSocket.OPEN && connectedPort !== null,
  };
}

async function connect(): Promise<ConnectionState> {
  // Respect user-pinned ports; only auto-detect when the port is not pinned.
  const { port: savedPort, pinned } = await getStoredPortConfig();
  const port = pinned
    ? (savedPort ?? DAEMON_PORT)
    : ((await detectDaemonPort(savedPort)) ?? savedPort ?? DAEMON_PORT);
  if ((ws?.readyState === WebSocket.OPEN || ws?.readyState === WebSocket.CONNECTING) && connectedPort === port) {
    return await getConnectionState();
  }
  if (ws && connectedPort !== null && connectedPort !== port) {
    try { ws.close(); } catch { /* ignore */ }
    ws = null;
  }
  if (!pinned && port !== savedPort) {
    await storePort(port, false);
  }
  const wsUrl = daemonWsUrl(port);
  try {
    ws = new WebSocket(wsUrl);
  } catch {
    scheduleReconnect();
    return await getConnectionState();
  }

  ws.onopen = () => {
    connectedPort = port;
    console.log(`[opencli] Connected to daemon on port ${port}`);
    reconnectAttempts = 0; // Reset on successful connection
    if (reconnectTimer) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
  };

  ws.onmessage = (event) => {
    const command = JSON.parse(event.data as string) as Command;
    handleCommand(command).then(result => {
      ws?.send(JSON.stringify(result));
    }).catch(err => {
      console.error('[opencli] Message handling error:', err);
      ws?.send(JSON.stringify({ id: command.id, ok: false, error: String(err) }));
    });
  };

  ws.onclose = () => {
    connectedPort = null;
    console.log('[opencli] Disconnected from daemon');
    ws = null;
    scheduleReconnect();
  };

  ws.onerror = () => {
    connectedPort = null;
    ws?.close();
  };

  const connected = await new Promise<boolean>((resolve) => {
    const timer = setTimeout(() => resolve(false), 1200);
    ws!.addEventListener('open', () => {
      clearTimeout(timer);
      resolve(true);
    }, { once: true });
    ws!.addEventListener('error', () => {
      clearTimeout(timer);
      resolve(false);
    }, { once: true });
    ws!.addEventListener('close', () => {
      clearTimeout(timer);
      resolve(false);
    }, { once: true });
  });

  if (!connected) {
    connectedPort = null;
  }
  return await getConnectionState();
}

function scheduleReconnect(): void {
  if (reconnectTimer) return;
  reconnectAttempts++;
  // Exponential backoff: 2s, 4s, 8s, 16s, ..., capped at 60s
  const delay = Math.min(WS_RECONNECT_BASE_DELAY * Math.pow(2, reconnectAttempts - 1), WS_RECONNECT_MAX_DELAY);
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    void connect(); // connect is async but we don't await here
  }, delay);
}

// ─── Automation session ───────────────────────────────────────────────
// Uses a dedicated minimized Chrome window. A YAML adapter can explicitly opt
// into borrowing a user tab, but only when that tab already has the exact
// target URL. Borrowed tabs are never cached, closed, or navigated.

type AutomationSession = {
  tabIds: Set<number>;
  tabLastUsedAt: Map<number, number>;
  lastUsedAt: number;
};

const automationSessions = new Map<string, AutomationSession>();
const borrowedTabWorkspaces = new Map<number, string>();
const PAGE_CACHE_LIMIT = 10;
const AUTOMATION_STATE_STORAGE_KEY = 'opencliAutomationStateV1';
let automationWindowId: number | null = null;
let lastAccessAt = 0;
let automationStateLoaded: Promise<void> | null = null;

type StoredAutomationState = {
  windowId: number | null;
  sessions: Record<string, { tabIds: number[]; tabLastUsedAt: Array<[number, number]>; lastUsedAt: number }>;
};

async function loadAutomationState(): Promise<void> {
  try {
    const stored = await chrome.storage.session.get(AUTOMATION_STATE_STORAGE_KEY) as Record<string, StoredAutomationState | undefined>;
    const state = stored[AUTOMATION_STATE_STORAGE_KEY];
    if (!state) return;

    automationWindowId = state.windowId;
    for (const [workspace, session] of Object.entries(state.sessions)) {
      automationSessions.set(workspace, {
        tabIds: new Set(session.tabIds),
        tabLastUsedAt: new Map(session.tabLastUsedAt),
        lastUsedAt: session.lastUsedAt,
      });
      lastAccessAt = Math.max(lastAccessAt, session.lastUsedAt);
    }
  } catch (err) {
    console.warn(`[opencli] Failed to restore automation page index: ${String(err)}`);
  }
}

async function ensureAutomationStateLoaded(): Promise<void> {
  if (!automationStateLoaded) automationStateLoaded = loadAutomationState();
  await automationStateLoaded;
}

async function persistAutomationState(): Promise<void> {
  const sessions: StoredAutomationState['sessions'] = {};
  for (const [workspace, session] of automationSessions) {
    sessions[workspace] = {
      tabIds: [...session.tabIds],
      tabLastUsedAt: [...session.tabLastUsedAt],
      lastUsedAt: session.lastUsedAt,
    };
  }
  try {
    await chrome.storage.session.set({
      [AUTOMATION_STATE_STORAGE_KEY]: { windowId: automationWindowId, sessions },
    });
  } catch (err) {
    console.warn(`[opencli] Failed to persist automation page index: ${String(err)}`);
  }
}

function getWorkspaceKey(workspace?: string): string {
  return workspace?.trim() || 'default';
}

function nextAccessAt(): number {
  // Date.now() has millisecond precision, while several commands can arrive
  // within one millisecond. Keep the LRU ordering deterministic in that case.
  lastAccessAt = Math.max(Date.now(), lastAccessAt + 1);
  return lastAccessAt;
}

function touchSession(workspace: string, tabId?: number): void {
  const session = automationSessions.get(workspace);
  if (!session) return;
  const at = nextAccessAt();
  session.lastUsedAt = at;
  if (tabId !== undefined && session.tabIds.has(tabId)) {
    session.tabLastUsedAt.set(tabId, at);
  }
}

function trackTab(workspace: string, tabId: number): void {
  const session = automationSessions.get(workspace);
  if (!session) return;
  session.tabIds.add(tabId);
  touchSession(workspace, tabId);
}

function forgetTab(workspace: string, tabId: number): void {
  const session = automationSessions.get(workspace);
  if (!session) return;
  session.tabIds.delete(tabId);
  session.tabLastUsedAt.delete(tabId);
  if (borrowedTabWorkspaces.get(tabId) === workspace) borrowedTabWorkspaces.delete(tabId);
  if (!session.tabIds.size) automationSessions.delete(workspace);
}

/**
 * Get (or create) the automation session for a workspace.
 *
 * All workspaces share one OpenCLI-owned minimized window. Each workspace
 * keeps one cached page in that window; the least recently used page is
 * evicted once the cache reaches PAGE_CACHE_LIMIT.
 */
async function getAutomationWindow(workspace: string): Promise<number> {
  await ensureAutomationStateLoaded();
  if (automationWindowId !== null) {
    try {
      await chrome.windows.get(automationWindowId);
    } catch {
      automationWindowId = null;
      automationSessions.clear();
    }
  }

  if (automationWindowId === null) {
    let win;
    try {
      win = await chrome.windows.create({
        url: 'data:text/html,<html></html>',
        focused: false,
        state: 'minimized',
        type: 'normal',
      });
    } catch (err) {
      console.error(`[opencli] Failed to create automation window: ${err}`);
      throw err;
    }
    if (!win.id) {
      console.error('[opencli] Window created but no ID');
      throw new Error('Failed to create automation window: no window ID');
    }
    automationWindowId = win.id;
    console.log(`[opencli] Created minimized automation window ${automationWindowId}`);
  }

  if (!automationSessions.has(workspace)) {
    const at = nextAccessAt();
    automationSessions.set(workspace, { tabIds: new Set(), tabLastUsedAt: new Map(), lastUsedAt: at });
  }
  touchSession(workspace);
  // The preceding branch assigns a valid id or throws, so this is non-null.
  return automationWindowId!;
}

async function enforcePageCacheLimit(exceptTabId: number): Promise<void> {
  const cached = () => [...automationSessions.entries()].flatMap(([workspace, session]) =>
    [...session.tabIds].map((tabId) => ({
      workspace,
      tabId,
      lastUsedAt: session.tabLastUsedAt.get(tabId) ?? session.lastUsedAt,
    }))
  );
  while (cached().length > PAGE_CACHE_LIMIT) {
    const victim = cached()
      .filter((page) => page.tabId !== exceptTabId)
      .sort((a, b) => a.lastUsedAt - b.lastUsedAt)[0];
    if (!victim) return;
    try {
      await chrome.tabs.remove(victim.tabId);
      await executor.detach(victim.tabId);
    } catch (err) {
      console.warn(`[opencli] Failed to evict cached page ${victim.workspace}: ${String(err)}`);
    }
    forgetTab(victim.workspace, victim.tabId);
    console.log(`[opencli] Evicted LRU cached page: ${victim.workspace} tab ${victim.tabId}`);
  }
}

// Clean up if our automation window closes
chrome.windows.onRemoved.addListener((windowId) => {
  if (automationWindowId === windowId) {
    automationWindowId = null;
    automationSessions.clear();
    console.log('[opencli] Shared automation window closed');
    void persistAutomationState();
  }
});

// Track when our tabs are closed externally
chrome.tabs.onRemoved.addListener((tabId) => {
  borrowedTabWorkspaces.delete(tabId);
  for (const workspace of automationSessions.keys()) {
    forgetTab(workspace, tabId);
  }
  void persistAutomationState();
});

// ─── Lifecycle events ────────────────────────────────────────────────

let initialized = false;

async function initialize(): Promise<void> {
  if (initialized) return;
  initialized = true;
  chrome.alarms.create('keepalive', { periodInMinutes: 0.4 }); // ~24 seconds
  executor.registerListeners();
  await connect();
  console.log('[opencli] OpenCLI extension initialized');
}

chrome.runtime.onInstalled.addListener(() => {
  void initialize();
});

chrome.runtime.onStartup.addListener(() => {
  void initialize();
});

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === 'keepalive') void connect();
});

// ─── Command dispatcher ─────────────────────────────────────────────

async function handleCommand(cmd: Command): Promise<Result> {
  await ensureAutomationStateLoaded();
  const workspace = getWorkspaceKey(cmd.workspace);
  touchSession(workspace);
  try {
    const result = await (async () => {
      switch (cmd.action) {
      case 'exec':
        return await handleExec(cmd, workspace);
      case 'navigate':
        return await handleNavigate(cmd, workspace);
      case 'tabs':
        return await handleTabs(cmd, workspace);
      case 'cookies':
        return await handleCookies(cmd);
      case 'screenshot':
        return await handleScreenshot(cmd, workspace);
      case 'close-window':
        return await handleCloseWindow(cmd, workspace);
      case 'sessions':
        return await handleSessions(cmd);
      case 'bg_fetch':
        return await handleBgFetch(cmd);
      case 'upload':
        return await handleUpload(cmd, workspace);
      default:
        return { id: cmd.id, ok: false, error: `Unknown action: ${cmd.action}` };
      }
    })();
    await persistAutomationState();
    return result;
  } catch (err) {
    return {
      id: cmd.id,
      ok: false,
      error: err instanceof Error ? err.message : String(err),
    };
  }
}

// ─── Action handlers ─────────────────────────────────────────────────

/** Check if a URL can be attached via CDP (not chrome:// or chrome-extension://) */
function isDebuggableUrl(url?: string): boolean {
  if (!url) return true;  // empty/undefined = tab still loading, allow it
  return !url.startsWith('chrome://') && !url.startsWith('chrome-extension://');
}

/** Treat https://example.com and https://example.com/ as the same document. */
function isSameNavigationTarget(currentUrl: string, targetUrl: string): boolean {
  try {
    return new URL(currentUrl).href === new URL(targetUrl).href;
  } catch {
    return currentUrl === targetUrl;
  }
}

/**
 * Resolve target tab in the automation window.
 * If explicit tabId is given, use that directly.
 * Otherwise, find or create a tab owned by OpenCLI in the automation window.
 *
 * Never select an arbitrary tab from a reused user window: that would make a
 * `navigate` command reload the page the user is actively reading.
 */
async function resolveTabId(tabId: number | undefined, workspace: string): Promise<number> {
  // Even when an explicit tabId is provided, validate it is still debuggable.
  // This prevents issues when extensions hijack the tab URL to chrome-extension://
  // or when the tab has been closed by the user.
  if (tabId !== undefined && borrowedTabWorkspaces.get(tabId) === workspace) {
    try {
      const tab = await chrome.tabs.get(tabId);
      if (isDebuggableUrl(tab.url)) return tabId;
      borrowedTabWorkspaces.delete(tabId);
    } catch {
      borrowedTabWorkspaces.delete(tabId);
    }
  }
  if (tabId !== undefined && automationSessions.get(workspace)?.tabIds.has(tabId)) {
    try {
      const tab = await chrome.tabs.get(tabId);
      if (isDebuggableUrl(tab.url)) {
        touchSession(workspace, tabId);
        return tabId;
      }
      // Tab exists but URL is not debuggable — fall through to auto-resolve
      console.warn(`[opencli] Tab ${tabId} URL is not debuggable (${tab.url}), re-resolving`);
    } catch {
      // Tab was closed — fall through to auto-resolve
      console.warn(`[opencli] Tab ${tabId} no longer exists, re-resolving`);
    }
  } else if (tabId !== undefined) {
    // A daemon command must never make a user-owned tab an automation target.
    console.warn(`[opencli] Ignoring tab ${tabId} outside OpenCLI workspace ${workspace}`);
  }

  // Get (or create) the dedicated automation window; only reuse its own tabs.
  const windowId = await getAutomationWindow(workspace);

  const session = automationSessions.get(workspace);
  if (session) {
    for (const ownedTabId of session.tabIds) {
      try {
        const tab = await chrome.tabs.get(ownedTabId);
        if (isDebuggableUrl(tab.url)) {
          touchSession(workspace, ownedTabId);
          return ownedTabId;
        }
      } catch {
        forgetTab(workspace, ownedTabId);
      }
    }
  }

  // No reusable OpenCLI tab: create an inactive tab in the minimized window.
  const newTab = await chrome.tabs.create({ windowId, url: 'data:text/html,<html></html>', active: false });
  if (!newTab.id) throw new Error('Failed to create tab in automation window');
  trackTab(workspace, newTab.id);
  await enforcePageCacheLimit(newTab.id);
  return newTab.id;
}

async function findExactUserTab(targetUrl: string): Promise<number | undefined> {
  const tabs = await chrome.tabs.query({});
  const match = tabs
    .filter((tab) => tab.id !== undefined && tab.windowId !== automationWindowId && isDebuggableUrl(tab.url))
    .filter((tab) => isSameNavigationTarget(tab.url ?? '', targetUrl))
    .sort((left, right) => Number(Boolean(right.active)) - Number(Boolean(left.active)))[0];
  return match?.id;
}

async function listAutomationTabs(workspace: string): Promise<chrome.tabs.Tab[]> {
  const session = automationSessions.get(workspace);
  if (!session) return [];

  const tabs: chrome.tabs.Tab[] = [];
  for (const tabId of session.tabIds) {
    try {
      tabs.push(await chrome.tabs.get(tabId));
    } catch {
      forgetTab(workspace, tabId);
    }
  }
  if (!session.tabIds.size) automationSessions.delete(workspace);
  return tabs;
}

async function listAutomationWebTabs(workspace: string): Promise<chrome.tabs.Tab[]> {
  const tabs = await listAutomationTabs(workspace);
  return tabs.filter((tab) => isDebuggableUrl(tab.url));
}

async function handleExec(cmd: Command, workspace: string): Promise<Result> {
  if (!cmd.code) return { id: cmd.id, ok: false, error: 'Missing code' };
  const tabId = await resolveTabId(cmd.tabId, workspace);
  try {
    const data = await executor.evaluateAsync(tabId, cmd.code);
    return { id: cmd.id, ok: true, data };
  } catch (err) {
    return { id: cmd.id, ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}

async function handleUpload(cmd: Command, workspace: string): Promise<Result> {
  if (!cmd.selector) return { id: cmd.id, ok: false, error: 'Missing file input selector' };
  if (!cmd.file_paths?.length) return { id: cmd.id, ok: false, error: 'No files supplied for upload' };
  const tabId = await resolveTabId(cmd.tabId, workspace);
  try {
    await executor.uploadFiles(tabId, cmd.selector, cmd.file_paths);
    return { id: cmd.id, ok: true, data: { uploaded: cmd.file_paths.length } };
  } catch (err) {
    return { id: cmd.id, ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}

async function handleNavigate(cmd: Command, workspace: string): Promise<Result> {
  if (!cmd.url) return { id: cmd.id, ok: false, error: 'Missing url' };
  const targetUrl = cmd.url;
  const borrowedTabId = cmd.reuse_existing_tab ? await findExactUserTab(targetUrl) : undefined;
  const tabId = borrowedTabId ?? await resolveTabId(cmd.tabId, workspace);
  if (borrowedTabId !== undefined) {
    borrowedTabWorkspaces.set(borrowedTabId, workspace);
    console.log(`[navigate] BORROWED existing user tab url=${targetUrl} tabId=${borrowedTabId}`);
  }

  // Capture the current URL before navigation to detect actual URL change
  const beforeTab = await chrome.tabs.get(tabId);
  const beforeUrl = beforeTab.url ?? '';
  const waitUntilCommit = cmd.wait_until === 'commit';

  // A same-origin API fetch routinely asks to navigate to the page that is
  // already open. Chrome does not emit a URL-change event for that no-op, so
  // the old code waited for its 15-second fallback on every cache hit. Keep
  // the existing document and go directly to the page-context fetch instead.
  if (isSameNavigationTarget(beforeUrl, targetUrl)) {
    touchSession(workspace, tabId);
    console.log(`[navigate] REUSED url=${targetUrl} tabId=${tabId}`);
    return {
      id: cmd.id,
      ok: true,
      data: { title: beforeTab.title, url: beforeTab.url, tabId, timedOut: false, reused: true },
    };
  }

  // Detach any existing debugger before top-level navigation.
  // Some sites can invalidate the current inspected target during navigation,
  // which leaves a stale CDP attach
  // state and causes the next Runtime.evaluate to fail with
  // "Inspected target navigated or closed". Resetting here forces a clean
  // re-attach after navigation.
  await executor.detach(tabId);

  await chrome.tabs.update(tabId, { url: targetUrl });

  // A normal navigation waits for URL change + complete. Same-origin API
  // fetches only need the document to commit, so `wait_until: commit` skips
  // slow subresources without affecting normal adapter navigation.
  let timedOut = false;
  await new Promise<void>((resolve) => {
    let urlChanged = false;

    const listener = (id: number, info: chrome.tabs.TabChangeInfo, tab: chrome.tabs.Tab) => {
      if (id !== tabId) return;

      // Track URL change — skip about:blank and data: which are transient
      // intermediate states during navigation, not the actual destination.
      if (info.url && info.url !== beforeUrl &&
          !info.url.startsWith('about:') && !info.url.startsWith('data:')) {
        urlChanged = true;
      }

      if (urlChanged && (waitUntilCommit || info.status === 'complete')) {
        chrome.tabs.onUpdated.removeListener(listener);
        resolve();
      }
    };
    chrome.tabs.onUpdated.addListener(listener);

    // Also check if the tab already navigated (e.g. instant cache hit)
    setTimeout(async () => {
      try {
        const currentTab = await chrome.tabs.get(tabId);
        if (currentTab.url && currentTab.url !== beforeUrl &&
            !currentTab.url.startsWith('about:') && !currentTab.url.startsWith('data:') &&
            (waitUntilCommit || currentTab.status === 'complete')) {
          chrome.tabs.onUpdated.removeListener(listener);
          resolve();
        }
      } catch { /* tab gone */ }
    }, 100);

    // Timeout fallback with warning
    setTimeout(() => {
      chrome.tabs.onUpdated.removeListener(listener);
      timedOut = true;
      console.warn(`[opencli] Navigate to ${targetUrl} timed out after 15s`);
      resolve();
    }, 15000);
  });

  const tab = await chrome.tabs.get(tabId);
  if (timedOut) {
    console.warn(`[navigate] TIMEOUT url=${targetUrl} tabId=${tabId} finalUrl=${tab.url ?? 'unknown'}`);
  } else {
    console.log(`[navigate] OK url=${targetUrl} tabId=${tabId} finalUrl=${tab.url ?? 'unknown'}`);
  }
  return {
    id: cmd.id,
    ok: true,
    data: { title: tab.title, url: tab.url, tabId, timedOut },
  };
}

async function handleTabs(cmd: Command, workspace: string): Promise<Result> {
  switch (cmd.op) {
    case 'list': {
      const tabs = await listAutomationWebTabs(workspace);
      const data = tabs
        .map((t, i) => ({
          index: i,
          tabId: t.id,
          url: t.url,
          title: t.title,
          active: t.active,
        }));
      return { id: cmd.id, ok: true, data };
    }
    case 'new': {
      const windowId = await getAutomationWindow(workspace);
      const tab = await chrome.tabs.create({ windowId, url: cmd.url ?? 'data:text/html,<html></html>', active: false });
      if (tab.id) {
        trackTab(workspace, tab.id);
        await enforcePageCacheLimit(tab.id);
      }
      return { id: cmd.id, ok: true, data: { tabId: tab.id, url: tab.url } };
    }
    case 'close': {
      if (cmd.index !== undefined) {
        const tabs = await listAutomationWebTabs(workspace);
        const target = tabs[cmd.index];
        if (!target?.id) return { id: cmd.id, ok: false, error: `Tab index ${cmd.index} not found` };
        await chrome.tabs.remove(target.id);
        await executor.detach(target.id);
        forgetTab(workspace, target.id);
        return { id: cmd.id, ok: true, data: { closed: target.id } };
      }
      const tabId = await resolveTabId(cmd.tabId, workspace);
      await chrome.tabs.remove(tabId);
      await executor.detach(tabId);
      forgetTab(workspace, tabId);
      return { id: cmd.id, ok: true, data: { closed: tabId } };
    }
    case 'select': {
      if (cmd.index === undefined && cmd.tabId === undefined)
        return { id: cmd.id, ok: false, error: 'Missing index or tabId' };
      if (cmd.tabId !== undefined) {
        await chrome.tabs.update(cmd.tabId, { active: true });
        return { id: cmd.id, ok: true, data: { selected: cmd.tabId } };
      }
      const tabs = await listAutomationWebTabs(workspace);
      const target = tabs[cmd.index!];
      if (!target?.id) return { id: cmd.id, ok: false, error: `Tab index ${cmd.index} not found` };
      await chrome.tabs.update(target.id, { active: true });
      return { id: cmd.id, ok: true, data: { selected: target.id } };
    }
    default:
      return { id: cmd.id, ok: false, error: `Unknown tabs op: ${cmd.op}` };
  }
}

async function handleCookies(cmd: Command): Promise<Result> {
  const details: chrome.cookies.GetAllDetails = {};
  if (cmd.domain) details.domain = cmd.domain;
  if (cmd.url) details.url = cmd.url;
  const cookies = await chrome.cookies.getAll(details);
  const data = cookies.map((c) => ({
    name: c.name,
    value: c.value,
    domain: c.domain,
    path: c.path,
    secure: c.secure,
    httpOnly: c.httpOnly,
    expirationDate: c.expirationDate,
  }));
  return { id: cmd.id, ok: true, data };
}

async function handleScreenshot(cmd: Command, workspace: string): Promise<Result> {
  const tabId = await resolveTabId(cmd.tabId, workspace);
  try {
    const data = await executor.screenshot(tabId, {
      format: cmd.format,
      quality: cmd.quality,
      fullPage: cmd.fullPage,
    });
    return { id: cmd.id, ok: true, data };
  } catch (err) {
    return { id: cmd.id, ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}

async function handleCloseWindow(cmd: Command, workspace: string): Promise<Result> {
  const session = automationSessions.get(workspace);
  if (session) {
    try {
      const tabIdArray = [...session.tabIds];
      if (tabIdArray.length) await chrome.tabs.remove(tabIdArray);
      await Promise.all(tabIdArray.map(tabId => executor.detach(tabId)));
    } catch {
      // Already gone
    }
    automationSessions.delete(workspace);
  }
  return { id: cmd.id, ok: true, data: { closed: true } };
}

/**
 * Run a fetch request from the service worker background context.
 * `Cookie` is a forbidden request header in browser Fetch. Let Chrome attach
 * the target site's eligible login cookies via `credentials: 'include'` rather
 * than trying to inject a header that can cause `TypeError: Failed to fetch`.
 */
async function handleBgFetch(cmd: Command): Promise<Result> {
  if (!cmd.url) return { id: cmd.id, ok: false, error: 'Missing url' };

  const headers: Record<string, string> = {
    ...(cmd.request_headers ?? {}),
  };

  const t0 = Date.now();
  let response: Response;
  try {
    response = await fetch(cmd.url, {
      method: cmd.method ?? 'GET',
      headers,
      body: cmd.body,
      credentials: 'include',
    });
  } catch (err) {
    const elapsed = Date.now() - t0;
    console.error(`[bg_fetch] NETWORK_ERROR url=${cmd.url} elapsed=${elapsed}ms err=${String(err)}`);
    return { id: cmd.id, ok: false, error: String(err) };
  }

  const elapsed = Date.now() - t0;
  const contentType = response.headers.get('content-type') ?? '';
  const body = contentType.includes('application/json')
    ? await response.json()
    : await response.text();

  const bodySize = typeof body === 'string' ? body.length : JSON.stringify(body).length;

  if (!response.ok) {
    const preview = typeof body === 'string'
      ? body.slice(0, 400)
      : JSON.stringify(body).slice(0, 400);
    console.warn(`[bg_fetch] FAILED url=${cmd.url} status=${response.status} contentType=${contentType} bodySize=${bodySize} elapsed=${elapsed}ms preview=${preview}`);
  } else {
    console.log(`[bg_fetch] OK url=${cmd.url} status=${response.status} contentType=${contentType} bodySize=${bodySize} elapsed=${elapsed}ms`);
  }

  return { id: cmd.id, ok: response.ok, data: { status: response.status, body } };
}

async function handleSessions(cmd: Command): Promise<Result> {
  const data = await Promise.all([...automationSessions.entries()].map(async ([workspace, session]) => ({
    workspace,
    windowId: automationWindowId,
    tabCount: (await listAutomationWebTabs(workspace)).length,
    lastUsedAt: session.lastUsedAt,
  })));
  return { id: cmd.id, ok: true, data };
}

async function getAutomationState(): Promise<{ capacity: number; count: number; windowId: number | null; pages: Array<{ workspace: string; tabId: number; url: string; title: string; lastUsedAt: number }> }> {
  await ensureAutomationStateLoaded();
  const pages: Array<{ workspace: string; tabId: number; url: string; title: string; lastUsedAt: number }> = [];
  for (const [workspace, session] of [...automationSessions.entries()]) {
    for (const tab of await listAutomationWebTabs(workspace)) {
      if (tab.id) pages.push({
        workspace,
        tabId: tab.id,
        url: tab.url ?? '',
        title: tab.title ?? '',
        lastUsedAt: session.tabLastUsedAt.get(tab.id) ?? session.lastUsedAt,
      });
    }
  }
  return { capacity: PAGE_CACHE_LIMIT, count: pages.length, windowId: automationWindowId, pages };
}

// ─── Popup / chrome.runtime message handler ──────────────────────────

chrome.runtime.onMessage.addListener((message: { type: string; message?: string; action?: import('./page_actions').DirectContextAction; expectedUrl?: string }, _sender, sendResponse) => {
  if (message.type === 'popupLog') {
    console.info(message.message ?? '[popup] missing diagnostic message');
    sendResponse({ ok: true });
    return false;
  }
  if (message.type === 'runCurrentPageAction') {
    if (!message.action) {
      sendResponse({ ok: false, error: '缺少当前页面 action 配置' });
      return false;
    }
    void runCurrentPageAction(message.action, message.expectedUrl ?? '')
      .then((result) => sendResponse({ ok: true, result }))
      .catch((error) => sendResponse({ ok: false, error: error instanceof Error ? error.message : String(error) }));
    return true;
  }
  if (message.type === 'getPort') {
    void getStoredPortConfig().then(({ port }) => {
      sendResponse({ port: port ?? DAEMON_PORT });
    });
    return true; // async response
  }
  if (message.type === 'getConnectionState') {
    void getConnectionState().then((state) => {
      sendResponse(state);
    });
    return true;
  }
  if (message.type === 'getAutomationState') {
    void getAutomationState().then(sendResponse);
    return true;
  }
  if (message.type === 'setPort') {
    const port = (message as { type: string; port: number }).port;
    void storePort(port, true).then(async () => {
      reconnectAttempts = 0;
      if (reconnectTimer) {
        clearTimeout(reconnectTimer);
        reconnectTimer = null;
      }
      if (ws) {
        try { ws.close(); } catch { /* ignore */ }
        ws = null;
        connectedPort = null;
      }
      const state = await connect();
      sendResponse(state);
    });
    return true;
  }
  return false;
});

export const __test__ = {
  handleTabs,
  handleSessions,
  handleNavigate,
  resolveTabId,
  getAutomationState,
  isSameNavigationTarget,
  getAutomationWindowId: () => automationWindowId,
  setAutomationWindowId: (workspace: string, windowId: number | null) => {
    automationStateLoaded = Promise.resolve();
    if (windowId === null) {
      automationWindowId = null;
      automationSessions.delete(workspace);
      return;
    }
    automationWindowId = windowId;
    automationSessions.set(workspace, {
      tabIds: new Set<number>(),
      tabLastUsedAt: new Map<number, number>(),
      lastUsedAt: nextAccessAt(),
    });
  },
};

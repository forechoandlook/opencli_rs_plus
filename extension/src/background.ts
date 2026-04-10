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
  getStoredPort,
  storePort,
  WS_RECONNECT_BASE_DELAY,
  WS_RECONNECT_MAX_DELAY,
} from './protocol';
import * as executor from './cdp';

let ws: WebSocket | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
let reconnectAttempts = 0;
let connectedPort: number | null = null;

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

async function connect(): Promise<void> {
  // Load saved port, then actively scan the daemon port range when needed.
  const savedPort = await getStoredPort();
  const port = (await detectDaemonPort(savedPort)) ?? savedPort ?? DAEMON_PORT;
  if ((ws?.readyState === WebSocket.OPEN || ws?.readyState === WebSocket.CONNECTING) && connectedPort === port) {
    return;
  }
  if (ws && connectedPort !== null && connectedPort !== port) {
    try { ws.close(); } catch { /* ignore */ }
    ws = null;
  }
  if (port !== savedPort) {
    await storePort(port);
  }
  const wsUrl = daemonWsUrl(port);
  try {
    ws = new WebSocket(wsUrl);
  } catch {
    scheduleReconnect();
    return;
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
// Reuses existing user Chrome windows when available (tabs in background).
// Only creates new windows if none exist. Tracks tab IDs so we only close
// our own tabs (not user's tabs or windows).

type AutomationSession = {
  windowId: number;
  ownWindow: boolean;          // true = we created this window, safe to close it
  tabIds: Set<number>;         // tabs we created, for selective cleanup
  idleTimer: ReturnType<typeof setTimeout> | null;
  idleDeadlineAt: number;
};

const automationSessions = new Map<string, AutomationSession>();
const WINDOW_IDLE_TIMEOUT = 120000; // 120s

function getWorkspaceKey(workspace?: string): string {
  return workspace?.trim() || 'default';
}

function resetWindowIdleTimer(workspace: string): void {
  const session = automationSessions.get(workspace);
  if (!session) return;
  if (session.idleTimer) clearTimeout(session.idleTimer);
  session.idleDeadlineAt = Date.now() + WINDOW_IDLE_TIMEOUT;
  session.idleTimer = setTimeout(async () => {
    const current = automationSessions.get(workspace);
    if (!current) return;
    try {
      if (current.ownWindow) {
        // We own the window — safe to close it
        await chrome.windows.remove(current.windowId);
        console.log(`[opencli] Automation window ${current.windowId} (${workspace}) closed (idle timeout)`);
      } else {
        // Reusing user's window — only close our tabs
        const tabIdArray = [...current.tabIds];
        if (tabIdArray.length) {
          await chrome.tabs.remove(tabIdArray);
          console.log(`[opencli] Automation tabs [${tabIdArray.join(',')}] (${workspace}) closed (idle timeout)`);
        }
      }
    } catch (err) {
      console.error(`[opencli] Error cleaning up session: ${err}`);
    }
    automationSessions.delete(workspace);
  }, WINDOW_IDLE_TIMEOUT);
}

/**
 * Get (or create) the automation session for a workspace.
 * Prefers to reuse an existing user Chrome window (tabs are background).
 * Only creates a new small window if no existing windows are available.
 */
async function getAutomationWindow(workspace: string): Promise<number> {
  const existing = automationSessions.get(workspace);
  if (existing) {
    try {
      await chrome.windows.get(existing.windowId);
      return existing.windowId;
    } catch {
      // Window was closed externally
      automationSessions.delete(workspace);
    }
  }

  // Try to reuse an existing user window
  const windows = await chrome.windows.getAll({ windowTypes: ['normal'] });
  const userWindow = windows.find(w => !w.incognito && w.id !== undefined);

  let windowId: number;
  let ownWindow: boolean;

  if (userWindow?.id !== undefined) {
    windowId = userWindow.id;
    ownWindow = false;
    console.log(`[opencli] Reusing existing window ${windowId} (${workspace})`);
  } else {
    // No existing window — create a small one as fallback
    let win;
    try {
      win = await chrome.windows.create({
        url: 'data:text/html,<html></html>',
        focused: false,
        width: 200,
        height: 200,
        left: 0,
        top: 0,
        type: 'normal',
      });
    } catch (err) {
      console.error(`[opencli] Failed to create automation window: ${err}`);
      throw err;
    }
    if (!win.id) {
      console.error(`[opencli] Window created but no ID`);
      throw new Error('Failed to create automation window: no window ID');
    }
    windowId = win.id;
    ownWindow = true;
    console.log(`[opencli] Created automation window ${windowId} (${workspace})`);
    await new Promise(resolve => setTimeout(resolve, 200));
  }

  const session: AutomationSession = {
    windowId,
    ownWindow,
    tabIds: new Set(),
    idleTimer: null,
    idleDeadlineAt: Date.now() + WINDOW_IDLE_TIMEOUT,
  };
  automationSessions.set(workspace, session);
  resetWindowIdleTimer(workspace);
  return windowId;
}

// Clean up if our automation window closes
chrome.windows.onRemoved.addListener((windowId) => {
  for (const [workspace, session] of automationSessions.entries()) {
    if (session.windowId === windowId && session.ownWindow) {
      console.log(`[opencli] Automation window closed (${workspace})`);
      if (session.idleTimer) clearTimeout(session.idleTimer);
      automationSessions.delete(workspace);
    }
  }
});

// Track when our tabs are closed externally
chrome.tabs.onRemoved.addListener((tabId) => {
  for (const session of automationSessions.values()) {
    session.tabIds.delete(tabId);
  }
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
  const workspace = getWorkspaceKey(cmd.workspace);
  // Reset idle timer on every command (window stays alive while active)
  resetWindowIdleTimer(workspace);
  try {
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
      default:
        return { id: cmd.id, ok: false, error: `Unknown action: ${cmd.action}` };
    }
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

/**
 * Resolve target tab in the automation window.
 * If explicit tabId is given, use that directly.
 * Otherwise, find or create a tab in the dedicated automation window.
 */
async function resolveTabId(tabId: number | undefined, workspace: string): Promise<number> {
  // Even when an explicit tabId is provided, validate it is still debuggable.
  // This prevents issues when extensions hijack the tab URL to chrome-extension://
  // or when the tab has been closed by the user.
  if (tabId !== undefined) {
    try {
      const tab = await chrome.tabs.get(tabId);
      if (isDebuggableUrl(tab.url)) return tabId;
      // Tab exists but URL is not debuggable — fall through to auto-resolve
      console.warn(`[opencli] Tab ${tabId} URL is not debuggable (${tab.url}), re-resolving`);
    } catch {
      // Tab was closed — fall through to auto-resolve
      console.warn(`[opencli] Tab ${tabId} no longer exists, re-resolving`);
    }
  }

  // Get (or create) the automation window
  const windowId = await getAutomationWindow(workspace);

  // Prefer an existing debuggable tab
  const tabs = await chrome.tabs.query({ windowId });
  const debuggableTab = tabs.find(t => t.id && isDebuggableUrl(t.url));
  if (debuggableTab?.id) return debuggableTab.id;

  // No debuggable tab — another extension may have hijacked the tab URL.
  // Try to reuse by navigating to a data: URI (not interceptable by New Tab Override).
  const reuseTab = tabs.find(t => t.id);
  if (reuseTab?.id) {
    await chrome.tabs.update(reuseTab.id, { url: 'data:text/html,<html></html>' });
    await new Promise(resolve => setTimeout(resolve, 300));
    try {
      const updated = await chrome.tabs.get(reuseTab.id);
      if (isDebuggableUrl(updated.url)) return reuseTab.id;
      console.warn(`[opencli] data: URI was intercepted (${updated.url}), creating fresh tab`);
    } catch {
      // Tab was closed during navigation
    }
  }

  // Fallback: create a new background tab
  const newTab = await chrome.tabs.create({ windowId, url: 'data:text/html,<html></html>', active: false });
  if (!newTab.id) throw new Error('Failed to create tab in automation window');
  automationSessions.get(workspace)?.tabIds.add(newTab.id);
  return newTab.id;
}

async function listAutomationTabs(workspace: string): Promise<chrome.tabs.Tab[]> {
  const session = automationSessions.get(workspace);
  if (!session) return [];

  if (session.ownWindow) {
    // We own the window — all tabs are ours
    try {
      return await chrome.tabs.query({ windowId: session.windowId });
    } catch {
      automationSessions.delete(workspace);
      return [];
    }
  }

  // Reusing user's window — only return tabs we created
  const tabs: chrome.tabs.Tab[] = [];
  for (const tabId of session.tabIds) {
    try {
      const tab = await chrome.tabs.get(tabId);
      tabs.push(tab);
    } catch {
      session.tabIds.delete(tabId); // tab was closed
    }
  }
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

async function handleNavigate(cmd: Command, workspace: string): Promise<Result> {
  if (!cmd.url) return { id: cmd.id, ok: false, error: 'Missing url' };
  const tabId = await resolveTabId(cmd.tabId, workspace);

  // Capture the current URL before navigation to detect actual URL change
  const beforeTab = await chrome.tabs.get(tabId);
  const beforeUrl = beforeTab.url ?? '';
  const targetUrl = cmd.url;

  // Detach any existing debugger before top-level navigation.
  // Some sites (observed on creator.xiaohongshu.com flows) can invalidate the
  // current inspected target during navigation, which leaves a stale CDP attach
  // state and causes the next Runtime.evaluate to fail with
  // "Inspected target navigated or closed". Resetting here forces a clean
  // re-attach after navigation.
  await executor.detach(tabId);

  await chrome.tabs.update(tabId, { url: targetUrl });

  // Wait for: 1) URL to change from the old URL, 2) tab.status === 'complete'
  // This avoids the race where 'complete' fires for the OLD URL (e.g. about:blank)
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

      // Only resolve when both URL has changed AND status is complete
      if (urlChanged && info.status === 'complete') {
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
            currentTab.status === 'complete') {
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
      if (tab.id) automationSessions.get(workspace)?.tabIds.add(tab.id);
      return { id: cmd.id, ok: true, data: { tabId: tab.id, url: tab.url } };
    }
    case 'close': {
      if (cmd.index !== undefined) {
        const tabs = await listAutomationWebTabs(workspace);
        const target = tabs[cmd.index];
        if (!target?.id) return { id: cmd.id, ok: false, error: `Tab index ${cmd.index} not found` };
        await chrome.tabs.remove(target.id);
        await executor.detach(target.id);
        return { id: cmd.id, ok: true, data: { closed: target.id } };
      }
      const tabId = await resolveTabId(cmd.tabId, workspace);
      await chrome.tabs.remove(tabId);
      await executor.detach(tabId);
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
      if (session.ownWindow) {
        // We own the window — safe to close it
        await chrome.windows.remove(session.windowId);
      } else {
        // Reusing user's window — only close our tabs
        const tabIdArray = [...session.tabIds];
        if (tabIdArray.length) await chrome.tabs.remove(tabIdArray);
      }
    } catch {
      // Already gone
    }
    if (session.idleTimer) clearTimeout(session.idleTimer);
    automationSessions.delete(workspace);
  }
  return { id: cmd.id, ok: true, data: { closed: true } };
}

/**
 * Run a fetch request from the service worker background context.
 * Cookies for the target domain are collected via chrome.cookies and injected
 * as a Cookie header — no tab or window is opened.
 */
async function handleBgFetch(cmd: Command): Promise<Result> {
  if (!cmd.url) return { id: cmd.id, ok: false, error: 'Missing url' };

  const cookieUrl = cmd.cookie_url ?? cmd.url;
  const cookies = await chrome.cookies.getAll({ url: cookieUrl });
  const cookieHeader = cookies.map(c => `${c.name}=${c.value}`).join('; ');

  const headers: Record<string, string> = {
    ...(cmd.request_headers ?? {}),
  };
  if (cookieHeader) headers['Cookie'] = cookieHeader;

  const response = await fetch(cmd.url, {
    method: cmd.method ?? 'GET',
    headers,
    body: cmd.body,
  });

  const contentType = response.headers.get('content-type') ?? '';
  const body = contentType.includes('application/json')
    ? await response.json()
    : await response.text();

  return { id: cmd.id, ok: response.ok, data: { status: response.status, body } };
}

async function handleSessions(cmd: Command): Promise<Result> {
  const now = Date.now();
  const data = await Promise.all([...automationSessions.entries()].map(async ([workspace, session]) => ({
    workspace,
    windowId: session.windowId,
    tabCount: (await listAutomationWebTabs(workspace)).length,
    idleMsRemaining: Math.max(0, session.idleDeadlineAt - now),
  })));
  return { id: cmd.id, ok: true, data };
}

// ─── Popup / chrome.runtime message handler ──────────────────────────

chrome.runtime.onMessage.addListener((message: { type: string }, _sender, sendResponse) => {
  if (message.type === 'getPort') {
    void getStoredPort().then((port) => {
      sendResponse({ port: port ?? DAEMON_PORT });
    });
    return true; // async response
  }
  if (message.type === 'setPort') {
    const port = (message as { type: string; port: number }).port;
    void storePort(port).then(async () => {
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
      await connect();
    });
    sendResponse({ ok: true });
    return true;
  }
  return false;
});

export const __test__ = {
  handleTabs,
  handleSessions,
  getAutomationWindowId: (workspace: string = 'default') => automationSessions.get(workspace)?.windowId ?? null,
  setAutomationWindowId: (workspace: string, windowId: number | null) => {
    if (windowId === null) {
      const session = automationSessions.get(workspace);
      if (session?.idleTimer) clearTimeout(session.idleTimer);
      automationSessions.delete(workspace);
      return;
    }
    automationSessions.set(workspace, {
      windowId,
      ownWindow: false,
      tabIds: new Set<number>(),
      idleTimer: null,
      idleDeadlineAt: Date.now() + WINDOW_IDLE_TIMEOUT,
    });
  },
};

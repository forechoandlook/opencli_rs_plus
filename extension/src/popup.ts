/**
 * OpenCLI — Browser Action Popup
 *
 * Lets the user set the daemon port and shows connection status.
 */

import { EXTENSION_API_PORT, getStoredPortConfig, storePort, DAEMON_PORT } from './protocol';

function setStatus(el: HTMLElement, text: string, color: string): void {
  el.textContent = text;
  el.style.color = color;
}

type RuntimeState = {
  configuredPort: number;
  pinned: boolean;
  connectedPort: number | null;
  connected: boolean;
};

type BrowserTask = {
  id: string;
  workspace: string;
  status: 'running' | 'done' | 'failed';
  started_at_ms: number;
  finished_at_ms?: number;
  result_preview?: string;
  error?: string;
};

type CachedPage = {
  workspace: string;
  tabId: number;
  url: string;
  title: string;
  lastUsedAt: number;
};

type AutomationState = {
  capacity: number;
  count: number;
  windowId: number | null;
  pages: CachedPage[];
};

type ContextAction = {
  adapter: string;
  title: string;
  description: string;
  activeTab: { usePipeline?: boolean; extract?: string };
  args?: Record<string, string>;
  pipeline?: unknown[];
};

function safePageLabel(url: string): string {
  try {
    const parsed = new URL(url);
    return `${parsed.hostname}${parsed.pathname}`;
  } catch {
    return 'invalid-url';
  }
}

function logPopup(message: string): void {
  // The background worker forwards these sanitized diagnostics to the existing
  // browser-daemon extension log. Never include the query string: it can carry
  // a short-lived site signature such as xsec_token.
  void chrome.runtime.sendMessage({ type: 'popupLog', message: `[popup] ${message}` }).catch(() => undefined);
}

async function getRuntimeState(fallbackPort: number, fallbackPinned: boolean): Promise<RuntimeState> {
  try {
    return await chrome.runtime.sendMessage({ type: 'getConnectionState' }) as RuntimeState;
  } catch {
    return {
      configuredPort: fallbackPort,
      pinned: fallbackPinned,
      connectedPort: null,
      connected: false,
    };
  }
}

function renderStatus(state: RuntimeState): { text: string; color: string } {
  const mode = state.pinned ? 'Pinned' : 'Auto';
  if (state.connected && state.connectedPort !== null) {
    return {
      text: `${mode} configured ${state.configuredPort}, connected ${state.connectedPort}`,
      color: '#0d0',
    };
  }
  return {
    text: `${mode} configured ${state.configuredPort}, disconnected`,
    color: '#e55',
  };
}

function taskName(workspace: string): string {
  return workspace.startsWith('adapter:')
    ? workspace.slice('adapter:'.length).replace(':', ' ')
    : workspace.startsWith('origin:')
      ? workspace.slice('origin:'.length)
      : workspace;
}

function taskTime(task: BrowserTask): string {
  const time = task.finished_at_ms ?? task.started_at_ms;
  return new Date(time).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

async function getTasks(port: number): Promise<BrowserTask[]> {
  // Do not add a custom header here: it turns a local GET into a CORS
  // preflight request. The daemon authenticates popup reads by its
  // chrome-extension:// Origin instead.
  const response = await fetch(`http://127.0.0.1:${port}/tasks`);
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  const data = await response.json() as { tasks?: BrowserTask[] };
  return data.tasks ?? [];
}

async function getActivePageUrl(): Promise<string | null> {
  const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  const url = tab?.url;
  return url && /^https?:\/\//i.test(url) ? url : null;
}

async function getContextActions(url: string): Promise<ContextAction[]> {
  const response = await fetch(`http://127.0.0.1:${EXTENSION_API_PORT}/extension/actions?${new URLSearchParams({ url })}`);
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  const data = await response.json() as { actions?: ContextAction[] };
  return data.actions ?? [];
}

async function runContextAction(action: ContextAction, url: string): Promise<string> {
  const response = await chrome.runtime.sendMessage({
    type: 'runCurrentPageAction',
    action,
    expectedUrl: url,
  }) as { ok: boolean; result?: { message?: string }; error?: string };
  if (!response.ok) throw new Error(response.error ?? '当前页面操作失败');
  return response.result?.message ?? '已保存';
}

function renderContextActions(container: HTMLElement, url: string | null, actions: ContextAction[] | null, error?: string): void {
  if (!url) {
    container.innerHTML = '<p class="empty">当前标签不是网页，暂无可用操作。</p>';
    return;
  }
  if (error) {
    container.replaceChildren(contextPageDiagnostic(url, `读取失败：${error}`));
    return;
  }
  if (!actions?.length) {
    container.replaceChildren(contextPageDiagnostic(url, `0 个匹配操作 · API :${EXTENSION_API_PORT}`));
    return;
  }

  const hostname = new URL(url).hostname;
  const page = document.createElement('p');
  page.className = 'context-page';
  page.textContent = `${hostname}${new URL(url).pathname} · ${actions.length} 个操作 · API :${EXTENSION_API_PORT}`;
  const rows = actions.map((action) => {
    const row = document.createElement('article');
    row.className = 'context-action';
    const text = document.createElement('div');
    const title = document.createElement('div');
    title.className = 'task-title';
    title.textContent = action.title;
    const description = document.createElement('div');
    description.className = 'context-description';
    description.textContent = action.description;
    text.append(title, description);
    const button = document.createElement('button');
    button.textContent = '执行';
    button.addEventListener('click', async () => {
      button.disabled = true;
      button.textContent = '启动中…';
      try {
        button.textContent = await runContextAction(action, url);
      } catch (runError) {
        button.textContent = '失败';
        description.textContent = runError instanceof Error ? runError.message : String(runError);
      }
    });
    row.append(text, button);
    return row;
  });
  container.replaceChildren(page, ...rows);
}

function contextPageDiagnostic(url: string, message: string): HTMLElement {
  const diagnostic = document.createElement('p');
  diagnostic.className = 'empty';
  diagnostic.textContent = `${safePageLabel(url)} · ${message}`;
  return diagnostic;
}

function renderTasks(container: HTMLElement, tasks: BrowserTask[], available: boolean): void {
  if (!available) {
    container.innerHTML = '<p class="empty">Daemon 未连接；任务运行后会显示在这里。</p>';
    return;
  }
  if (!tasks.length) {
    container.innerHTML = '<p class="empty">暂时没有浏览器任务。</p>';
    return;
  }

  container.replaceChildren(...tasks.slice(0, 8).map((task) => {
    const item = document.createElement('article');
    item.className = 'task';
    const title = document.createElement('div');
    title.className = 'task-title';
    title.textContent = taskName(task.workspace);
    const meta = document.createElement('div');
    meta.className = 'task-meta';
    const status = document.createElement('span');
    status.className = `badge ${task.status}`;
    status.textContent = task.status === 'done' ? '完成' : task.status === 'failed' ? '失败' : '运行中';
    const time = document.createElement('span');
    time.textContent = taskTime(task);
    meta.append(status, time);
    item.append(title, meta);
    const output = task.error ?? task.result_preview;
    if (output) {
      const preview = document.createElement('pre');
      preview.textContent = output;
      item.append(preview);
    }
    return item;
  }));
}

async function getAutomationState(): Promise<AutomationState> {
  return await chrome.runtime.sendMessage({ type: 'getAutomationState' }) as AutomationState;
}

function renderCache(container: HTMLElement, state: AutomationState | null): void {
  if (!state) {
    container.innerHTML = '<p class="empty">扩展未连接。</p>';
    return;
  }
  if (!state.pages.length) {
    container.innerHTML = `<p class="empty">缓存为空（0 / ${state.capacity}）。</p>`;
    return;
  }

  const summary = document.createElement('p');
  summary.className = 'cache-summary';
  summary.textContent = `${state.count} / ${state.capacity} 页面缓存`;
  const rows = state.pages
    .sort((a, b) => b.lastUsedAt - a.lastUsedAt)
    .map((page) => {
      const row = document.createElement('article');
      row.className = 'cache-page';
      const name = document.createElement('div');
      name.className = 'task-title';
      name.textContent = taskName(page.workspace);
      const url = document.createElement('div');
      url.className = 'cache-url';
      url.textContent = page.url;
      row.append(name, url);
      return row;
    });
  container.replaceChildren(summary, ...rows);
}

async function init(): Promise<void> {
  const portInput = document.getElementById('port-input') as HTMLInputElement;
  const statusEl = document.getElementById('status') as HTMLElement;
  const saveBtn = document.getElementById('save-btn') as HTMLButtonElement;
  const refreshBtn = document.getElementById('refresh-btn') as HTMLButtonElement;
  const actionsEl = document.getElementById('context-actions') as HTMLElement;
  const tasksEl = document.getElementById('tasks') as HTMLElement;
  const cacheEl = document.getElementById('page-cache') as HTMLElement;

  if (!portInput || !statusEl || !saveBtn || !refreshBtn || !actionsEl || !tasksEl || !cacheEl) return;

  const refreshTasks = async (state: RuntimeState): Promise<void> => {
    refreshBtn.disabled = true;
    try {
      renderTasks(tasksEl, await getTasks(state.connectedPort ?? state.configuredPort), true);
    } catch {
      renderTasks(tasksEl, [], false);
    } finally {
      refreshBtn.disabled = false;
    }
  };

  const refreshCache = async (): Promise<void> => {
    try {
      renderCache(cacheEl, await getAutomationState());
    } catch {
      renderCache(cacheEl, null);
    }
  };

  const refreshActions = async (): Promise<void> => {
    const url = await getActivePageUrl();
    if (!url) {
      renderContextActions(actionsEl, null, null);
      return;
    }
    try {
      const actions = await getContextActions(url);
      logPopup(`context actions url=${safePageLabel(url)} count=${actions.length} api_port=${EXTENSION_API_PORT}`);
      renderContextActions(actionsEl, url, actions);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      logPopup(`context actions failed url=${safePageLabel(url)} error=${message}`);
      renderContextActions(actionsEl, url, null, message);
    }
  };

  // Load saved port
  const { port: savedPort, pinned } = await getStoredPortConfig();
  const initialPort = savedPort ?? DAEMON_PORT;
  portInput.value = String(initialPort);

  setStatus(statusEl, 'Checking…', '#888');
  const initialState = await getRuntimeState(initialPort, pinned);
  const initialRendered = renderStatus(initialState);
  portInput.value = String(initialState.configuredPort);
  setStatus(statusEl, initialRendered.text, initialRendered.color);
  await refreshTasks(initialState);
  await refreshCache();
  await refreshActions();

  refreshBtn.addEventListener('click', async () => {
    const state = await getRuntimeState(Number(portInput.value), initialState.pinned);
    await refreshTasks(state);
    await refreshCache();
    await refreshActions();
  });

  // Save button
  saveBtn.addEventListener('click', async () => {
    const port = parseInt(portInput.value, 10);
    if (!port || port < 1 || port > 65535) {
      setStatus(statusEl, 'Invalid port', '#e55');
      return;
    }

    await storePort(port, true);
    setStatus(statusEl, 'Switching…', '#888');
    let stateAfterSave = await getRuntimeState(port, true);
    try {
      stateAfterSave = await chrome.runtime.sendMessage({ type: 'setPort', port }) as RuntimeState;
    } catch { /* ignore */ }
    const rendered = renderStatus(stateAfterSave);
    portInput.value = String(stateAfterSave.configuredPort);
    setStatus(statusEl, rendered.text, rendered.color);
    await refreshTasks(stateAfterSave);
    await refreshCache();
    await refreshActions();
  });

  // Enter key to save
  portInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') saveBtn.click();
  });
}

document.addEventListener('DOMContentLoaded', () => {
  void init();
});

/**
 * OpenCLI — Browser Action Popup
 *
 * Lets the user set the daemon port and shows connection status.
 */

import { getStoredPortConfig, storePort, DAEMON_PORT } from './protocol';

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
  const tasksEl = document.getElementById('tasks') as HTMLElement;
  const cacheEl = document.getElementById('page-cache') as HTMLElement;

  if (!portInput || !statusEl || !saveBtn || !refreshBtn || !tasksEl || !cacheEl) return;

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

  refreshBtn.addEventListener('click', async () => {
    const state = await getRuntimeState(Number(portInput.value), initialState.pinned);
    await refreshTasks(state);
    await refreshCache();
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
  });

  // Enter key to save
  portInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') saveBtn.click();
  });
}

document.addEventListener('DOMContentLoaded', () => {
  void init();
});

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

async function init(): Promise<void> {
  const portInput = document.getElementById('port-input') as HTMLInputElement;
  const statusEl = document.getElementById('status') as HTMLElement;
  const saveBtn = document.getElementById('save-btn') as HTMLButtonElement;

  if (!portInput || !statusEl || !saveBtn) return;

  // Load saved port
  const { port: savedPort, pinned } = await getStoredPortConfig();
  const initialPort = savedPort ?? DAEMON_PORT;
  portInput.value = String(initialPort);

  setStatus(statusEl, 'Checking…', '#888');
  const initialState = await getRuntimeState(initialPort, pinned);
  const initialRendered = renderStatus(initialState);
  portInput.value = String(initialState.configuredPort);
  setStatus(statusEl, initialRendered.text, initialRendered.color);

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
  });

  // Enter key to save
  portInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') saveBtn.click();
  });
}

document.addEventListener('DOMContentLoaded', () => {
  void init();
});

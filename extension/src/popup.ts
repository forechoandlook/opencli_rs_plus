/**
 * OpenCLI — Browser Action Popup
 *
 * Lets the user set the daemon port and shows connection status.
 */

import { checkDaemonConnection, getStoredPortConfig, storePort, DAEMON_PORT } from './protocol';

function setStatus(el: HTMLElement, text: string, color: string): void {
  el.textContent = text;
  el.style.color = color;
}

async function init(): Promise<void> {
  const portInput = document.getElementById('port-input') as HTMLInputElement;
  const statusEl = document.getElementById('status') as HTMLElement;
  const saveBtn = document.getElementById('save-btn') as HTMLButtonElement;

  if (!portInput || !statusEl || !saveBtn) return;

  // Load saved port
  const { port: savedPort, pinned } = await getStoredPortConfig();
  if (savedPort !== null) {
    portInput.value = String(savedPort);
  }

  // Popup should reflect the current stored choice quickly instead of blocking on
  // multi-port auto-detection. Background can keep its own detection logic.
  setStatus(statusEl, 'Checking…', '#888');
  const currentPort = savedPort ?? DAEMON_PORT;
  const ok = await checkDaemonConnection(currentPort, 700);
  if (ok) {
    setStatus(statusEl, pinned ? `Pinned (${currentPort}) connected` : `Auto (${currentPort}) connected`, '#0d0');
  } else {
    setStatus(statusEl, pinned ? `Pinned (${currentPort}) not connected` : `Auto (${currentPort}) not connected`, '#e55');
  }

  // Save button
  saveBtn.addEventListener('click', async () => {
    const port = parseInt(portInput.value, 10);
    if (!port || port < 1 || port > 65535) {
      setStatus(statusEl, 'Invalid port', '#e55');
      return;
    }

    await storePort(port, true);
    try {
      await chrome.runtime.sendMessage({ type: 'setPort', port });
    } catch { /* ignore */ }
    setStatus(statusEl, 'Checking…', '#888');
    const ok = await checkDaemonConnection(port);
    if (ok) {
      setStatus(statusEl, 'Saved — Connected', '#0d0');
    } else {
      setStatus(statusEl, `Saved (daemon not on ${port})`, '#e55');
    }
  });

  // Enter key to save
  portInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') saveBtn.click();
  });
}

document.addEventListener('DOMContentLoaded', () => {
  void init();
});

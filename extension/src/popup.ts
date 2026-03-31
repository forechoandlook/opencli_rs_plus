/**
 * OpenCLI — Browser Action Popup
 *
 * Lets the user set the daemon port and shows connection status.
 */

import { getStoredPort, storePort, daemonWsUrl, DAEMON_PORT } from './protocol';

function setStatus(el: HTMLElement, text: string, color: string): void {
  el.textContent = text;
  el.style.color = color;
}

async function checkConnection(port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const ws = new WebSocket(daemonWsUrl(port));
    const timer = setTimeout(() => { ws.close(); resolve(false); }, 2000);
    ws.onopen = () => { clearTimeout(timer); ws.close(); resolve(true); };
    ws.onerror = () => { clearTimeout(timer); resolve(false); };
  });
}

async function init(): Promise<void> {
  const portInput = document.getElementById('port-input') as HTMLInputElement;
  const statusEl = document.getElementById('status') as HTMLElement;
  const saveBtn = document.getElementById('save-btn') as HTMLButtonElement;

  if (!portInput || !statusEl || !saveBtn) return;

  // Load saved port
  const savedPort = await getStoredPort();
  if (savedPort !== null) {
    portInput.value = String(savedPort);
  }

  // Check current connection status
  setStatus(statusEl, 'Checking…', '#888');
  const currentPort = parseInt(portInput.value, 10) || DAEMON_PORT;
  const connected = await checkConnection(currentPort);
  if (connected) {
    setStatus(statusEl, 'Connected', '#0d0');
  } else {
    setStatus(statusEl, 'Not connected', '#e55');
  }

  // Save button
  saveBtn.addEventListener('click', async () => {
    const port = parseInt(portInput.value, 10);
    if (!port || port < 1 || port > 65535) {
      setStatus(statusEl, 'Invalid port', '#e55');
      return;
    }

    await storePort(port);
    setStatus(statusEl, 'Checking…', '#888');
    const ok = await checkConnection(port);
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

/**
 * OpenCLI — Browser Action Popup
 *
 * Lets the user set the daemon port and shows connection status.
 */

import { checkDaemonConnection, detectDaemonPort, getStoredPort, storePort, DAEMON_PORT } from './protocol';

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
  const savedPort = await getStoredPort();
  if (savedPort !== null) {
    portInput.value = String(savedPort);
  }

  // Prefer an actively reachable daemon port over a stale saved/default port.
  setStatus(statusEl, 'Checking…', '#888');
  const detectedPort = await detectDaemonPort(savedPort);
  if (detectedPort !== null) {
    portInput.value = String(detectedPort);
    if (detectedPort !== savedPort) {
      await storePort(detectedPort);
    }
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

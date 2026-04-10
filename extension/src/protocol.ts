/**
 * opencli browser protocol — shared types between daemon, extension, and CLI.
 *
 * 5 actions: exec, navigate, tabs, cookies, screenshot.
 * Everything else is just JS code sent via 'exec'.
 */

export type Action = 'exec' | 'navigate' | 'tabs' | 'cookies' | 'screenshot' | 'close-window' | 'sessions' | 'bg_fetch';

export interface Command {
  /** Unique request ID */
  id: string;
  /** Action type */
  action: Action;
  /** Target tab ID (omit for active tab) */
  tabId?: number;
  /** JS code to evaluate in page context (exec action) */
  code?: string;
  /** Logical workspace for automation session reuse */
  workspace?: string;
  /** URL to navigate to (navigate action) */
  url?: string;
  /** Sub-operation for tabs: list, new, close, select */
  op?: 'list' | 'new' | 'close' | 'select';
  /** Tab index for tabs select/close */
  index?: number;
  /** Cookie domain filter */
  domain?: string;
  /** HTTP method for bg_fetch (default: GET) */
  method?: string;
  /** Extra request headers for bg_fetch */
  request_headers?: Record<string, string>;
  /** Request body for bg_fetch */
  body?: string;
  /** URL to extract cookies from for bg_fetch (defaults to url) */
  cookie_url?: string;
  /** Screenshot format: png (default) or jpeg */
  format?: 'png' | 'jpeg';
  /** JPEG quality (0-100), only for jpeg format */
  quality?: number;
  /** Whether to capture full page (not just viewport) */
  fullPage?: boolean;
}

export interface Result {
  /** Matching request ID */
  id: string;
  /** Whether the command succeeded */
  ok: boolean;
  /** Result data on success */
  data?: unknown;
  /** Error message on failure */
  error?: string;
}

/** Default daemon port */
export const DAEMON_PORT = 19825;
export const DAEMON_HOST = 'localhost';
export const DAEMON_PORT_RANGE_START = 19825;
export const DAEMON_PORT_RANGE_END = 19834;

/** Storage key for the configured daemon port */
export const STORAGE_KEY_PORT = 'opencli_daemon_port';

/** Get the configured daemon port from chrome.storage.local */
export async function getStoredPort(): Promise<number | null> {
  const result = await chrome.storage.local.get(STORAGE_KEY_PORT);
  return (result[STORAGE_KEY_PORT] as number) ?? null;
}

/** Save the daemon port to chrome.storage.local */
export async function storePort(port: number): Promise<void> {
  await chrome.storage.local.set({ [STORAGE_KEY_PORT]: port });
}

/** Build WebSocket URL for a specific port */
export function daemonWsUrl(port: number): string {
  return `ws://${DAEMON_HOST}:${port}/ext`;
}

/** Check whether a daemon WebSocket is reachable on the given port */
export async function checkDaemonConnection(port: number, timeoutMs = 1200): Promise<boolean> {
  return new Promise((resolve) => {
    const ws = new WebSocket(daemonWsUrl(port));
    const timer = setTimeout(() => {
      try { ws.close(); } catch { /* ignore */ }
      resolve(false);
    }, timeoutMs);
    ws.onopen = () => {
      clearTimeout(timer);
      ws.close();
      resolve(true);
    };
    ws.onerror = () => {
      clearTimeout(timer);
      resolve(false);
    };
  });
}

/** Find the first reachable daemon port, preferring the given port when present. */
export async function detectDaemonPort(preferredPort?: number | null): Promise<number | null> {
  const tried = new Set<number>();
  const ports: number[] = [];
  if (preferredPort && preferredPort >= 1 && preferredPort <= 65535) {
    ports.push(preferredPort);
    tried.add(preferredPort);
  }
  for (let port = DAEMON_PORT_RANGE_START; port <= DAEMON_PORT_RANGE_END; port++) {
    if (!tried.has(port)) ports.push(port);
  }

  for (const port of ports) {
    if (await checkDaemonConnection(port)) {
      return port;
    }
  }
  return null;
}

/** Base reconnect delay for extension WebSocket (ms) */
export const WS_RECONNECT_BASE_DELAY = 2000;
/** Max reconnect delay (ms) */
export const WS_RECONNECT_MAX_DELAY = 60000;
/** Idle timeout before daemon auto-exits (ms) */
export const DAEMON_IDLE_TIMEOUT = 5 * 60 * 1000;

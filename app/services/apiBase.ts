/**
 * Where the studio service lives.
 *
 * In the packaged desktop application the UI is served from Tauri's asset
 * protocol, so a relative `/setup/status` never reaches the service — it
 * resolves against the asset host and comes back as the HTML shell, which the
 * caller then fails to parse as JSON. Development is different again: the Vite
 * server proxies those paths.
 *
 * One rule covers both: address the service directly on loopback. The service
 * answers with permissive CORS, so this works from the desktop shell, from the
 * dev server, and from a plain browser tab.
 */

import { STUDIO } from '../studio';

const DEFAULT_BASE = `http://127.0.0.1:${STUDIO.port}`;

/** Paths owned by the studio service; everything else is left untouched. */
const SERVICE_PREFIXES = ['/v1/', '/setup/', '/engine/', '/health'];

function resolveBase(): string {
  const override = (import.meta as { env?: Record<string, string | undefined> }).env?.VITE_STUDIO_API_BASE;
  if (override) return override.replace(/\/$/, '');
  // Already served by the service itself: keep requests same-origin.
  if (typeof location !== 'undefined' && location.port === String(STUDIO.port)) return '';
  return DEFAULT_BASE;
}

export const API_BASE = resolveBase();

export function apiUrl(path: string): string {
  if (!API_BASE || !path.startsWith('/')) return path;
  return `${API_BASE}${path}`;
}

function isServicePath(path: string): boolean {
  return SERVICE_PREFIXES.some((prefix) => path === prefix || path.startsWith(prefix));
}

/**
 * Rewrites service-owned relative requests once, centrally, so every caller can
 * keep using readable paths and no screen can be left behind pointing at the
 * wrong host.
 */
export function installApiBase(): void {
  if (!API_BASE || typeof window === 'undefined') return;
  const original = window.fetch.bind(window);
  window.fetch = (input: RequestInfo | URL, init?: RequestInit) => {
    if (typeof input === 'string' && input.startsWith('/') && isServicePath(input)) {
      return original(apiUrl(input), init);
    }
    if (input instanceof Request && input.url.startsWith(location.origin)) {
      const path = input.url.slice(location.origin.length);
      if (isServicePath(path)) return original(new Request(apiUrl(path), input));
    }
    return original(input, init);
  };
}

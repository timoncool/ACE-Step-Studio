/**
 * Links that leave the studio.
 *
 * The desktop window is a webview, not a browser: an anchor with
 * `target="_blank"` either does nothing or, worse, replaces the application
 * with a web page. Every external link is therefore handed to the system
 * browser through Tauri's opener, and in a plain browser it behaves as it
 * always did.
 *
 * One listener on the document covers every link in the interface, including
 * the ones inside news items, so nothing has to remember to be special.
 */

type Opener = { openUrl?: (url: string) => Promise<void> };
type Invoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>;

function bridge(): { opener?: Opener; core?: { invoke?: Invoke } } | null {
  return (window as unknown as { __TAURI__?: { opener?: Opener; core?: { invoke?: Invoke } } }).__TAURI__ ?? null;
}

/** True inside the desktop shell. */
export function isDesktop(): boolean {
  return Boolean(bridge());
}

/** Opens one URL wherever it belongs. */
export async function openExternal(url: string): Promise<void> {
  const tauri = bridge();
  // Two ways in, because which one exists depends on how the shell is built:
  // the plugin's own binding, or the command behind it.
  const open = tauri?.opener?.openUrl;
  if (open) {
    await open(url).catch(() => fallback(url));
    return;
  }
  const invoke = tauri?.core?.invoke;
  if (invoke) {
    await invoke('plugin:opener|open_url', { url }).catch(() => fallback(url));
    return;
  }
  fallback(url);
}

function fallback(url: string): void {
  window.open(url, '_blank', 'noopener');
}

/** Sends every external link click to the system browser. Call once. */
export function installExternalLinkHandler(): void {
  // Capture, not bubble: dialogs stop propagation on their own container to
  // keep a click inside from closing them, and that also hid every link in
  // Settings and the news panel from a listener on the document.
  document.addEventListener('click', (event) => {
    if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey) return;
    const anchor = (event.target as HTMLElement | null)?.closest?.('a');
    const href = anchor?.getAttribute('href');
    if (!href || !/^https?:\/\//i.test(href)) return;
    event.preventDefault();
    void openExternal(href);
  }, true);
}

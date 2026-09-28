/** Whether the equalizer panel is open, for the player's button, the panel and the agent. */

let equalizerOpen = false;
const listeners = new Set<(open: boolean) => void>();

export function equalizerPanelOpen(): boolean {
  return equalizerOpen;
}

export function setEqualizerPanelOpen(open: boolean): void {
  if (equalizerOpen === open) return;
  equalizerOpen = open;
  listeners.forEach((listener) => listener(open));
}

export function onEqualizerPanel(listener: (open: boolean) => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Which of the extras the sidebar shows; the player bar always has all three. */
export interface SidebarExtras {
  equalizer: boolean;
  visualizer: boolean;
  winamp: boolean;
}

const EXTRAS_KEY = 'studio.sidebarExtras';
const ALL_EXTRAS: SidebarExtras = { equalizer: true, visualizer: true, winamp: true };
const extrasListeners = new Set<(extras: SidebarExtras) => void>();

export function sidebarExtras(): SidebarExtras {
  try {
    return { ...ALL_EXTRAS, ...JSON.parse(window.localStorage.getItem(EXTRAS_KEY) ?? '{}') };
  } catch {
    return ALL_EXTRAS;
  }
}

export function setSidebarExtras(change: Partial<SidebarExtras>): void {
  const next = { ...sidebarExtras(), ...change };
  try {
    window.localStorage.setItem(EXTRAS_KEY, JSON.stringify(next));
  } catch {
    // a browser that keeps nothing still shows the change until a reload
  }
  extrasListeners.forEach((listener) => listener(next));
}

export function onSidebarExtras(listener: (extras: SidebarExtras) => void): () => void {
  extrasListeners.add(listener);
  return () => extrasListeners.delete(listener);
}

const STOCK_COVERS_KEY = 'studio.stockCovers';
const stockCoverListeners = new Set<(on: boolean) => void>();

/** Whether a track without a cover of its own shows a stock photo (on by
 *  default) or the pattern drawn from its id. */
export function stockCovers(): boolean {
  try {
    return window.localStorage.getItem(STOCK_COVERS_KEY) !== 'off';
  } catch {
    return true;
  }
}

export function setStockCovers(on: boolean): void {
  try {
    window.localStorage.setItem(STOCK_COVERS_KEY, on ? 'on' : 'off');
  } catch {
    // a browser that keeps nothing still shows the change until a reload
  }
  stockCoverListeners.forEach((listener) => listener(on));
}

export function onStockCovers(listener: (on: boolean) => void): () => void {
  stockCoverListeners.add(listener);
  return () => stockCoverListeners.delete(listener);
}

/** The photo a track without a cover shows while stock covers are on:
 *  picsum.photos, seeded by the track so it stays the same. */
export function stockCoverUrl(seed: string): string {
  return `https://picsum.photos/seed/${encodeURIComponent(seed)}/400/400`;
}

/** The picture a track shows: its own cover, else the stock photo while
 *  stock covers are on, else none. */
export function trackCoverUrl(song: { id: string; coverUrl?: string }): string {
  return song.coverUrl || (stockCovers() ? stockCoverUrl(song.id) : '');
}

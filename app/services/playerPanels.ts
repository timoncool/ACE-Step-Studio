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

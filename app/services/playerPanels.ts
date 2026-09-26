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

/** When each song was last played in this window, read by the "recently played" order. */

const KEY = 'studio.playedAt';
const listeners = new Set<() => void>();

function read(): Record<string, number> {
  try {
    const kept = JSON.parse(window.localStorage.getItem(KEY) ?? '{}');
    return kept && typeof kept === 'object' ? kept as Record<string, number> : {};
  } catch {
    return {};
  }
}

let played = read();

export function playedAt(songId: string): number {
  return played[songId] ?? 0;
}

export function notePlayed(songId: string): void {
  played = { ...played, [songId]: Date.now() };
  try {
    window.localStorage.setItem(KEY, JSON.stringify(played));
  } catch (error) {
    console.error('The play history could not be kept:', error);
  }
  listeners.forEach((listener) => listener());
}

export function onPlayHistory(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

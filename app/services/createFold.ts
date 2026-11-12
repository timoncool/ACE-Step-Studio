/** The blocks of the create form the person folded, kept across restarts apart from the settings Reset puts back. */
export const FOLDED_KEY = 'studio.createFolded';
/** The blocks of the song panel the person folded. */
export const SONG_PANEL_FOLDED_KEY = 'studio.songPanelFolded';

export function keptFolded(key: string = FOLDED_KEY): string[] {
  try {
    const kept: unknown = JSON.parse(window.localStorage.getItem(key) ?? '[]');
    return Array.isArray(kept) ? kept.filter((id): id is string => typeof id === 'string') : [];
  } catch {
    return [];
  }
}

/** The folded blocks with `id` folded or opened. */
export function foldedAfter(folded: string[], id: string, fold: boolean): string[] {
  const rest = folded.filter(other => other !== id);
  return fold ? [...rest, id] : rest;
}

export function keepFolded(folded: string[], key: string = FOLDED_KEY): void {
  try {
    window.localStorage.setItem(key, JSON.stringify(folded));
  } catch {
    /* kept until a reload */
  }
}

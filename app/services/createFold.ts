/** The blocks of the create form the person folded, kept across restarts apart from the settings Reset puts back. */
export const FOLDED_KEY = 'studio.createFolded';

export function keptFolded(): string[] {
  try {
    const kept: unknown = JSON.parse(window.localStorage.getItem(FOLDED_KEY) ?? '[]');
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

export function keepFolded(folded: string[]): void {
  try {
    window.localStorage.setItem(FOLDED_KEY, JSON.stringify(folded));
  } catch {
    /* kept until a reload */
  }
}

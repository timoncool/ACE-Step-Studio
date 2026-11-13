import type { Song } from '../types';

/** The words of a search, lowercased; an empty search has none and finds everything. */
export function searchWords(query: string): string[] {
  return query.toLowerCase().split(/\s+/).filter(Boolean);
}

/** Every string and number in a value, however deep: a seed, a model's file name, a setting. Values only: the names
 * of the settings would make a word like "seed" find every song. */
function textOf(value: unknown): string[] {
  if (value == null) return [];
  if (typeof value === 'string') return [value];
  if (typeof value === 'number') return [String(value)];
  if (Array.isArray(value)) return value.flatMap(textOf);
  if (typeof value === 'object') return Object.values(value as Record<string, unknown>).flatMap(textOf);
  return [];
}

/** A song's text, gathered once per song object: a keystroke in the search reads it, it does not rebuild it. */
const gathered = new WeakMap<Song, string>();

/** What a search reads in a song: title, style, tags, lyrics, the person's note, the model and the settings it was made with. */
function searchableText(song: Song): string {
  const known = gathered.get(song);
  if (known !== undefined) return known;
  const text = [song.title, song.style, ...(song.tags ?? []), song.lyrics, song.note, song.ditModel, ...textOf(song.generationParams)]
    .filter((part): part is string => typeof part === 'string' && part.length > 0)
    .join('\n')
    .toLowerCase();
  gathered.set(song, text);
  return text;
}

/** Whether every word of the search is somewhere in the song, in any order. */
export function songMatches(song: Song, words: string[]): boolean {
  if (words.length === 0) return true;
  const text = searchableText(song);
  return words.every(word => text.includes(word));
}

/** Whether every word of the search is in a name, in any order. */
export function nameMatches(name: string, words: string[]): boolean {
  const text = name.toLowerCase();
  return words.every(word => text.includes(word));
}

import type { Song } from '../types';

/** The words of a search, lowercased; an empty search has none and finds everything. */
export function searchWords(query: string): string[] {
  return query.toLowerCase().split(/\s+/).filter(Boolean);
}

/** Every string and number in a value, however deep: a seed, a model's file name, a setting. */
function textOf(value: unknown): string[] {
  if (value == null) return [];
  if (typeof value === 'string') return [value];
  if (typeof value === 'number' || typeof value === 'boolean') return [String(value)];
  if (Array.isArray(value)) return value.flatMap(textOf);
  if (typeof value === 'object') return Object.entries(value as Record<string, unknown>).flatMap(([key, inner]) => [key, ...textOf(inner)]);
  return [];
}

/** What a search reads in a song: title, style, tags, lyrics, the person's note, the model and the settings it was made with. */
function searchableText(song: Song): string {
  return [song.title, song.style, ...(song.tags ?? []), song.lyrics, song.note, song.ditModel, ...textOf(song.generationParams)]
    .filter((part): part is string => typeof part === 'string' && part.length > 0)
    .join('\n')
    .toLowerCase();
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

import type { Song } from '../types';

/** The stems a separation makes, in the order a mixer lists them. */
const STEM_ORDER = ['vocals', 'drums', 'bass', 'guitar', 'piano', 'other'];

/** Which stem a track is, when a separation made it; null for anything else. */
export function stemOf(song: Song): string | null {
  if (song.derived?.tool !== 'stems') return null;
  const stem = song.derived.settings?.stem;
  return typeof stem === 'string' && stem ? stem : 'other';
}

const stemRank = (song: Song) => {
  const rank = STEM_ORDER.indexOf(stemOf(song) ?? 'other');
  return rank < 0 ? STEM_ORDER.length : rank;
};

/**
 * A list's songs with the stems folded under the song they came from. A stem
 * is shown under its song when that song is in the list, on its own otherwise;
 * the stems of a song come from the whole library, so they go wherever the
 * song is shown. Covers, re-renders and processed songs are songs of their own
 * and are not folded.
 */
export function foldStems(list: Song[], library: Song[]): { songs: Song[]; stemsOf: Map<string, Song[]> } {
  const shown = new Set(list.map(song => song.id));
  const stemsOf = new Map<string, Song[]>();
  const seen = new Set<string>();
  for (const song of [...library, ...list]) {
    const from = song.derived?.from;
    if (!from || !shown.has(from) || stemOf(song) === null || seen.has(song.id)) continue;
    seen.add(song.id);
    stemsOf.set(from, [...(stemsOf.get(from) ?? []), song]);
  }
  for (const stems of stemsOf.values()) stems.sort((a, b) => stemRank(a) - stemRank(b));
  const songs = list.filter(song => !(stemOf(song) !== null && song.derived && shown.has(song.derived.from)));
  return { songs, stemsOf };
}

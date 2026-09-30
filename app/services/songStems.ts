import type { Song } from '../types';

/** The stems a separation makes, in the order a mixer lists them. */
const STEM_ORDER = ['vocals', 'drums', 'bass', 'guitar', 'piano', 'other'];

/** Which stem a track is, when a separation made it; null for anything else. */
export function stemOf(song: Pick<Song, 'derived'>): string | null {
  if (song.derived?.tool !== 'stems') return null;
  const stem = song.derived.settings?.stem;
  return typeof stem === 'string' && stem ? stem : 'other';
}

/** What a track's placeholder cover - the stock photo or the pattern - is drawn from: a stem wears its song's. */
export function coverSeed(song: Pick<Song, 'id' | 'title' | 'derived'>): string {
  return (stemOf(song) !== null && song.derived?.from) || song.id || song.title;
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
  const byId = new Map([...library, ...list].map(song => [song.id, song]));
  // the song a stem folds under: its own, when that is in the list and is a
  // song - a stem separated from a stem, which older versions allowed, has no
  // row to fold under and stays in sight on its own
  const foldsUnder = (song: Song): string | null => {
    const from = song.derived?.from;
    if (!from || stemOf(song) === null || !shown.has(from)) return null;
    const parent = byId.get(from);
    return parent && stemOf(parent) === null ? from : null;
  };
  const stemsOf = new Map<string, Song[]>();
  const seen = new Set<string>();
  for (const song of [...library, ...list]) {
    const from = foldsUnder(song);
    if (from === null || seen.has(song.id)) continue;
    seen.add(song.id);
    stemsOf.set(from, [...(stemsOf.get(from) ?? []), song]);
  }
  for (const stems of stemsOf.values()) stems.sort((a, b) => stemRank(a) - stemRank(b));
  const songs = list.filter(song => foldsUnder(song) === null);
  return { songs, stemsOf };
}

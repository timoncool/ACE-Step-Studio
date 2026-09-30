import { describe, expect, it } from 'vitest';
import type { Song } from '../types';
import { foldStems, stemOf } from './songStems';

const song = (id: string, derived?: Song['derived']): Song => ({
  id, title: id, lyrics: '', style: '', coverUrl: '', duration: '0:00', createdAt: new Date(0), tags: [], derived,
});
const stem = (id: string, from: string, name: string) => song(id, { from, fromTitle: from, tool: 'stems', settings: { stem: name } });

describe('stems folded under their song', () => {
  const night = song('night');
  const cover = song('cover', { from: 'night', fromTitle: 'night', tool: 'cover' });
  const [drums, vocals, other] = [stem('d', 'night', 'drums'), stem('v', 'night', 'vocals'), stem('o', 'night', 'other')];
  const library = [night, cover, drums, vocals, other];

  it('keeps songs and covers, folds only the stems, in the order of a mixer', () => {
    const folded = foldStems(library, library);
    expect(folded.songs.map(entry => entry.id)).toEqual(['night', 'cover']);
    expect(folded.stemsOf.get('night')?.map(entry => entry.id)).toEqual(['v', 'd', 'o']);
  });

  it('brings a song its stems from the library when the list holds the song alone', () => {
    const folded = foldStems([night], library);
    expect(folded.songs.map(entry => entry.id)).toEqual(['night']);
    expect(folded.stemsOf.get('night')).toHaveLength(3);
  });

  it('keeps a stem separated from a stem in sight, with no stem row to fold under', () => {
    const again = stem('va', 'v', 'vocals');
    const folded = foldStems([...library, again], [...library, again]);
    expect(folded.songs.map(entry => entry.id)).toEqual(['night', 'cover', 'va']);
    expect(folded.stemsOf.has('v')).toBe(false);
  });

  it('shows a stem on its own when its song is not in the list', () => {
    const folded = foldStems([vocals], library);
    expect(folded.songs.map(entry => entry.id)).toEqual(['v']);
    expect(folded.stemsOf.size).toBe(0);
  });

  it('names a stem by what the separation recorded', () => {
    expect(stemOf(vocals)).toBe('vocals');
    expect(stemOf(song('x', { from: 'night', fromTitle: 'night', tool: 'stems' }))).toBe('other');
    expect(stemOf(cover)).toBeNull();
  });
});

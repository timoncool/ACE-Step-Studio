import { describe, expect, it } from 'vitest';
import type { Song } from '../types';
import { nameMatches, searchWords, songMatches } from './songSearch';

const song = (fields: Partial<Song>): Song =>
  ({ id: 's', title: 'Neon mirage', style: 'synthwave, gated reverb', tags: ['night'], lyrics: '', ...fields }) as Song;

describe('library search', () => {
  it('finds a song by its words in any order', () => {
    expect(songMatches(song({}), searchWords('mirage neon'))).toBe(true);
    expect(songMatches(song({}), searchWords('neon  SYNTHWAVE'))).toBe(true);
    expect(songMatches(song({}), searchWords('neon dawn'))).toBe(false);
  });

  it('reads the lyrics, the note and the tags', () => {
    expect(songMatches(song({ lyrics: '[verse]\nFeel the wind tonight' }), searchWords('wind night'))).toBe(true);
    expect(songMatches(song({ note: 'for the second album' }), searchWords('album'))).toBe(true);
  });

  it('reads the settings the song was made with: the seed and the model', () => {
    const made = song({ ditModel: 'acestep-v15-turbo-Q8_0.gguf', generationParams: { seed: 1234567, synth_model: 'acestep-v15-sft-Q4_K_M.gguf', lm_seed: 42 } });
    expect(songMatches(made, searchWords('1234567'))).toBe(true);
    expect(songMatches(made, searchWords('turbo q8'))).toBe(true);
    expect(songMatches(made, searchWords('sft'))).toBe(true);
    expect(songMatches(made, searchWords('42 1234567'))).toBe(true);
  });

  it('does not read the names of the settings, only their values', () => {
    const made = song({ generationParams: { seed: 7, steps: 8, lora: null, duration: 120, synth_model: 'acestep-v15-turbo.gguf' } });
    for (const name of ['seed', 'steps', 'lora', 'duration', 'model']) {
      expect(songMatches(made, searchWords(name))).toBe(false);
    }
    expect(songMatches(made, searchWords('turbo'))).toBe(true);
  });

  it('finds everything with an empty search and a song with nothing but a title', () => {
    expect(searchWords('   ')).toEqual([]);
    expect(songMatches(song({}), [])).toBe(true);
    expect(songMatches({ id: 'x', title: 'Bare', style: '', tags: [], lyrics: '' } as unknown as Song, searchWords('bare'))).toBe(true);
  });

  it('matches an uploaded file by the words of its name', () => {
    expect(nameMatches('Demo take 3', searchWords('take demo'))).toBe(true);
    expect(nameMatches('Demo take 3', searchWords('demo final'))).toBe(false);
  });
});

import { describe, expect, it } from 'vitest';
import { toolsSongOnOpen } from './toolsSong';

describe('toolsSongOnOpen', () => {
  const songs = ['a', 'b', 'c'];

  it('opens on the song a stems run is working on', () => {
    expect(toolsSongOnOpen(songs, [{ song_id: 'b', done: false }, null])).toBe('b');
  });

  it('opens on the song a MIDI run is working on', () => {
    expect(toolsSongOnOpen(songs, [null, { song_id: 'c', done: false }])).toBe('c');
  });

  it('takes the first song when every run has ended', () => {
    expect(toolsSongOnOpen(songs, [{ song_id: 'b', done: true }, { song_id: 'c', done: true }])).toBe('a');
  });

  it('passes over a run of a song no longer in the library', () => {
    expect(toolsSongOnOpen(songs, [{ song_id: 'gone', done: false }, { song_id: null, done: false }])).toBe('a');
  });

  it('is empty without songs', () => {
    expect(toolsSongOnOpen([], [{ song_id: 'b', done: false }])).toBe('');
  });
});

import { describe, expect, it } from 'vitest';
import { shownSongs, toolsSongOnOpen } from './toolsSong';

describe('shownSongs', () => {
  const list = ['a', 'b', 'c', 'd'].map(id => ({ id }));
  const ids = (songs: { id: string }[]) => songs.map(song => song.id);

  it('draws the first matches when the chosen song is among them', () => {
    expect(ids(shownSongs(list, 'b', 3))).toEqual(['a', 'b', 'c']);
  });

  it('puts a chosen song past the limit first', () => {
    expect(ids(shownSongs(list, 'd', 3))).toEqual(['d', 'a', 'b']);
  });

  it('leaves out a chosen song the search does not match', () => {
    expect(ids(shownSongs(list.slice(0, 2), 'd', 3))).toEqual(['a', 'b']);
  });
});

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

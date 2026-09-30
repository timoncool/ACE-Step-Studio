import { describe, expect, it } from 'vitest';
import type { Song } from '../types';
import { orderSongs } from './songOrder';

const song = (id: string, title: string, made: number, duration: string, likedAt?: number): Song => ({
  id, title, lyrics: '', style: '', coverUrl: '', duration, createdAt: new Date(made), tags: [],
  likedAt: likedAt === undefined ? undefined : new Date(likedAt),
});

describe('the order a list is read in', () => {
  const songs = [song('a', 'Ёлка', 2, '3:05', 10), song('b', 'арфа', 3, '0:59'), song('c', 'Бас 10', 1, '12:00', 30), song('d', 'Бас 9', 4, '')];
  const ids = (order: Parameters<typeof orderSongs>[1]) => orderSongs(songs, order, 'ru').map(entry => entry.id);

  it('reads by when a song was made, newest first by default', () => {
    expect(ids('newest')).toEqual(['d', 'b', 'a', 'c']);
    expect(ids('oldest')).toEqual(['c', 'a', 'b', 'd']);
  });

  it('reads titles as the language sorts them, numbers as numbers', () => {
    expect(ids('titleAsc')).toEqual(['b', 'd', 'c', 'a']);
    expect(ids('titleDesc')).toEqual(['a', 'c', 'd', 'b']);
  });

  it('reads by length, a song without one as the shortest', () => {
    expect(ids('longest')).toEqual(['c', 'a', 'b', 'd']);
    expect(ids('shortest')).toEqual(['d', 'b', 'a', 'c']);
  });

  it('reads the liked songs from the latest like', () => {
    expect(ids('likedNewest').slice(0, 2)).toEqual(['c', 'a']);
  });
});

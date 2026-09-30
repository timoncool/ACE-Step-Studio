import { useState } from 'react';
import type { Song } from '../types';
import { parseDuration } from './nativeLibrary';

/** How a list of songs is read. */
export type SongOrder = 'newest' | 'oldest' | 'titleAsc' | 'titleDesc' | 'longest' | 'shortest' | 'likedNewest';

export const SONG_ORDERS: SongOrder[] = ['newest', 'oldest', 'titleAsc', 'titleDesc', 'longest', 'shortest'];

/** "3:07" as seconds; a song without a length sorts as the shortest. */
function seconds(duration: string | undefined): number {
  return parseDuration(duration ?? '') ?? 0;
}

/** Anything a list shows as a row: a song, or a file brought in beside the songs. */
export interface Orderable {
  createdAt: Date;
  title: string;
  duration?: string;
  likedAt?: Date;
}

/** A comparator for the order, titles compared as the window's language sorts them. */
export function compareBy(order: SongOrder, language: string): (a: Orderable, b: Orderable) => number {
  const titles = new Intl.Collator(language, { numeric: true, sensitivity: 'base' });
  const made = (a: Orderable, b: Orderable) => a.createdAt.getTime() - b.createdAt.getTime();
  switch (order) {
    case 'oldest': return made;
    case 'titleAsc': return (a, b) => titles.compare(a.title, b.title) || made(b, a);
    case 'titleDesc': return (a, b) => titles.compare(b.title, a.title) || made(b, a);
    case 'longest': return (a, b) => seconds(b.duration) - seconds(a.duration) || made(b, a);
    case 'shortest': return (a, b) => seconds(a.duration) - seconds(b.duration) || made(b, a);
    case 'likedNewest': return (a, b) => (b.likedAt?.getTime() ?? 0) - (a.likedAt?.getTime() ?? 0) || made(b, a);
    default: return (a, b) => made(b, a);
  }
}

export function orderSongs(songs: Song[], order: SongOrder, language: string): Song[] {
  return [...songs].sort(compareBy(order, language));
}

/** The order a list was last read in, kept for that list in the window's storage. */
export function useListOrder(list: string, fallback: SongOrder, allowed: SongOrder[]): [SongOrder, (order: SongOrder) => void] {
  const key = `studio.order.${list}`;
  const [order, setOrder] = useState<SongOrder>(() => {
    try {
      const kept = window.localStorage.getItem(key) as SongOrder | null;
      return kept && allowed.includes(kept) ? kept : fallback;
    } catch {
      return fallback;
    }
  });
  const choose = (next: SongOrder) => {
    setOrder(next);
    try { window.localStorage.setItem(key, next); } catch { /* kept until a reload */ }
  };
  return [order, choose];
}

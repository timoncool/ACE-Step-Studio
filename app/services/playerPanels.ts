import type { Song } from '../types';
import { apiUrl } from './apiBase';
import { DEFAULT_COVER_PATTERN, isCoverPattern, patternArt } from './coverArt';
import { coverSeed } from './songStems';
import { coverLookNow } from './studioQueries';

/** Whether the equalizer panel is open, for the player's button, the panel and the agent. */

let equalizerOpen = false;
const listeners = new Set<(open: boolean) => void>();

export function equalizerPanelOpen(): boolean {
  return equalizerOpen;
}

export function setEqualizerPanelOpen(open: boolean): void {
  if (equalizerOpen === open) return;
  equalizerOpen = open;
  listeners.forEach((listener) => listener(open));
}

export function onEqualizerPanel(listener: (open: boolean) => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Which of the extras the sidebar shows; the player bar always has all three. */
export interface SidebarExtras {
  equalizer: boolean;
  visualizer: boolean;
  winamp: boolean;
}

const EXTRAS_KEY = 'studio.sidebarExtras';
const ALL_EXTRAS: SidebarExtras = { equalizer: true, visualizer: true, winamp: true };
const extrasListeners = new Set<(extras: SidebarExtras) => void>();

export function sidebarExtras(): SidebarExtras {
  try {
    return { ...ALL_EXTRAS, ...JSON.parse(window.localStorage.getItem(EXTRAS_KEY) ?? '{}') };
  } catch {
    return ALL_EXTRAS;
  }
}

export function setSidebarExtras(change: Partial<SidebarExtras>): void {
  const next = { ...sidebarExtras(), ...change };
  try {
    window.localStorage.setItem(EXTRAS_KEY, JSON.stringify(next));
  } catch {
    // a browser that keeps nothing still shows the change until a reload
  }
  extrasListeners.forEach((listener) => listener(next));
}

export function onSidebarExtras(listener: (extras: SidebarExtras) => void): () => void {
  extrasListeners.add(listener);
  return () => extrasListeners.delete(listener);
}

/** The picture a track shows outside the lists - the player's skins, a video:
 *  its own cover, else the look's photograph while it is not written into the
 *  track, else its pattern. */
export function trackCoverUrl(song: Pick<Song, 'id' | 'title' | 'derived' | 'coverUrl'>): string {
  if (song.coverUrl) return song.coverUrl;
  const look = coverLookNow();
  if (look?.photo && !look.keep) return apiUrl(`/v1/library/songs/${encodeURIComponent(song.id)}/cover/placeholder?look=${encodeURIComponent(look.pattern)}`);
  return patternArt(coverSeed(song), isCoverPattern(look?.pattern) ? look.pattern : DEFAULT_COVER_PATTERN);
}

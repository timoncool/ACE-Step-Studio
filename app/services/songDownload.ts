import type { Song } from '../types';
import { saveFile } from './saveFile';

const EXTENSIONS: [string, string][] = [
  ['wav', 'wav'],
  ['flac', 'flac'],
  ['ogg', 'ogg'],
  ['mp4', 'm4a'],
  ['aac', 'm4a'],
  ['mpeg', 'mp3'],
];

/** The label of the version that plays, when it is not the original. */
export function activeVersionLabel(song: Song): string | null {
  const active = song.activeVersion;
  if (!active || active === 'original') return null;
  return song.audioVersions?.find(version => version.id === active)?.label || null;
}

/** A file name for the track as it plays now: its title, the processing applied, the stored format. */
export function songFileName(song: Song, contentType: string): string {
  const version = activeVersionLabel(song);
  const base = version ? `${song.title || 'song'} (${version})` : song.title || 'song';
  const safe = base.replace(/[<>:"/\\|?*\u0000-\u001f]/g, '_').replace(/[. ]+$/, '').slice(0, 180);
  const extension = EXTENSIONS.find(([marker]) => contentType.includes(marker))?.[1] ?? 'mp3';
  return `${safe || 'song'}.${extension}`;
}

/** Saves the track's audio as it plays now, where the user says. */
export async function downloadSongAudio(song: Song): Promise<void> {
  if (!song.audioUrl) return;
  // the name carries the stored format, which only the file's own type tells
  const head = await fetch(song.audioUrl, { method: 'HEAD' });
  if (!head.ok) throw new Error(`HTTP ${head.status}`);
  await saveFile(songFileName(song, head.headers.get('content-type') ?? ''), { url: song.audioUrl });
}

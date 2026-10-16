import React, { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { X, Play, Pencil, Trash2, ListStart, ListEnd, Shuffle } from 'lucide-react';
import type { Playlist, Song } from '../types';
import { useI18n } from '../context/I18nContext';
import { AlbumCover, SongCover } from './AlbumCover';
import { updateNativePlaylist } from '../services/nativeLibrary';
import { updateLibraryPlaylists } from '../services/studioQueries';

/** A playlist's songs in its order, those still in the library. */
export function playlistSongs(playlist: Playlist, library: Song[]): Song[] {
  const byId = new Map(library.map((song) => [song.id, song]));
  return (playlist.songIds ?? []).flatMap((id) => (byId.has(id) ? [byId.get(id)!] : []));
}

/** "1 h 12 min", "4 min 05 s", "58 s": a playlist's length, never rounded down to nothing. */
export function playlistLength(seconds: number, t: (key: 'hoursShort' | 'minutesShort' | 'secondsShort') => string): string {
  const total = Math.round(seconds);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const rest = total % 60;
  if (hours) return `${hours} ${t('hoursShort')} ${minutes} ${t('minutesShort')}`;
  if (minutes) return `${minutes} ${t('minutesShort')} ${String(rest).padStart(2, '0')} ${t('secondsShort')}`;
  return `${rest} ${t('secondsShort')}`;
}

/** The playlist's picture: the covers of its first four songs, as Spotify draws one; one song, its cover. */
export const PlaylistCover: React.FC<{ playlist: Playlist; songs: Song[]; className?: string }> = ({ playlist, songs, className = '' }) => {
  const seen = new Set<string>();
  const covers = songs.filter((song) => {
    const key = song.coverUrl || song.id;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
  if (covers.length >= 4) {
    return (
      <div className={`grid grid-cols-2 grid-rows-2 overflow-hidden ${className}`}>
        {covers.slice(0, 4).map((song) => <SongCover key={song.id} song={song} size="full" className="h-full w-full" />)}
      </div>
    );
  }
  if (covers.length) return <SongCover song={covers[0]} size="full" className={className} />;
  return <AlbumCover seed={playlist.id || playlist.name} size="full" className={className} />;
};

/** Name and description of a playlist, changed in place. */
export const PlaylistEditModal: React.FC<{ playlist: Playlist | null; onClose: () => void; onError: (message: string) => void }> = ({ playlist, onClose, onError }) => {
  const { t } = useI18n();
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!playlist) return;
    setName(playlist.name);
    setDescription(playlist.description ?? '');
  }, [playlist]);

  useEffect(() => {
    if (!playlist) return undefined;
    const key = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', key);
    return () => window.removeEventListener('keydown', key);
  }, [playlist, onClose]);

  if (!playlist) return null;

  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!name.trim()) return;
    setSaving(true);
    try {
      const updated = await updateNativePlaylist(playlist.id, { ...playlist, name: name.trim(), description: description.trim() || undefined });
      updateLibraryPlaylists((lists) => lists.map((entry) => (entry.id === updated.id ? updated : entry)));
      onClose();
    } catch (error) {
      onError(error instanceof Error ? error.message : String(error));
    } finally {
      setSaving(false);
    }
  };

  return createPortal(
    <div className="fixed inset-0 z-60 flex items-center justify-center bg-black/50 p-4 backdrop-blur-xs dark:bg-black/80" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <div role="dialog" aria-modal="true" aria-labelledby="playlist-edit-title" className="w-full max-w-md rounded-xl border border-zinc-200 bg-white p-6 shadow-2xl animate-in fade-in zoom-in-95 duration-200 dark:border-white/10 dark:bg-zinc-900">
        <div className="mb-6 flex items-center justify-between">
          <h2 id="playlist-edit-title" className="text-xl font-bold text-zinc-900 dark:text-white">{t('editPlaylistDetails')}</h2>
          <button type="button" onClick={onClose} aria-label={t('close')} className="text-zinc-400 hover:text-zinc-900 dark:hover:text-white">
            <X size={20} />
          </button>
        </div>
        <form onSubmit={(event) => void save(event)} className="space-y-4">
          <div>
            <label htmlFor="playlist-edit-name" className="mb-1 block text-xs font-semibold uppercase text-zinc-500 dark:text-zinc-400">{t('playlistName')}</label>
            <input
              id="playlist-edit-name"
              type="text"
              value={name}
              onChange={(event) => setName(event.target.value)}
              autoFocus
              className="w-full rounded-lg border border-zinc-200 bg-zinc-50 p-3 text-zinc-900 focus:border-pink-500 focus:outline-hidden focus:ring-1 focus:ring-pink-500 dark:border-white/10 dark:bg-black/50 dark:text-white"
            />
          </div>
          <div>
            <label htmlFor="playlist-edit-description" className="mb-1 block text-xs font-semibold uppercase text-zinc-500 dark:text-zinc-400">{t('playlistDescription')}</label>
            <textarea
              id="playlist-edit-description"
              value={description}
              onChange={(event) => setDescription(event.target.value)}
              placeholder={t('descriptionPlaceholder')}
              className="h-24 w-full resize-none rounded-lg border border-zinc-200 bg-zinc-50 p-3 text-zinc-900 placeholder-zinc-400 focus:border-pink-500 focus:outline-hidden focus:ring-1 focus:ring-pink-500 dark:border-white/10 dark:bg-black/50 dark:text-white dark:placeholder-zinc-600"
            />
          </div>
          <div className="flex justify-end gap-3 pt-2">
            <button type="button" onClick={onClose} className="px-4 py-2 text-sm font-medium text-zinc-500 hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-white">{t('cancel')}</button>
            <button type="submit" disabled={!name.trim() || saving} className="rounded-full bg-zinc-900 px-6 py-2 text-sm font-bold text-white transition-transform hover:scale-105 disabled:opacity-50 dark:bg-white dark:text-black">{t('save')}</button>
          </div>
        </form>
      </div>
    </div>,
    document.body,
  );
};

export interface PlaylistMenuActions {
  play: () => void;
  shuffle: () => void;
  queue: (next: boolean) => void;
  edit: () => void;
  remove: () => void;
}

/** What a playlist offers from its "..." button and from a right click. */
export const PlaylistMenu: React.FC<{ at: { x: number; y: number } | null; empty: boolean; actions: PlaylistMenuActions; onClose: () => void }> = ({ at, empty, actions, onClose }) => {
  const { t } = useI18n();
  const menu = useRef<HTMLDivElement>(null);
  const [spot, setSpot] = useState<{ left: number; top: number } | null>(null);

  useLayoutEffect(() => {
    const node = menu.current;
    if (!at || !node) {
      setSpot(null);
      return;
    }
    const box = node.getBoundingClientRect();
    setSpot({ left: Math.max(8, Math.min(at.x, window.innerWidth - box.width - 8)), top: Math.max(8, Math.min(at.y, window.innerHeight - box.height - 8)) });
  }, [at]);

  useEffect(() => {
    if (!at) return undefined;
    const outside = (event: MouseEvent) => {
      if (menu.current && !menu.current.contains(event.target as Node)) onClose();
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    document.addEventListener('mousedown', outside);
    document.addEventListener('keydown', key);
    return () => {
      document.removeEventListener('mousedown', outside);
      document.removeEventListener('keydown', key);
    };
  }, [at, onClose]);

  if (!at) return null;
  const item = (icon: React.ReactNode, label: string, run: () => void, options: { disabled?: boolean; danger?: boolean } = {}) => (
    <button
      type="button"
      role="menuitem"
      disabled={options.disabled}
      onClick={(event) => {
        event.stopPropagation();
        onClose();
        run();
      }}
      className={`flex w-full items-center gap-3 px-3 py-2 text-left text-sm transition-colors disabled:cursor-not-allowed disabled:opacity-50 ${options.danger ? 'text-red-400 hover:bg-red-500/10 hover:text-red-300' : 'text-zinc-300 hover:bg-white/5 hover:text-white'}`}
    >
      <span className="flex h-4 w-4 items-center justify-center opacity-70">{icon}</span>
      {label}
    </button>
  );
  return createPortal(
    <div
      ref={menu}
      role="menu"
      onClick={(event) => event.stopPropagation()}
      onContextMenu={(event) => event.preventDefault()}
      style={{ left: spot?.left ?? at.x, top: spot?.top ?? at.y, visibility: spot ? 'visible' : 'hidden' }}
      className="fixed z-200 w-56 rounded-xl border border-white/10 bg-zinc-900 py-1.5 shadow-2xl transition-none animate-in fade-in"
    >
      {item(<Play size={14} />, t('play'), actions.play, { disabled: empty })}
      {item(<Shuffle size={14} />, t('shufflePlay'), actions.shuffle, { disabled: empty })}
      {item(<ListStart size={14} />, t('playNextMenu'), () => actions.queue(true), { disabled: empty })}
      {item(<ListEnd size={14} />, t('addToQueueMenu'), () => actions.queue(false), { disabled: empty })}
      <div className="mx-2 my-1 h-px bg-white/10" />
      {item(<Pencil size={14} />, t('editPlaylistDetails'), actions.edit)}
      {item(<Trash2 size={14} />, t('deletePlaylist'), actions.remove, { danger: true })}
    </div>,
    document.body,
  );
};

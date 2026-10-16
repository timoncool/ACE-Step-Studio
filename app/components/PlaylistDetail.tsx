import React, { useMemo, useState } from 'react';
import { Song } from '../services/api';
import { useI18n } from '../context/I18nContext';
import { ArrowLeft, Play, Pause, MoreHorizontal, Clock, Shuffle, Trash2, Pencil, GripVertical, Plus, Search, Check, ListMusic } from 'lucide-react';
import { deleteNativePlaylist, parseDuration, updateNativePlaylist } from '../services/nativeLibrary';
import { updateLibraryPlaylists, useLibraryPlaylists, useLibrarySongs } from '../services/studioQueries';
import { named } from '../services/accessibleName';
import { captionSummary } from '../services/examples';
import { useSongActions } from '../context/SongActionsContext';
import { SongCover } from './AlbumCover';
import { ConfirmDialog } from './ConfirmDialog';
import { SongDropdownMenu } from './SongDropdownMenu';
import { PlaylistCover, PlaylistEditModal, PlaylistMenu, playlistLength, playlistSongs } from './PlaylistEditor';
import type { Playlist } from '../types';
import type { ToastType } from './Toast';

interface PlaylistDetailProps {
    playlistId: string;
    onBack: () => void;
    onPlaySong: (song: Song, list?: Song[]) => void;
    onSelect: (song: Song) => void;
    /** What the player holds, so its row says so. */
    currentSong: Song | null;
    isPlaying: boolean;
    /** A message for a moment, with one step it offers, such as undo. */
    notify: (message: string, type?: ToastType, action?: { label: string; onClick: () => void }) => void;
    /** Says why a change to the playlist did not go through. */
    onError: (message: string) => void;
}

function shuffled<T>(items: T[]): T[] {
    const copy = [...items];
    for (let index = copy.length - 1; index > 0; index -= 1) {
        const other = Math.floor(Math.random() * (index + 1));
        [copy[index], copy[other]] = [copy[other], copy[index]];
    }
    return copy;
}

export const PlaylistDetail: React.FC<PlaylistDetailProps> = ({ playlistId, onBack, onPlaySong, onSelect, currentSong, isPlaying, notify, onError }) => {
    const { t, songCount, language } = useI18n();
    const actions = useSongActions();
    // the library's own reads: a change made anywhere, a deletion included, shows here at once
    const playlists = useLibraryPlaylists();
    const library = useLibrarySongs();
    const playlist = playlists.data?.find(entry => entry.id === playlistId) ?? null;
    const loading = playlists.isPending || library.isPending;
    const songs = useMemo(() => (playlist ? playlistSongs(playlist, library.data ?? []) : []), [playlist, library.data]);
    const [confirmDelete, setConfirmDelete] = useState(false);
    const [editing, setEditing] = useState<Playlist | null>(null);
    const [menuAt, setMenuAt] = useState<{ x: number; y: number } | null>(null);
    const [songMenu, setSongMenu] = useState<{ song: Song; at: { x: number; y: number } } | null>(null);
    const [dragId, setDragId] = useState<string | null>(null);
    const [dropAt, setDropAt] = useState<number | null>(null);
    const [adding, setAdding] = useState<boolean | null>(null);
    const [query, setQuery] = useState('');

    // the order is shown at once and kept by the library; when it refuses, the list goes back
    const saveSongs = async (songIds: string[]) => {
        if (!playlist) return;
        const before = playlist;
        updateLibraryPlaylists(lists => lists.map(entry => (entry.id === before.id ? { ...entry, songIds, song_count: songIds.length } : entry)));
        try {
            const updated = await updateNativePlaylist(before.id, before, songIds);
            updateLibraryPlaylists(lists => lists.map(entry => (entry.id === updated.id ? updated : entry)));
        } catch (error) {
            updateLibraryPlaylists(lists => lists.map(entry => (entry.id === before.id ? before : entry)));
            onError(error instanceof Error ? error.message : String(error));
        }
    };

    const removeSong = (song: Song) => {
        if (!playlist) return;
        const ids = songs.map(entry => entry.id);
        void saveSongs(ids.filter(id => id !== song.id));
        notify(t('removedFromPlaylist').replace('{title}', song.title).replace('{name}', playlist.name), 'success', {
            label: t('undo'),
            onClick: () => void saveSongs(ids),
        });
    };

    const moveSong = (songId: string, to: number) => {
        const ids = songs.map(entry => entry.id);
        const from = ids.indexOf(songId);
        if (from < 0 || to === from || to === from + 1) return;
        ids.splice(from, 1);
        ids.splice(to > from ? to - 1 : to, 0, songId);
        void saveSongs(ids);
    };

    const addSong = (song: Song) => {
        if (!playlist || songs.some(entry => entry.id === song.id)) return;
        // the search stays open while songs are being picked
        setAdding(true);
        void saveSongs([...songs.map(entry => entry.id), song.id]);
    };

    const handleDeletePlaylist = async () => {
        if (!playlist) return;
        setConfirmDelete(false);
        try {
            await deleteNativePlaylist(playlist.id);
            updateLibraryPlaylists(lists => lists.filter(entry => entry.id !== playlist.id));
            onBack();
        } catch (error) {
            onError(error instanceof Error ? error.message : String(error));
        }
    };

    if (loading) return (
        <div className="flex items-center justify-center h-full bg-black">
            <div className="text-zinc-400 gap-2 flex items-center">
                <div className="w-4 h-4 border-2 border-zinc-400 border-t-transparent rounded-full animate-spin"></div>
                {t('loadingPlaylist')}
            </div>
        </div>
    );

    if (!playlist) return (
        <div className="flex flex-col items-center justify-center h-full gap-4 bg-black">
            <div className="text-zinc-400">{t('playlistNotFound')}</div>
            <button onClick={onBack} className="px-4 py-2 bg-zinc-800 hover:bg-zinc-700 rounded-lg text-white">
                {t('goBack')}
            </button>
        </div>
    );

    const playable = songs.filter(song => song.audioUrl && !song.isGenerating);
    const playingHere = Boolean(currentSong && songs.some(song => song.id === currentSong.id));
    const seconds = songs.reduce((sum, song) => sum + (parseDuration(song.duration) ?? 0), 0);
    const showAdd = adding ?? songs.length === 0;
    const inList = new Set(songs.map(song => song.id));
    const needle = query.trim().toLowerCase();
    const candidates = (library.data ?? [])
        .filter(song => song.audioUrl && !song.isGenerating && !inList.has(song.id))
        .filter(song => !needle || song.title.toLowerCase().includes(needle) || song.style.toLowerCase().includes(needle))
        .slice(0, 30);
    const menuActions = {
        play: () => playable.length && onPlaySong(playable[0], playable),
        shuffle: () => {
            const order = shuffled(playable);
            if (order.length) onPlaySong(order[0], order);
        },
        queue: (next: boolean) => actions.queue?.(playable, next),
        edit: () => setEditing(playlist),
        remove: () => setConfirmDelete(true),
    };
    const gradients = ['from-purple-900 to-black', 'from-blue-900 to-black', 'from-indigo-900 to-black', 'from-rose-900 to-black'];
    const bgGradient = gradients[playlist.name.length % gradients.length];

    return (
        <div className={`relative w-full h-full flex flex-col bg-linear-to-b ${bgGradient} overflow-hidden`}>
            {/* Header */}
            <div className="shrink-0 p-4 md:p-8 pt-12 md:pt-8 flex flex-col md:flex-row gap-4 md:gap-8 items-center md:items-end bg-black/20 backdrop-blur-lg border-b border-white/10">
                <button
                    type="button"
                    onClick={() => setEditing(playlist)}
                    {...named(t('editPlaylistDetails'))}
                    className="group relative h-32 w-32 shrink-0 overflow-hidden rounded-lg bg-zinc-800 shadow-2xl md:h-52 md:w-52"
                >
                    <PlaylistCover playlist={playlist} songs={songs} className="h-full w-full" />
                    <span className="absolute inset-0 flex items-center justify-center bg-black/50 opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100">
                        <Pencil size={36} className="text-white" />
                    </span>
                </button>

                <div className="min-w-0 flex-1 space-y-2 md:space-y-4 text-center md:text-left">
                    <span className="text-xs font-bold tracking-wider uppercase text-white/80">{t('playlist')}</span>
                    <h1>
                        <button
                            type="button"
                            onClick={() => setEditing(playlist)}
                            title={t('editPlaylistDetails')}
                            className="max-w-full wrap-break-word text-left text-2xl md:text-5xl lg:text-7xl font-bold text-white tracking-tight leading-none drop-shadow-lg hover:underline"
                        >
                            {playlist.name}
                        </button>
                    </h1>
                    {playlist.description && (
                        <p className="text-zinc-300 text-sm max-w-2xl">{playlist.description}</p>
                    )}
                    <div className="flex items-center justify-center md:justify-start gap-2 text-sm text-white font-medium flex-wrap">
                        <span>{songCount(songs.length)}</span>
                        {seconds > 0 && (
                            <>
                                <span className="w-1 h-1 rounded-full bg-white/50"></span>
                                <span className="text-zinc-300">{playlistLength(seconds, t)}</span>
                            </>
                        )}
                    </div>
                </div>
            </div>

            {/* Actions Bar */}
            <div className="px-4 md:px-8 py-3 md:py-4 bg-black/20 flex items-center gap-3 md:gap-4">
                <button
                    onClick={menuActions.play}
                    disabled={!playable.length}
                    {...named(playable.length ? t('play') : t('playlistNothingToPlay'))}
                    className="w-12 h-12 md:w-14 md:h-14 rounded-full bg-green-500 hover:scale-105 transition-transform flex items-center justify-center text-black shadow-lg disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:scale-100"
                >
                    {playingHere && isPlaying ? <Pause size={24} fill="currentColor" /> : <Play size={24} fill="currentColor" className="ml-1" />}
                </button>
                <button
                    onClick={menuActions.shuffle}
                    disabled={!playable.length}
                    {...named(t('shufflePlay'))}
                    className="p-2 text-zinc-300 transition-colors hover:text-white disabled:cursor-not-allowed disabled:opacity-40"
                >
                    <Shuffle size={22} />
                </button>
                <button
                    onClick={() => setEditing(playlist)}
                    {...named(t('editPlaylistDetails'))}
                    className="p-2 text-zinc-300 transition-colors hover:text-white"
                >
                    <Pencil size={20} />
                </button>
                <button
                    onClick={(event) => {
                        const box = event.currentTarget.getBoundingClientRect();
                        setMenuAt({ x: box.left, y: box.bottom + 6 });
                    }}
                    aria-haspopup="menu"
                    {...named(t('moreActions'))}
                    className="p-2 text-zinc-300 transition-colors hover:text-white"
                >
                    <MoreHorizontal size={22} />
                </button>
                <button
                    onClick={() => setConfirmDelete(true)}
                    className="p-2 text-zinc-400 transition-colors hover:text-red-500"
                    {...named(t('deletePlaylist'))}
                >
                    <Trash2 size={20} />
                </button>
            </div>

            {/* Song List */}
            <div className="flex-1 overflow-y-auto bg-black/40">
                <div className="px-2 md:px-8 py-2 md:py-4 pb-24 lg:pb-32">
                    <div>
                    {songs.length > 0 && (
                        <div className="hidden md:grid grid-cols-[16px_16px_4fr_3fr_2fr_minmax(96px,1fr)] gap-4 px-4 py-2 border-b border-white/10 text-sm font-medium text-zinc-400 mb-2 sticky top-0 bg-[#121212] z-10">
                            <span></span>
                            <span>#</span>
                            <span>{t('title')}</span>
                            <span>{t('style')}</span>
                            <span>{t('created')}</span>
                            <span className="text-right"><Clock size={16} className="inline" /></span>
                        </div>
                    )}

                    <ol className="space-y-1" onDragLeave={(event) => { if (event.currentTarget === event.target) setDropAt(null); }}>
                        {songs.map((song, index) => {
                            const current = currentSong?.id === song.id;
                            return (
                                <li
                                    key={song.id}
                                    data-mcp-context={`song ${song.id}: ${song.title}`}
                                    tabIndex={0}
                                    aria-current={current ? 'true' : undefined}
                                    onDragOver={(event) => {
                                        if (!dragId) return;
                                        event.preventDefault();
                                        const box = event.currentTarget.getBoundingClientRect();
                                        setDropAt(event.clientY < box.top + box.height / 2 ? index : index + 1);
                                    }}
                                    onDrop={(event) => {
                                        event.preventDefault();
                                        if (dragId && dropAt !== null) moveSong(dragId, dropAt);
                                        setDragId(null);
                                        setDropAt(null);
                                    }}
                                    onKeyDown={(event) => {
                                        if (event.altKey && (event.key === 'ArrowUp' || event.key === 'ArrowDown')) {
                                            event.preventDefault();
                                            moveSong(song.id, event.key === 'ArrowUp' ? index - 1 : index + 2);
                                        } else if (event.key === 'Enter') {
                                            onSelect(song);
                                            onPlaySong(song, playable);
                                        } else if (event.key === 'Delete') {
                                            removeSong(song);
                                        }
                                    }}
                                    onContextMenu={(event) => {
                                        event.preventDefault();
                                        setSongMenu({ song, at: { x: event.clientX, y: event.clientY } });
                                    }}
                                    className={`group relative flex md:grid md:grid-cols-[16px_16px_4fr_3fr_2fr_minmax(96px,1fr)] gap-3 md:gap-4 px-2 md:px-4 py-3 rounded-md items-center transition-colors text-sm cursor-pointer outline-hidden focus-visible:ring-2 focus-visible:ring-pink-500 ${current ? 'bg-white/10 text-white' : 'text-zinc-400 hover:bg-white/10 hover:text-white'} ${dragId === song.id ? 'opacity-40' : ''}`}
                                    onClick={() => {
                                        onSelect(song);
                                        onPlaySong(song, playable);
                                    }}
                                >
                                    {dropAt === index && <span className="pointer-events-none absolute -top-0.5 left-0 right-0 h-0.5 rounded-full bg-green-500" />}
                                    {dropAt === index + 1 && index === songs.length - 1 && <span className="pointer-events-none absolute -bottom-0.5 left-0 right-0 h-0.5 rounded-full bg-green-500" />}

                                    <span
                                        draggable
                                        onDragStart={(event) => {
                                            event.dataTransfer.effectAllowed = 'move';
                                            event.dataTransfer.setData('text/plain', song.id);
                                            setDragId(song.id);
                                        }}
                                        onDragEnd={() => {
                                            setDragId(null);
                                            setDropAt(null);
                                        }}
                                        onClick={(event) => event.stopPropagation()}
                                        title={t('dragToReorder')}
                                        className="hidden md:block cursor-grab text-zinc-600 opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100 active:cursor-grabbing"
                                    >
                                        <GripVertical size={16} />
                                    </span>

                                    <span className={`hidden md:block tabular-nums ${current ? 'text-green-500' : 'group-hover:text-white'}`}>
                                        {current && isPlaying ? (
                                            <span className="flex h-4 items-end gap-0.5" aria-label={t('nowPlaying')}>
                                                <span className="w-0.5 bg-green-500 music-bar-anim" style={{ animationDelay: '0s' }} />
                                                <span className="w-0.5 bg-green-500 music-bar-anim" style={{ animationDelay: '0.2s' }} />
                                                <span className="w-0.5 bg-green-500 music-bar-anim" style={{ animationDelay: '0.4s' }} />
                                            </span>
                                        ) : index + 1}
                                    </span>

                                    <div className="flex items-center gap-3 overflow-hidden flex-1 md:flex-none">
                                        <div className="w-12 h-12 md:w-10 md:h-10 rounded-sm bg-zinc-800 shrink-0 overflow-hidden relative group/img">
                                            <SongCover song={song} size="full" className="w-full h-full" />
                                            <button
                                                onClick={(e) => {
                                                    e.stopPropagation();
                                                    onPlaySong(song, playable);
                                                }}
                                                {...named(t('play'))}
                                                className="absolute inset-0 bg-black/50 flex md:hidden group-hover/img:flex items-center justify-center text-white"
                                            >
                                                <Play size={16} fill="white" />
                                            </button>
                                        </div>
                                        <div className="flex flex-col truncate min-w-0">
                                            <span className={`font-medium truncate ${current ? 'text-green-500' : 'text-white'}`}>{song.title}</span>
                                            <span className="text-xs text-zinc-500 group-hover:text-zinc-400 truncate md:hidden">
                                                {song.duration || '0:00'}
                                            </span>
                                        </div>
                                    </div>

                                    <span className="hidden md:block truncate" title={song.style}>{captionSummary(song.style)}</span>

                                    <span className="hidden md:block">
                                        {song.createdAt.toLocaleDateString(language)}
                                    </span>

                                    <div className="flex items-center justify-end gap-3 md:gap-4">
                                        <span className="hidden md:inline font-mono text-xs">{song.duration || '0:00'}</span>
                                        <button
                                            onClick={(e) => {
                                                e.stopPropagation();
                                                removeSong(song);
                                            }}
                                            {...named(t('removeFromPlaylist'))}
                                            className="p-2 text-zinc-500 transition-opacity hover:text-white md:p-0 md:opacity-0 md:group-hover:opacity-100 md:focus-visible:opacity-100"
                                        >
                                            <Trash2 size={16} />
                                        </button>
                                        <button
                                            onClick={(e) => {
                                                e.stopPropagation();
                                                const box = e.currentTarget.getBoundingClientRect();
                                                setSongMenu({ song, at: { x: box.right - 208, y: box.bottom + 4 } });
                                            }}
                                            aria-haspopup="menu"
                                            {...named(t('moreActions'))}
                                            className="hidden text-zinc-500 transition-opacity hover:text-white md:block md:opacity-0 md:group-hover:opacity-100 md:focus-visible:opacity-100"
                                        >
                                            <MoreHorizontal size={16} />
                                        </button>
                                    </div>
                                </li>
                            );
                        })}
                    </ol>
                    </div>

                    {/* songs to add, found right here: an empty playlist opens with it */}
                    <section className="mt-8 border-t border-white/10 pt-6" aria-labelledby="playlist-add-title">
                        {songs.length === 0 && (
                            <div className="mb-6 flex flex-col items-center gap-2 text-center text-zinc-400">
                                <ListMusic size={36} className="text-zinc-500" />
                                <p className="font-medium text-white">{t('playlistEmptyTitle')}</p>
                                <p className="max-w-md text-sm">{t('playlistEmptyHint')}</p>
                            </div>
                        )}
                        <div className="flex items-center justify-between gap-3">
                            <h2 id="playlist-add-title" className="text-lg font-bold text-white">{t('findSongsForPlaylist')}</h2>
                            <button
                                type="button"
                                onClick={() => setAdding(!showAdd)}
                                aria-expanded={showAdd}
                                className="rounded-full border border-white/20 px-3 py-1.5 text-xs font-semibold text-zinc-200 hover:bg-white/10"
                            >
                                {showAdd ? t('hideAddSongs') : t('addSongs')}
                            </button>
                        </div>
                        {showAdd && (
                            <div className="mt-4 space-y-3">
                                <div className="relative max-w-md">
                                    <Search size={16} className="absolute left-3 top-1/2 -translate-y-1/2 text-zinc-500" />
                                    <input
                                        type="search"
                                        value={query}
                                        onChange={(event) => setQuery(event.target.value)}
                                        placeholder={t('searchYourSongs')}
                                        aria-label={t('searchYourSongs')}
                                        className="w-full rounded-lg border border-white/10 bg-black/40 py-2.5 pl-9 pr-3 text-sm text-white placeholder-zinc-500 focus:border-white/30 focus:outline-hidden"
                                    />
                                </div>
                                {candidates.length === 0 ? (
                                    <p className="text-sm text-zinc-500">{needle ? t('noSongsMatchFilters') : t('allSongsInPlaylist')}</p>
                                ) : (
                                    <ul className="space-y-1">
                                        {candidates.map(song => (
                                            <li key={song.id} className="flex items-center gap-3 rounded-md px-2 py-2 text-sm text-zinc-300 hover:bg-white/10">
                                                <div className="h-10 w-10 shrink-0 overflow-hidden rounded-sm bg-zinc-800">
                                                    <SongCover song={song} size="full" className="h-full w-full" />
                                                </div>
                                                <div className="min-w-0 flex-1">
                                                    <p className="truncate font-medium text-white">{song.title}</p>
                                                    <p className="truncate text-xs text-zinc-500">{captionSummary(song.style)}</p>
                                                </div>
                                                <span className="font-mono text-xs text-zinc-500">{song.duration}</span>
                                                <button
                                                    type="button"
                                                    onClick={() => addSong(song)}
                                                    className="flex items-center gap-1 rounded-full border border-white/20 px-3 py-1 text-xs font-semibold text-white hover:border-white hover:bg-white/10"
                                                >
                                                    <Plus size={14} />
                                                    {t('add')}
                                                </button>
                                            </li>
                                        ))}
                                    </ul>
                                )}
                                {songs.length > 0 && <p className="flex items-center gap-1.5 text-xs text-zinc-500"><Check size={12} />{t('playlistReorderHint')}</p>}
                            </div>
                        )}
                    </section>
                </div>
            </div>

            <PlaylistMenu at={menuAt} empty={!playable.length} actions={menuActions} onClose={() => setMenuAt(null)} />
            {songMenu && <SongDropdownMenu song={songMenu.song} isOpen at={songMenu.at} onClose={() => setSongMenu(null)} />}
            <PlaylistEditModal playlist={editing} onClose={() => setEditing(null)} onError={onError} />
            <ConfirmDialog
                isOpen={confirmDelete}
                title={t('deletePlaylist')}
                message={t('deletePlaylistConfirm')}
                confirmLabel={t('deletePlaylist')}
                onConfirm={() => void handleDeletePlaylist()}
                onCancel={() => setConfirmDelete(false)}
            />

            <button
                onClick={onBack}
                {...named(t('goBack'))}
                className="absolute top-6 left-6 z-50 w-8 h-8 rounded-full bg-black/50 flex items-center justify-center text-white hover:bg-black/70 transition-colors"
            >
                <ArrowLeft size={18} />
            </button>
        </div>
    );
};

import React, { Activity, useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { Sidebar } from './components/Sidebar';
import { CreatePanel } from './components/CreatePanel';
import { SongList } from './components/SongList';
import { RightSidebar } from './components/RightSidebar';
import { Player } from './components/Player';
import { LibraryView } from './components/LibraryView';
import { CreatePlaylistModal, AddToPlaylistModal } from './components/PlaylistModals';
import { CoverRegenModal } from './components/CoverRegenModal';
import { ReplayModal } from './components/ReplayModal';
import { ProcessingModal } from './components/ProcessingModal';
import { VideoGeneratorModal } from './components/VideoGeneratorModal';
import { SongActions, SongActionsProvider } from './context/SongActionsContext';
import { useBridgeCommand } from './services/mcpBridge';
import { apiUrl } from './services/apiBase';
import { QueryClientProvider } from '@tanstack/react-query';
import { libraryChanged, queryClient, updateLibraryPlaylists, updateLibrarySongs, useLibraryPlaylists, useLibrarySongs, useSetupStatus } from './services/studioQueries';
import { useGenerations } from './services/generations';
import { STUDIO } from './studio';
import { SettingsModal } from './components/SettingsModal';
import { Song, View, Playlist } from './types';
// Resizable panel hook
const PANEL_MAX_SHARE = 0.4;
const PANEL_KEY_STEP = 16;

/** A side panel the user sizes by dragging its edge, with the arrow keys once
 *  the edge has focus, or back to its default with a double click. Wide
 *  screens get wide panels; the middle always keeps the rest of the window. */
function useResizablePanel(key: string, defaultWidth: number, min: number, max: number, direction: 'left' | 'right' = 'left', label = '') {
  const limitNow = React.useCallback(() => Math.max(min, Math.min(max, Math.round(window.innerWidth * PANEL_MAX_SHARE))), [min, max]);
  const [width, setWidth] = React.useState(() => {
    const saved = Number(localStorage.getItem(`panel-${key}`));
    return Number.isFinite(saved) && saved > 0 ? saved : defaultWidth;
  });
  const [limit, setLimit] = React.useState(limitNow);
  React.useEffect(() => {
    const onResize = () => setLimit(limitNow());
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, [limitNow]);
  // a width saved on a larger window is drawn within this one, and kept for the larger one
  const shown = Math.min(Math.max(width, min), limit);

  const commit = React.useCallback((next: number) => {
    const clamped = Math.min(limitNow(), Math.max(min, Math.round(next)));
    setWidth(clamped);
    localStorage.setItem(`panel-${key}`, String(clamped));
  }, [key, min, limitNow]);

  const onMouseDown = React.useCallback((e: React.MouseEvent) => {
    const startX = e.clientX;
    const startW = shown;
    const bound = limitNow();
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
    const at = (ev: MouseEvent) => Math.min(bound, Math.max(min, startW + (direction === 'left' ? 1 : -1) * (ev.clientX - startX)));
    const onMouseMove = (ev: MouseEvent) => setWidth(at(ev));
    const onMouseUp = (ev: MouseEvent) => {
      commit(at(ev));
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      document.removeEventListener('mousemove', onMouseMove);
      document.removeEventListener('mouseup', onMouseUp);
    };
    document.addEventListener('mousemove', onMouseMove);
    document.addEventListener('mouseup', onMouseUp);
  }, [shown, min, direction, limitNow, commit]);

  const onKeyDown = React.useCallback((e: React.KeyboardEvent) => {
    const grow = direction === 'left' ? 'ArrowRight' : 'ArrowLeft';
    const shrink = direction === 'left' ? 'ArrowLeft' : 'ArrowRight';
    const step = e.shiftKey ? PANEL_KEY_STEP * 4 : PANEL_KEY_STEP;
    const next = e.key === grow ? shown + step : e.key === shrink ? shown - step : e.key === 'Home' ? min : e.key === 'End' ? limitNow() : null;
    if (next === null) return;
    e.preventDefault();
    commit(next);
  }, [shown, min, direction, limitNow, commit]);

  const handle = (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-valuenow={shown}
      aria-valuemin={min}
      aria-valuemax={limit}
      tabIndex={0}
      onMouseDown={onMouseDown}
      onDoubleClick={() => {
        // the default is kept as it is; a narrow window only draws it narrower
        setWidth(defaultWidth);
        localStorage.removeItem(`panel-${key}`);
      }}
      onKeyDown={onKeyDown}
      className="hidden md:flex w-[5px] shrink-0 items-center justify-center cursor-col-resize group z-20 relative bg-zinc-200/50 dark:bg-zinc-800 hover:bg-pink-500/30 focus-visible:bg-pink-500/40 focus-visible:outline-hidden transition-colors"
    >
      <div className="w-[3px] h-10 rounded-full bg-zinc-400/30 dark:bg-zinc-600/50 group-hover:bg-pink-500 group-focus-visible:bg-pink-500 transition-colors" />
    </div>
  );

  return { width: shown, handle };
}
import { getAudioUrl } from './services/api';
import { useAuth } from './context/AuthContext';
import { useResponsive } from './context/ResponsiveContext';
import { I18nProvider, useI18n } from './context/I18nContext';
import { List } from 'lucide-react';
import { PlaylistDetail } from './components/PlaylistDetail';
import { Toast, ToastType } from './components/Toast';
import { FilesPanel } from './components/FilesPanel';
import { SearchPage } from './components/SearchPage';
import { NewsPage } from './components/NewsPage';
import { AdaptersPage } from './components/AdaptersPage';
import { PlayerExtras } from './components/player/PlayerExtras';
import { registerPlayer } from './services/audioGraph';
import { serveVisualizerFeed } from './services/visualizerFeed';
import { winampControl, winampOn } from './services/winamp';
import { ConfirmDialog } from './components/ConfirmDialog';
import { SetupGate } from './components/SetupGate';
import { EngineStarting } from './components/EngineStarting';
import { StudioOffline } from './components/StudioOffline';
import { StudioToolsPanel } from './components/StudioToolsPanel';
import { createNativePlaylist, deleteNativeSong, moveStoredLikes, setNativeSongLiked, updateNativePlaylist } from './services/nativeLibrary';
import { foldStems } from './services/songStems';
import { noteStudioMessage } from './services/journal';
import { JournalPanel } from './components/JournalPanel';
import { hubStateChanged, useHubState } from './services/studioQueries';
import { setTelemetry } from './services/studioHub';

/** Where versions before 3.3 kept the likes, in the window's own storage. */
const STORED_LIKES_KEY = 'native-liked-song-ids';
const NO_SONGS: Song[] = [];
const NO_PLAYLISTS: Playlist[] = [];

function storedLikes(): string[] {
  try {
    const stored: unknown = JSON.parse(localStorage.getItem(STORED_LIKES_KEY) || '[]');
    return Array.isArray(stored) ? stored.filter((id): id is string => typeof id === 'string') : [];
  } catch (error) {
    console.error('[ERROR] reading the likes kept in the window:', error);
    return [];
  }
}

function NativeUnavailableView({ title, detail }: { title: string; detail: string }): React.ReactElement {
  return (
    <div className="flex h-full min-h-0 flex-1 items-center justify-center overflow-y-auto bg-white px-6 py-10 dark:bg-suno">
      <section className="w-full max-w-xl rounded-2xl border border-amber-500/30 bg-amber-500/5 p-6 text-center shadow-xs">
        <p className="text-xs font-semibold uppercase tracking-[0.16em] text-amber-600 dark:text-amber-400">{STUDIO.name}</p>
        <h1 className="mt-2 text-xl font-bold text-zinc-950 dark:text-white">{title}</h1>
        <p className="mt-3 text-sm leading-6 text-zinc-600 dark:text-zinc-300">{detail}</p>
      </section>
    </div>
  );
}


function AppContent() {
  // i18n
  const { t, language } = useI18n();

  // Responsive
  const { isMobile, isDesktop } = useResponsive();

  // Auth
  const { user } = useAuth();
  const leftPanel = useResizablePanel('create', 420, 320, 1200, 'left', t('createMusic'));
  const rightPanel = useResizablePanel('details', 400, 320, 1200, 'right', t('songDetails'));
  const [nativeSetupReady, setNativeSetupReady] = useState(false);
  // A track sent here from a menu's "separate into stems".
  const [stemsSongId, setStemsSongId] = useState<string | null>(null);
  // Which of the three situations the studio is in. It starts unknown, and
  // unknown must not look like "nothing is installed": showing the download
  // page for a second on every launch is how a ready studio was made to look
  // like a bill.
  const [nativeModels, setNativeModels] = useState<'unknown' | 'missing' | 'installed' | 'offline'>('unknown');
  const setupQuery = useSetupStatus<{ ready?: boolean; engine_ready?: boolean }>();
  useEffect(() => {
    // A service that does not answer is not an engine that is still coming
    // up: the application has been closed, and saying "starting" at a dead
    // process is the one thing the window must not do.
    if (setupQuery.isError) {
      setNativeModels('offline');
      setNativeSetupReady(false);
      return;
    }
    const status = setupQuery.data;
    if (!status) return;
    setNativeModels(status.ready === true ? 'installed' : 'missing');
    if (status.ready && status.engine_ready) setNativeSetupReady(true);
  }, [setupQuery.data, setupQuery.isError]);
  useEffect(() => {
    const open = (event: Event) => {
      setStemsSongId((event as CustomEvent<string>).detail);
      setCurrentView('tools');
    };
    const openSettings = (event: Event) => {
      setSettingsSection((event as CustomEvent<string>).detail);
      setShowSettingsModal(true);
    };
    const process = (event: Event) => {
      const song = (event as CustomEvent<Song>).detail;
      if (song) setSongToProcess(song);
    };
    // A library song as the source of a cover: the create form opens on it in cover mode.
    const cover = (event: Event) => {
      const song = (event as CustomEvent<Song>).detail;
      if (!song) return;
      setReuseData({ song: { ...song, generationParams: { task_type: 'cover', source_song_id: song.id } }, timestamp: Date.now() });
      setCurrentView('create');
      setMobileShowList(false);
    };
    window.addEventListener('studio:open-stems', open);
    // A page asked for by another page, the LoRA card sending the user to its library.
    const navigate = (event: Event) => {
      const view = (event as CustomEvent<View>).detail;
      if (view) setCurrentView(view);
    };
    window.addEventListener('studio:open-settings', openSettings);
    window.addEventListener('studio:process-song', process);
    window.addEventListener('studio:cover-song', cover);
    window.addEventListener('studio:navigate', navigate);
    return () => {
      window.removeEventListener('studio:open-stems', open);
      window.removeEventListener('studio:open-settings', openSettings);
      window.removeEventListener('studio:process-song', process);
      window.removeEventListener('studio:cover-song', cover);
      window.removeEventListener('studio:navigate', navigate);
    };
  }, []);

  // Theme State
  // Dark is the studio's own look, not a preference inherited from the desktop:
  // the interface was drawn for it, and a light Windows was turning a music
  // studio into a spreadsheet on first launch. A user who picks light keeps it.
  const [theme, setTheme] = useState<'dark' | 'light'>(() => {
    const stored = localStorage.getItem('theme');
    return stored === 'light' ? 'light' : 'dark';
  });

  // Navigation State - default to create view
  const [currentView, setCurrentView] = useState<View>('create');

  // Content State: the library is the service's, read through the query cache
  // The playlist new songs go into, kept between sessions: the project the
  // user works on. The list beside the form shows it, as a workspace would.
  const [createPlaylistId, setCreatePlaylistId] = useState(() => {
    try { return window.localStorage.getItem('studio.createPlaylist') ?? ''; } catch { return ''; }
  });
  const chooseCreatePlaylist = (id: string) => {
    setCreatePlaylistId(id);
    try { window.localStorage.setItem('studio.createPlaylist', id); } catch { /* kept until a reload */ }
  };
  const libraryRead = useLibrarySongs();
  const librarySongs = libraryRead.data ?? NO_SONGS;
  const playlists = useLibraryPlaylists().data ?? NO_PLAYLISTS;
  // the like is kept with the song, so every window and the agent see one mark
  const likedSongIds = useMemo(() => new Set(librarySongs.filter(song => song.liked).map(song => song.id)), [librarySongs]);
  const likedSongs = useMemo(() => librarySongs.filter(song => song.liked), [librarySongs]);
  // Likes an earlier version kept in this window move into the library once,
  // after the first read shows the service answers.
  const likesMoved = useRef(false);
  useEffect(() => {
    if (likesMoved.current || !libraryRead.isSuccess) return;
    likesMoved.current = true;
    const stored = storedLikes();
    if (stored.length === 0) return;
    void moveStoredLikes(stored).then(left => {
      try {
        if (left.length === 0) localStorage.removeItem(STORED_LIKES_KEY);
        else localStorage.setItem(STORED_LIKES_KEY, JSON.stringify(left));
      } catch (error) {
        console.error('[ERROR] updating the likes kept in the window:', error);
      }
      if (left.length > 0) console.error(`[ERROR] ${left.length} likes stay in the window until the service takes them`);
      libraryChanged();
    });
  }, [libraryRead.isSuccess]);
  const [playQueue, setPlayQueue] = useState<Song[]>([]);
  const [queueIndex, setQueueIndex] = useState(-1);

  // Selection State
  const [currentSong, setCurrentSong] = useState<Song | null>(null);
  const [selectedSong, setSelectedSong] = useState<Song | null>(null);
  const [selectedPlaylist, setSelectedPlaylist] = useState<Playlist | null>(null);

  // Player State
  const [isPlaying, setIsPlaying] = useState(false);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [volume, setVolume] = useState(() => {
    const stored = localStorage.getItem('volume');
    return stored ? parseFloat(stored) : 0.8;
  });
  const [playbackRate, setPlaybackRate] = useState(1.0);
  const [isShuffle, setIsShuffle] = useState(() => {
    try { return localStorage.getItem('player.shuffle') === '1'; } catch { return false; }
  });
  // stop: play this track to its end and stay there, as Winamp's and
  // foobar's "stop after current"; next and previous still move by hand
  const [repeatMode, setRepeatMode] = useState<'none' | 'all' | 'one' | 'stop'>(() => {
    try {
      const stored = localStorage.getItem('player.repeat');
      return stored === 'none' || stored === 'one' || stored === 'stop' ? stored : 'all';
    } catch { return 'all'; }
  });
  useEffect(() => {
    try {
      localStorage.setItem('player.repeat', repeatMode);
      localStorage.setItem('player.shuffle', isShuffle ? '1' : '0');
    } catch { /* kept until a reload */ }
  }, [repeatMode, isShuffle]);
  const repeatModeRef = useRef(repeatMode);
  repeatModeRef.current = repeatMode;

  // UI State
  const [showRightSidebar, setShowRightSidebar] = useState(false);
  const [showLeftSidebar, setShowLeftSidebar] = useState(() => window.innerWidth >= 768);

  useEffect(() => {
    if (isMobile) setShowLeftSidebar(false);
  }, [isMobile]);

  // Mobile UI Toggle
  const [mobileShowList, setMobileShowList] = useState(false);

  // Modals
  const [isCreatePlaylistModalOpen, setIsCreatePlaylistModalOpen] = useState(false);
  const [isAddToPlaylistModalOpen, setIsAddToPlaylistModalOpen] = useState(false);
  const [songToAddToPlaylist, setSongToAddToPlaylist] = useState<Song | null>(null);

  // Video Modal

  // Cover regen modal — manual Pollinations / upload entry from SongList row
  // and RightSidebar. Updates songs.cover_url via /api/songs/:id/regen-cover.
  const [songForCoverRegen, setSongForCoverRegen] = useState<Song | null>(null);
  const [songForReplay, setSongForReplay] = useState<Song | null>(null);
  const [replayRequestRef, setReplayRequestRef] = useState('');
  const [songToProcess, setSongToProcess] = useState<Song | null>(null);
  const [songForVideo, setSongForVideo] = useState<Song | null>(null);

  // Settings Modal
  const [showSettingsModal, setShowSettingsModal] = useState(false);
  // Which settings page to land on, when something asks for a particular one.
  const [settingsSection, setSettingsSection] = useState<string | null>(null);

  // Profile View

  // Song View

  // Playlist View
  const [viewingPlaylistId, setViewingPlaylistId] = useState<string | null>(null);

  // Reuse State
  const [reuseData, setReuseData] = useState<{ song: Song, timestamp: number } | null>(null);

  const audioRef = useRef<HTMLAudioElement | null>(null);
  const selectedSongRef = useRef<Song | null>(null);
  const currentSongIdRef = useRef<string | null>(null);
  const pendingSeekRef = useRef<number | null>(null);
  const playNextRef = useRef<() => void>(() => {});

  // Mobile Details Modal State
  const [showMobileDetails, setShowMobileDetails] = useState(false);

  // Toast State
  const [toast, setToast] = useState<{ message: string; type: ToastType; isVisible: boolean }>({
    message: '',
    type: 'success',
    isVisible: false,
  });

  // Confirm Dialog State
  const [confirmDialog, setConfirmDialog] = useState<{
    title: string;
    message: string;
    onConfirm: () => void;
  } | null>(null);


  // a message shown for a moment stays readable in the journal
  const showToast = (message: string, type: ToastType = 'success') => {
    setToast({ message, type, isVisible: true });
    noteStudioMessage(message, type);
  };

  const [journalOpen, setJournalOpen] = useState(false);

  const closeToast = () => {
    setToast(prev => ({ ...prev, isVisible: false }));
  };

  // The songs being made: their cards, the jobs they follow, and the row each
  // hands to the song it becomes.
  const generations = useGenerations({
    enabled: nativeSetupReady,
    notify: (message, type) => showToast(message, type),
    onFinished: (cardId, made) => {
      // a card that was open in the details stays open as the song it became
      setSelectedSong(current => (current?.id === cardId ? made[0] : current));
      if (window.innerWidth < 768) setMobileShowList(true);
    },
  });
  // The list beside the form shows the playlist the songs go into, and the
  // songs being made for it; with none chosen it is the whole library.
  const createScope = playlists.find(entry => entry.id === createPlaylistId) ?? null;
  const createSongs = useMemo(() => {
    if (!createScope) return generations.songs;
    const inside = new Set(createScope.songIds ?? []);
    return generations.songs.filter(song => (song.isGenerating || song.stage === 'cancelled'
      ? song.playlistId === createScope.id
      : inside.has(song.id)));
  }, [generations.songs, createScope]);

  // The library asked for while the service was still starting came back
  // empty; once the service answers again it is read afresh.
  const wasOffline = useRef(false);
  useEffect(() => {
    if (nativeModels === 'offline') {
      wasOffline.current = true;
      return;
    }
    if (wasOffline.current && nativeModels !== 'unknown') {
      wasOffline.current = false;
      libraryChanged();
    }
  }, [nativeModels]);

  const handleNativeReplay = useCallback((song: Song) => {
    if (!song.nativeReplayAvailable) return;
    setReplayRequestRef(`replay_${Date.now()}_${Math.random().toString(36).slice(2, 11)}`);
    setSongForReplay(song);
  }, []);
  const closeReplay = useCallback(() => setSongForReplay(null), []);

  // Keep selectedSongRef in sync for use in callbacks without stale closures
  useEffect(() => { selectedSongRef.current = selectedSong; }, [selectedSong]);

  const handleShowDetails = (song: Song) => {
    setSelectedSong(song);
    setShowMobileDetails(true);
  };

  // Reuse Handler
  const handleReuse = (song: Song) => {
    setReuseData({ song, timestamp: Date.now() });
    setCurrentView('create');
    setMobileShowList(false);
  };

  // Song Update Handler
  const handleSongUpdate = (updatedSong: Song) => {
    updateLibrarySongs(songs => songs.map(s => s.id === updatedSong.id ? updatedSong : s));
    if (currentSong?.id === updatedSong.id) {
      setCurrentSong(updatedSong);
    }
    if (selectedSong?.id === updatedSong.id) {
      setSelectedSong(updatedSong);
    }
  };

  // Theme Effect
  useEffect(() => {
    localStorage.setItem('theme', theme);
    if (theme === 'dark') {
      document.documentElement.classList.add('dark');
    } else {
      document.documentElement.classList.remove('dark');
    }
  }, [theme]);

  const toggleTheme = () => {
    setTheme(prev => prev === 'dark' ? 'light' : 'dark');
  };

  // URL Routing Effect
  useEffect(() => {
    const handleUrlChange = () => {
      const path = window.location.pathname;
      const params = new URLSearchParams(window.location.search);

      if (path === '/create' || path === '/') {
        setCurrentView('create');
        setMobileShowList(false);
      } else if (path === '/library') {
        setCurrentView('library');
      } else if (path.startsWith('/playlist/')) {
        const playlistId = path.substring(10);
        if (playlistId) {
          setViewingPlaylistId(playlistId);
          setCurrentView('playlist');
        }
      } else if (path === '/search') {
        setCurrentView('search');
      } else if (path === '/news') {
        setCurrentView('news');
      }
    };

    handleUrlChange();

    window.addEventListener('popstate', handleUrlChange);
    return () => window.removeEventListener('popstate', handleUrlChange);
  }, []);



  // Player Logic
  // with no queue chosen the library plays, its songs without their stems
  const libraryQueue = useMemo(() => foldStems(librarySongs, librarySongs).songs, [librarySongs]);
  const getActiveQueue = () => (playQueue.length > 0 ? playQueue : libraryQueue);

  const playNext = useCallback(() => {
    if (!currentSong) return;
    const queue = getActiveQueue();
    if (queue.length === 0) return;

    const currentIndex = queueIndex >= 0 && queue[queueIndex]?.id === currentSong.id
      ? queueIndex
      : queue.findIndex(s => s.id === currentSong.id);
    if (currentIndex === -1) return;

    if (repeatMode === 'one') {
      if (audioRef.current) {
        audioRef.current.currentTime = 0;
        audioRef.current.play();
      }
      return;
    }

    // Find next playable song (has audioUrl and not generating)
    const queueLen = queue.length;
    for (let i = 1; i <= queueLen; i++) {
      let nextIndex;
      if (isShuffle) {
        nextIndex = Math.floor(Math.random() * queueLen);
        if (queueLen > 1 && nextIndex === currentIndex) continue;
      } else {
        nextIndex = currentIndex + i;
        // In 'none' repeat mode, stop at end of queue
        if (repeatMode === 'none' && nextIndex >= queueLen) {
          setIsPlaying(false);
          return;
        }
        nextIndex = nextIndex % queueLen;
      }

      const candidate = queue[nextIndex];
      if (candidate.audioUrl && !candidate.isGenerating) {
        setQueueIndex(nextIndex);
        setCurrentSong(candidate);
        setIsPlaying(true);
        return;
      }
    }

    // No playable songs found
    setIsPlaying(false);
  }, [currentSong, queueIndex, isShuffle, repeatMode, playQueue, librarySongs]);

  const playPrevious = useCallback(() => {
    if (!currentSong) return;
    const queue = getActiveQueue();
    if (queue.length === 0) return;

    const currentIndex = queueIndex >= 0 && queue[queueIndex]?.id === currentSong.id
      ? queueIndex
      : queue.findIndex(s => s.id === currentSong.id);
    if (currentIndex === -1) return;

    if (currentTime > 3) {
      if (audioRef.current) audioRef.current.currentTime = 0;
      return;
    }

    // Find previous playable song (has audioUrl and not generating)
    const queueLen = queue.length;
    for (let i = 1; i <= queueLen; i++) {
      let prevIndex;
      if (isShuffle) {
        prevIndex = Math.floor(Math.random() * queueLen);
        if (queueLen > 1 && prevIndex === currentIndex) continue;
      } else {
        prevIndex = currentIndex - i;
        // In 'none' repeat mode, stop at beginning of queue
        if (repeatMode === 'none' && prevIndex < 0) {
          if (audioRef.current) audioRef.current.currentTime = 0;
          return;
        }
        prevIndex = (prevIndex + queueLen) % queueLen;
      }

      const candidate = queue[prevIndex];
      if (candidate.audioUrl && !candidate.isGenerating) {
        setQueueIndex(prevIndex);
        setCurrentSong(candidate);
        setIsPlaying(true);
        return;
      }
    }

    // No playable songs found
    setIsPlaying(false);
  }, [currentSong, queueIndex, currentTime, isShuffle, repeatMode, playQueue, librarySongs]);

  useEffect(() => {
    playNextRef.current = playNext;
  }, [playNext]);

  // Audio Setup
  useEffect(() => {
    audioRef.current = new Audio();
    audioRef.current.crossOrigin = "anonymous";
    const audio = audioRef.current;
    audio.volume = volume;
    // the equalizer and the visualisers hear the player through a Web Audio graph made when first needed
    registerPlayer(audio);

    const onTimeUpdate = () => setCurrentTime(audio.currentTime);
    const applyPendingSeek = () => {
      if (pendingSeekRef.current === null) return;
      if (audio.seekable.length === 0) return;
      const target = pendingSeekRef.current;
      const safeTarget = Number.isFinite(audio.duration)
        ? Math.min(Math.max(target, 0), audio.duration)
        : Math.max(target, 0);
      audio.currentTime = safeTarget;
      setCurrentTime(safeTarget);
      pendingSeekRef.current = null;
    };

    const onLoadedMetadata = () => {
      setDuration(audio.duration);
      applyPendingSeek();
    };

    const onCanPlay = () => {
      applyPendingSeek();
    };

    const onProgress = () => {
      applyPendingSeek();
    };

    const onEnded = () => {
      if (repeatModeRef.current === 'stop') {
        setIsPlaying(false);
        return;
      }
      playNextRef.current();
    };

    const onError = (e: Event) => {
      if (audio.error && audio.error.code !== 1) {
        console.error("Audio playback error:", audio.error);
        if (audio.error.code === 4) {
          showToast(t('songNotAvailable'), 'error');
        } else {
          showToast(t('unableToPlay'), 'error');
        }
      }
      setIsPlaying(false);
    };

    audio.addEventListener('timeupdate', onTimeUpdate);
    audio.addEventListener('loadedmetadata', onLoadedMetadata);
    audio.addEventListener('canplay', onCanPlay);
    audio.addEventListener('progress', onProgress);
    audio.addEventListener('ended', onEnded);
    audio.addEventListener('error', onError);

    return () => {
      audio.pause();
      audio.removeEventListener('timeupdate', onTimeUpdate);
      audio.removeEventListener('loadedmetadata', onLoadedMetadata);
      audio.removeEventListener('canplay', onCanPlay);
      audio.removeEventListener('progress', onProgress);
      audio.removeEventListener('ended', onEnded);
      audio.removeEventListener('error', onError);
    };
  }, []);

  // Handle Playback State
  useEffect(() => {
    const audio = audioRef.current;
    if (!audio || !currentSong?.audioUrl) return;

    const playAudio = async () => {
      try {
        await audio.play();
      } catch (err) {
        if (err instanceof Error && err.name !== 'AbortError') {
          console.error("Playback failed:", err);
          if (err.name === 'NotSupportedError') {
            showToast(t('songNotAvailable'), 'error');
          }
          setIsPlaying(false);
        }
      }
    };

    if (currentSongIdRef.current !== currentSong.id) {
      currentSongIdRef.current = currentSong.id;
      audio.src = currentSong.audioUrl;
      audio.load();
      if (isPlaying) playAudio();
    } else {
      if (isPlaying) playAudio();
      else audio.pause();
    }
  }, [currentSong, isPlaying]);

  // Handle Volume
  useEffect(() => {
    if (audioRef.current) {
      audioRef.current.volume = volume;
    }
    localStorage.setItem('volume', String(volume));
  }, [volume]);

  // Handle Playback Rate
  useEffect(() => {
    if (audioRef.current) {
      audioRef.current.playbackRate = playbackRate;
    }
  }, [playbackRate]);

  // a visualiser in its own window hears the player through this window
  useEffect(() => serveVisualizerFeed(), []);

  // Spacebar play/pause
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.code !== 'Space' || winampOn()) return;
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || (e.target as HTMLElement)?.isContentEditable) return;
      e.preventDefault();
      if (currentSong) {
        if (currentSong.audioUrl) {
          setIsPlaying(prev => !prev);
        }
      } else {
        // No song selected — play first available
        const available = librarySongs.filter(s => s.audioUrl);
        if (available.length > 0) {
          playSong(available[0], available);
        }
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [currentSong, librarySongs]);


  const togglePlay = () => {
    const song = currentSong || selectedSong;
    if (!song) return;
    if (!song.audioUrl) {
      showToast(t('songNotAvailable'), 'error');
      return;
    }
    // If no currentSong yet, start playing the selected song
    if (!currentSong && song) {
      playSong(song);
      return;
    }
    setIsPlaying(!isPlaying);
  };

  const playFirst = () => {
    const available = librarySongs.filter(s => s.audioUrl);
    if (available.length > 0) {
      playSong(available[0], available);
    }
  };

  const playSong = (song: Song, list?: Song[]) => {
    const nextQueue = list && list.length > 0
      ? list
      : (playQueue.length > 0 && playQueue.some(s => s.id === song.id))
          ? playQueue
          : (libraryQueue.some(s => s.id === song.id) ? libraryQueue : [song]);
    const nextIndex = nextQueue.findIndex(s => s.id === song.id);
    setPlayQueue(nextQueue);
    setQueueIndex(nextIndex);

    if (currentSong?.id !== song.id) {
      const updatedSong = { ...song, viewCount: (song.viewCount || 0) + 1 };
      setCurrentSong(updatedSong);
      setSelectedSong(updatedSong);
      setIsPlaying(true);
      updateLibrarySongs(songs => songs.map(s => s.id === song.id ? { ...s, viewCount: updatedSong.viewCount } : s));
    } else {
      togglePlay();
    }
    if (currentSong?.id === song.id) {
      setSelectedSong(song);
    }
  };

  // An agent connected over MCP works this window like a user
  const VIEWS: View[] = ['create', 'library', 'tools', 'adapters', 'playlist', 'search', 'news'];
  const songById = (id: unknown) => {
    const song = librarySongs.find(entry => entry.id === id);
    if (!song) throw new Error(`No song ${String(id)} in the library; library_songs_list gives the ids.`);
    return song;
  };
  useBridgeCommand('navigate', ({ view }) => {
    if (!VIEWS.includes(view as View)) throw new Error(`Pages: ${VIEWS.join(', ')}.`);
    setCurrentView(view as View);
    return { text: `On ${String(view)}.` };
  });
  useBridgeCommand('notify', ({ text, tone }) => {
    const kind: ToastType = tone === 'error' || tone === 'success' ? tone : 'info';
    showToast(String(text ?? ''), kind);
    return { text: 'Shown.' };
  });
  useBridgeCommand('open_settings', ({ section }) => {
    setSettingsSection(typeof section === 'string' ? section : null);
    setShowSettingsModal(true);
    return { text: 'Settings are open.' };
  });
  useBridgeCommand('player_state', () => winampControl() ? { winamp: true, ...winampControl()!.state() } : ({
    song: currentSong ? { id: currentSong.id, title: currentSong.title } : null,
    playing: isPlaying,
    position_seconds: Math.round(currentTime * 10) / 10,
    duration_seconds: Math.round(duration * 10) / 10,
    volume,
    shuffle: isShuffle,
    repeat: repeatMode,
    queue: playQueue.length,
  }));
  // while the Winamp mode is on, Winamp is the player the agent drives
  const inWinamp = async (change: Record<string, unknown>) => ({ text: `Winamp: ${await winampControl()!.set(change)}` });
  useBridgeCommand('player_play', ({ song_id, song_ids, stem }) => {
    if (Array.isArray(song_ids) && song_ids.length) {
      // a list of songs becomes the queue, played from its first
      const list = song_ids.map((id) => songById(id)).filter((song) => song.audioUrl);
      if (!list.length) throw new Error('None of those songs has audio to play.');
      if (winampControl()) return inWinamp({ song_ids: list.map((song) => song.id) });
      playSong(list[0], list);
      return { text: `Playing ${list.length} song(s) from ${list[0].title}.` };
    }
    if (winampControl() && !stem) return inWinamp(song_id ? { song_id } : { action: 'play' });
    if (song_id && stem) {
      // one separated stem of the song, played on its own
      const song = songById(song_id);
      const take: Song = { ...song, id: `${song.id}#${String(stem)}`, title: `${song.title} · ${String(stem)}`, audioUrl: apiUrl(`/v1/library/songs/${encodeURIComponent(song.id)}/stems/${encodeURIComponent(String(stem))}`) };
      playSong(take, [take]);
      return { text: `Playing the ${String(stem)} stem of ${song.title}.` };
    }
    if (song_id) {
      const song = songById(song_id);
      if (currentSong?.id !== song.id) playSong(song);
      else setIsPlaying(true);
      return { text: `Playing ${song.title}.` };
    }
    if (!currentSong) throw new Error('Nothing is loaded; pass song_id.');
    setIsPlaying(true);
    return { text: `Playing ${currentSong.title}.` };
  });
  useBridgeCommand('player_pause', () => {
    if (winampControl()) return inWinamp({ action: 'pause' });
    setIsPlaying(false);
    return { text: 'Paused.' };
  });
  useBridgeCommand('player_seek', ({ seconds }) => {
    if (winampControl()) return inWinamp({ seek_seconds: Number(seconds) || 0 });
    handleSeek(Number(seconds) || 0);
    return { text: `At ${Number(seconds) || 0} s.` };
  });
  useBridgeCommand('player_next', () => {
    if (winampControl()) return inWinamp({ action: 'next' });
    playNext();
    return { text: 'Next song.' };
  });
  useBridgeCommand('player_previous', () => {
    if (winampControl()) return inWinamp({ action: 'previous' });
    playPrevious();
    return { text: 'Previous song.' };
  });
  useBridgeCommand('player_set', ({ volume: level, shuffle, repeat }) => {
    if (winampControl()) {
      return inWinamp({
        ...(level !== undefined ? { volume: Number(level) } : {}),
        ...(shuffle !== undefined ? { shuffle: Boolean(shuffle) } : {}),
        ...(repeat === 'none' || repeat === 'all' ? { repeat: repeat === 'all' } : {}),
      });
    }
    if (level !== undefined) setVolume(Math.max(0, Math.min(1, Number(level))));
    if (shuffle !== undefined) setIsShuffle(Boolean(shuffle));
    if (repeat === 'none' || repeat === 'all' || repeat === 'one' || repeat === 'stop') setRepeatMode(repeat);
    return { text: 'Set.' };
  });
  useBridgeCommand('video_open', ({ song_id }) => {
    const song = songById(song_id);
    setSongForVideo(song);
    return { text: `The video editor is open for ${song.title}.` };
  });
  useBridgeCommand('video_close', () => {
    setSongForVideo(null);
    return { text: 'The video editor is closed.' };
  });

  const handleSeek = (time: number) => {
    const audio = audioRef.current;
    if (!audio) return;
    if (Number.isNaN(audio.duration) || audio.readyState < 1 || audio.seekable.length === 0) {
      pendingSeekRef.current = time;
      return;
    }
    audio.currentTime = time;
    setCurrentTime(time);
  };

  /// The thumbs-up is shown at once and kept in the library; when the service
  /// refuses it, the mark goes back and the window says why.
  const toggleLike = (songId: string) => {
    const liked = !likedSongIds.has(songId);
    const before = librarySongs.find(song => song.id === songId);
    const mark = (value: boolean, at: Date | undefined) => updateLibrarySongs(songs => songs.map(song => (
      song.id === songId ? { ...song, liked: value, likedAt: at } : song
    )));
    mark(liked, liked ? new Date() : undefined);
    setNativeSongLiked(songId, liked)
      .then(stored => updateLibrarySongs(songs => songs.map(song => (song.id === songId ? stored : song))))
      .catch((error: unknown) => {
        // the mark goes back to what it was, the moment of the like with it
        mark(!liked, before?.likedAt);
        showToast(error instanceof Error ? error.message : String(error), 'error');
      });
  };

  const handleDeleteSong = (song: Song) => {
    handleDeleteSongs([song]);
  };

  const handleDeleteSongs = (songsToDelete: Song[]) => {
    if (songsToDelete.length === 0) return;

    const isSingle = songsToDelete.length === 1;
    const title = isSingle ? t('confirmDeleteTitle') : t('confirmDeleteManyTitle');
    const message = isSingle
      ? t('deleteSongConfirm').replace('{title}', () => songsToDelete[0].title)
      : t('deleteSongsConfirm').replace('{count}', String(songsToDelete.length));

    setConfirmDialog({
      title,
      message,
      onConfirm: async () => {
        setConfirmDialog(null);

        const idsToDelete = new Set(songsToDelete.map(song => song.id));
        const succeeded: string[] = [];
        const failed: string[] = [];

        for (const song of songsToDelete) {
          try {
            await deleteNativeSong(song.id);
            succeeded.push(song.id);
          } catch (error) {
            console.error('Failed to delete song:', error);
            failed.push(song.id);
          }
        }

        if (succeeded.length > 0) {
          updateLibrarySongs(songs => songs.filter(s => !idsToDelete.has(s.id) || failed.includes(s.id)));

          if (selectedSong?.id && succeeded.includes(selectedSong.id)) {
            setSelectedSong(null);
          }

          if (currentSong?.id && succeeded.includes(currentSong.id)) {
            setCurrentSong(null);
            setIsPlaying(false);
            if (audioRef.current) {
              audioRef.current.pause();
              audioRef.current.src = '';
            }
          }

          setPlayQueue(prev => prev.filter(s => !idsToDelete.has(s.id) || failed.includes(s.id)));
        }

        if (failed.length > 0) {
          showToast(t('songsDeletedPartial').replace('{succeeded}', String(succeeded.length)).replace('{total}', String(songsToDelete.length)), 'error');
        } else if (isSingle) {
          showToast(t('songDeleted'));
        } else {
          showToast(t('songsDeletedSuccess'));
        }
      },
    });
  };

  const createPlaylist = async (name: string, description: string) => {
    try {
      const playlist = await createNativePlaylist(name, description, songToAddToPlaylist ? [songToAddToPlaylist.id] : []);
      updateLibraryPlaylists(prev => [playlist, ...prev]);
      if (songToAddToPlaylist) setSongToAddToPlaylist(null);
      showToast(t('playlistCreated'));
    } catch (error) {
      console.error('Create playlist error:', error);
      showToast(t('failedToCreatePlaylist'), 'error');
    }
  };

  // A playlist made from the create form, for the songs still to come.
  const createEmptyPlaylist = async (name: string, description: string): Promise<Playlist | null> => {
    try {
      const playlist = await createNativePlaylist(name, description, []);
      updateLibraryPlaylists(prev => [playlist, ...prev]);
      showToast(t('playlistCreated'));
      return playlist;
    } catch (error) {
      console.error('Create playlist error:', error);
      showToast(t('failedToCreatePlaylist'), 'error');
      return null;
    }
  };

  const openAddToPlaylistModal = (song: Song) => {
    setSongToAddToPlaylist(song);
    setIsAddToPlaylistModalOpen(true);
  };

  const addSongToPlaylist = async (playlistId: string) => {
    if (!songToAddToPlaylist) return;
    try {
      const playlist = playlists.find(item => item.id === playlistId);
      if (!playlist) throw new Error('Playlist was not found in the local library');
      const songIds = Array.from(new Set([...(playlist.songIds || []), songToAddToPlaylist.id]));
      const updated = await updateNativePlaylist(playlist.id, playlist, songIds);
      updateLibraryPlaylists(prev => prev.map(item => item.id === updated.id ? updated : item));
      setSongToAddToPlaylist(null);
      showToast(t('songAddedToPlaylist'));
    } catch (error) {
      console.error('Add song error:', error);
      showToast(t('failedToAddSong'), 'error');
    }
  };

  const handleNavigateToPlaylist = (playlistId: string) => {
    setViewingPlaylistId(playlistId);
    setCurrentView('playlist');
    window.history.pushState({}, '', `/playlist/${playlistId}`);
  };





  const handleBackFromPlaylist = () => {
    setViewingPlaylistId(null);
    setCurrentView('library');
    window.history.pushState({}, '', '/library');
  };

  const openCoverRegen = (song: Song) => {
    // Cover work is non-destructive, so playback deliberately keeps running.
    setSongForCoverRegen(song);
  };

  // Apply the new cover URL to local state without a full /api/songs reload
  // (the backend already wrote songs.cover_url; we just need the UI to
  // reflect it). Cache-bust by appending a timestamp so <img> re-fetches.
  const applyCoverUpdate = useCallback((songId: string, coverUrl: string) => {
    const bust = `${coverUrl}${coverUrl.includes('?') ? '&' : '?'}t=${Date.now()}`;
    updateLibrarySongs(songs => songs.map(s => s.id === songId ? { ...s, coverUrl: bust } : s));
    setSelectedSong(prev => prev?.id === songId ? { ...prev, coverUrl: bust } : prev);
  }, []);


  // Background work reports its outcome here once, and the toast goes away.
  useEffect(() => {
    const onToast = (event: Event) => {
      const { message, type } = (event as CustomEvent<{ message: string; type?: ToastType }>).detail;
      showToast(message, type ?? 'info');
    };
    window.addEventListener('studio:toast', onToast);
    return () => window.removeEventListener('studio:toast', onToast);
  }, []);

  // Render Layout Logic
  // The create page stays mounted once the engine is up, hidden while another
  // page shows: leaving it for the library used to throw away the style, the
  // lyrics, the score and every setting typed into it.
  const createKept = nativeModels !== 'offline' && nativeSetupReady;
  const showingCreate = !['tools', 'adapters', 'library', 'playlist', 'search', 'news'].includes(currentView);

  // the details show the library's copy of the song, so a cover or a rename
  // made after it was opened appears there too
  const selectedShown = selectedSong ? librarySongs.find(song => song.id === selectedSong.id) ?? selectedSong : null;

  const renderContent = (view: typeof currentView = currentView) => {
    switch (view) {
      case 'tools':
        return <StudioToolsPanel initialSongId={stemsSongId} />;

      case 'library': {
        return (
          <LibraryView
            allSongs={librarySongs}
            likedSongs={likedSongs}
            playlists={playlists}
            onPlaySong={playSong}
            currentSong={currentSong}
            isPlaying={isPlaying}
            onCreatePlaylist={() => {
              setSongToAddToPlaylist(null);
              setIsCreatePlaylistModalOpen(true);
            }}
            onSelectPlaylist={(p) => handleNavigateToPlaylist(p.id)}
            onImported={libraryChanged}
          />
        );
      }

      case 'playlist':
        if (!viewingPlaylistId) return null;
        return (
          <PlaylistDetail
            playlistId={viewingPlaylistId}
            onBack={handleBackFromPlaylist}
            onPlaySong={playSong}
            onSelect={(s) => {
              setSelectedSong(s);
              setShowRightSidebar(true);
            }}
            onError={message => showToast(message, 'error')}
          />
        );

      case 'search':
        return (
          <SearchPage
            songs={librarySongs}
            playlists={playlists}
            onPlaySong={playSong}
            currentSong={currentSong}
            isPlaying={isPlaying}
            onNavigateToPlaylist={handleNavigateToPlaylist}
          />
        );

      case 'news':
        return <NewsPage />;

      case 'adapters':
        return <AdaptersPage />;

      case 'create':
      default:
        // Two different situations, two different screens: nothing installed
        // is a decision to make, and an engine coming up is a wait to sit
        // through. They used to be the same page, which made a running studio
        // look like it owed 26 GB.
        if (nativeModels === 'offline') return <StudioOffline />;
        if (!nativeSetupReady) {
          // Until the studio has answered, and while the engine is coming up,
          // this is a wait - not a decision. The download page appears only
          // when components are genuinely missing.
          return nativeModels === 'missing'
            ? <SetupGate onReady={() => setNativeSetupReady(true)} />
            : <EngineStarting onReady={() => setNativeSetupReady(true)} />;
        }
        return (
          <div className="relative flex h-full min-h-0 min-w-0 w-full overflow-hidden">
            {/* Create Panel */}
            <div
              className={`
                ${mobileShowList ? 'hidden md:block' : 'w-full'}
                md:block min-h-0 min-w-0 shrink-0 h-full bg-zinc-50 dark:bg-suno-panel relative z-10 transition-colors duration-300
              `}
              style={{ width: window.innerWidth >= 768 ? leftPanel.width : undefined }}
            >
              <CreatePanel
                onGenerate={generations.generate}
                isGenerating={generations.isGenerating}
                activeJobCount={generations.activeJobCount}
                initialData={reuseData}
                playlists={playlists}
                onCreatePlaylist={createEmptyPlaylist}
                playlistId={createPlaylistId}
                onChoosePlaylist={chooseCreatePlaylist}
              />
            </div>
            {leftPanel.handle}

            {/* Song List */}
            <div className={`
              ${!mobileShowList ? 'hidden md:flex' : 'flex'}
              min-h-0 min-w-0 flex-1 flex-col h-full overflow-hidden bg-white dark:bg-suno transition-colors duration-300
            `}>
              <SongList
                songs={createSongs}
                scopeName={createScope?.name}
                loading={libraryRead.isPending}
                librarySongs={librarySongs}
                currentSong={currentSong}
                selectedSong={selectedSong}
                likedSongIds={likedSongIds}
                isPlaying={isPlaying}
                onPlay={playSong}
                onSelect={(s) => {
                  setSelectedSong(s);
                  setShowRightSidebar(true);
                }}
                onToggleLike={toggleLike}
                onAddToPlaylist={openAddToPlaylistModal}
                onOpenCoverRegen={openCoverRegen}
                onShowDetails={handleShowDetails}
                onDeleteMany={handleDeleteSongs}
                onSongUpdate={handleSongUpdate}
                onCancelJob={generations.cancel}
                onResetJob={generations.reset}
                onCancelAll={generations.cancelAll}
                onResetAll={generations.cancelAll}
                activeJobCount={generations.activeJobCount}
              />
            </div>

            {/* Right Sidebar */}
            {showRightSidebar && selectedShown && (
              <>
              {rightPanel.handle}
              <div
                className="hidden xl:block min-h-0 min-w-0 shrink-0 h-full bg-zinc-50 dark:bg-suno-panel relative z-10 transition-colors duration-300"
                style={{ width: rightPanel.width }}
              >
                <RightSidebar
                  song={selectedShown}
                  onClose={() => setShowRightSidebar(false)}
                  onOpenCoverRegen={() => selectedSong && openCoverRegen(selectedSong)}
                  onReuse={handleReuse}
                  onSongUpdate={handleSongUpdate}
                  isLiked={selectedSong ? likedSongIds.has(selectedSong.id) : false}
                  onToggleLike={toggleLike}
                  onPlay={playSong}
                  isPlaying={isPlaying}
                  currentSong={currentSong}
                />
              </div>
              </>
            )}

            {/* Mobile Toggle Button */}
            <div className="md:hidden absolute top-4 right-4 z-50">
              <button
                onClick={() => setMobileShowList(!mobileShowList)}
                className="bg-zinc-800 text-white px-4 py-2 rounded-full shadow-lg border border-white/10 flex items-center gap-2 text-sm font-bold"
              >
                {mobileShowList ? t('createSong') : t('viewList')}
                <List size={16} />
              </button>
            </div>
          </div>
        );
    }
  };

  // Every song menu and song button takes its actions from here. The value
  // keeps one identity, so the playing clock does not re-render every row.
  const songActionsLatest = useRef<Required<SongActions>>(null as never);
  songActionsLatest.current = {
    reusePrompt: handleReuse,
    replay: handleNativeReplay,
    exportVideo: setSongForVideo,
    addToPlaylist: openAddToPlaylistModal,
    remove: handleDeleteSong,
    update: handleSongUpdate,
  };
  const songActions = useMemo<SongActions>(() => ({
    reusePrompt: song => songActionsLatest.current.reusePrompt(song),
    replay: song => songActionsLatest.current.replay(song),
    exportVideo: song => songActionsLatest.current.exportVideo(song),
    addToPlaylist: song => songActionsLatest.current.addToPlaylist(song),
    remove: song => songActionsLatest.current.remove(song),
    update: song => songActionsLatest.current.update(song),
  }), []);

  // Anonymous statistics are on by default: the start screen's checkbox decides on a first run, and an install that
  // updated past that screen keeps the default until it is unchecked in Settings.
  const hub = useHubState(language);
  useEffect(() => {
    const telemetry = hub.data?.telemetry;
    if (!nativeSetupReady || !telemetry || telemetry.acknowledged || telemetry.disabledByEnv) return;
    setTelemetry(telemetry.enabled, true).catch((error: Error) => console.warn('[hub] the statistics default was not saved:', error.message)).finally(hubStateChanged);
  }, [nativeSetupReady, hub.data?.telemetry.acknowledged]);

  return (
    <SongActionsProvider value={songActions}>
    <div className="flex h-dvh min-h-0 min-w-0 flex-col overflow-hidden bg-white dark:bg-suno text-zinc-900 dark:text-white font-sans antialiased selection:bg-pink-500/30 transition-colors duration-300">
      <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden">
        <Sidebar
          currentView={currentView}
          onNavigate={(v) => {
            setCurrentView(v);
            if (v === 'create') {
              setMobileShowList(false);
              window.history.pushState({}, '', '/');
            } else if (v === 'library') {
              window.history.pushState({}, '', '/library');
            } else if (v === 'search') {
              window.history.pushState({}, '', '/search');
            } else if (v === 'news') {
              window.history.pushState({}, '', '/news');
            } else if (v === 'tools') {
              window.history.pushState({}, '', '/tools');
            }
            if (isMobile) setShowLeftSidebar(false);
          }}
          theme={theme}
          onToggleTheme={toggleTheme}
          user={user}
          onOpenSettings={() => setShowSettingsModal(true)}
          isOpen={showLeftSidebar}
          onToggle={() => setShowLeftSidebar(!showLeftSidebar)}
          journalOpen={journalOpen}
          onToggleJournal={() => setJournalOpen(open => !open)}
        />

        <main className="relative ml-[72px] flex min-h-0 min-w-0 flex-1 overflow-hidden md:ml-0">
          {createKept && <Activity mode={showingCreate ? 'visible' : 'hidden'}>{renderContent('create')}</Activity>}
          {!(createKept && showingCreate) && renderContent()}
        </main>
      </div>

      {(currentSong || selectedSong) && <Player
        currentSong={currentSong || selectedSong}
        isPlaying={isPlaying}
        onTogglePlay={togglePlay}
        currentTime={currentTime}
        duration={duration}
        onSeek={handleSeek}
        onNext={playNext}
        onPrevious={playPrevious}
        volume={volume}
        onVolumeChange={setVolume}
        playbackRate={playbackRate}
        onPlaybackRateChange={setPlaybackRate}
        audioRef={audioRef}
        isShuffle={isShuffle}
        onToggleShuffle={() => setIsShuffle(!isShuffle)}
        repeatMode={repeatMode}
        onToggleRepeat={() => setRepeatMode(prev => prev === 'none' ? 'all' : prev === 'all' ? 'one' : prev === 'one' ? 'stop' : 'none')}
        isLiked={currentSong ? likedSongIds.has(currentSong.id) : false}
        onToggleLike={() => currentSong && toggleLike(currentSong.id)}
        onPlayFirst={playFirst}
      />}

      <PlayerExtras
        queue={playQueue}
        currentSong={currentSong}
        currentTime={currentTime}
        isPlaying={isPlaying}
        volume={volume}
        library={librarySongs}
        onPause={() => setIsPlaying(false)}
        onLeaveWinamp={(exit) => {
          setVolume(exit.volume);
          const song = exit.songId ? librarySongs.find((entry) => entry.id === exit.songId) : null;
          if (song && song.id !== currentSong?.id) {
            // the position waits for the new song to load
            pendingSeekRef.current = exit.seconds;
            playSong(song, playQueue.some((entry) => entry.id === song.id) ? playQueue : undefined);
          } else if (song) {
            handleSeek(exit.seconds);
          }
          // Winamp played a file from disk: the studio's own song stays paused
          setIsPlaying(Boolean(song) && exit.playing);
        }}
        onSavePlaylist={async (songIds) => {
          const playlist = await createNativePlaylist(`Winamp ${new Date().toLocaleString()}`, '', songIds);
          updateLibraryPlaylists((prev) => [playlist, ...prev]);
          showToast(t('playlistCreated'));
        }}
      />

      <CreatePlaylistModal
        isOpen={isCreatePlaylistModalOpen}
        onClose={() => setIsCreatePlaylistModalOpen(false)}
        onCreate={createPlaylist}
      />
      <AddToPlaylistModal
        isOpen={isAddToPlaylistModalOpen}
        onClose={() => setIsAddToPlaylistModalOpen(false)}
        playlists={playlists}
        onSelect={addSongToPlaylist}
        onCreateNew={() => {
          setIsAddToPlaylistModalOpen(false);
          setIsCreatePlaylistModalOpen(true);
        }}
      />
      <FilesPanel />
      <Toast
        message={toast.message}
        type={toast.type}
        isVisible={toast.isVisible}
        onClose={closeToast}
        duration={toast.type === 'error' ? 8000 : 3000}
      />
      {/* Cover regen modal — only mounted while a song is selected for regen.
          Unmounting on close revokes blob URLs (see CoverRegenModal cleanup
          effect) so generated previews don't leak across modal opens. */}
      <VideoGeneratorModal
        isOpen={Boolean(songForVideo)}
        song={songForVideo}
        onClose={() => setSongForVideo(null)}
      />
      {songToProcess && (
        <ProcessingModal
          song={songToProcess}
          onClose={() => setSongToProcess(null)}
          onKept={libraryChanged}
        />
      )}
      {songForReplay && (
        <ReplayModal
          song={songForReplay}
          clientRef={replayRequestRef}
          onClose={closeReplay}
          onQueued={(jobId) => {
            // A re-render is a generation like any other: it gets its own card
            // with the engine's stages, and lands in the library the same way.
            generations.track(jobId, { title: songForReplay.title, lyrics: songForReplay.lyrics, style: songForReplay.style });
            showToast(t('replayQueued'));
          }}
        />
      )}
      {songForCoverRegen && (
        <CoverRegenModal
          song={songForCoverRegen}
          onClose={() => setSongForCoverRegen(null)}
          onCoverSaved={applyCoverUpdate}
        />
      )}
      <SettingsModal
        isOpen={showSettingsModal}
        initialSection={settingsSection}
        onClose={() => { setShowSettingsModal(false); setSettingsSection(null); }}
        theme={theme}
        onToggleTheme={toggleTheme}
      />

      {/* Mobile Details Modal */}
      {showMobileDetails && selectedShown && (
        <div className="fixed inset-0 z-60 flex justify-end xl:hidden">
          <div
            className="absolute inset-0 bg-black/60 backdrop-blur-xs animate-in fade-in"
            onClick={() => setShowMobileDetails(false)}
          />
          <div className="relative w-full max-w-md h-full bg-zinc-50 dark:bg-suno-panel shadow-2xl animate-in slide-in-from-right duration-300 border-l border-white/10">
            <RightSidebar
              song={selectedShown}
              onClose={() => setShowMobileDetails(false)}
              onOpenCoverRegen={() => selectedSong && openCoverRegen(selectedSong)}
              onReuse={handleReuse}
              onSongUpdate={handleSongUpdate}
              isLiked={selectedSong ? likedSongIds.has(selectedSong.id) : false}
              onToggleLike={toggleLike}
              onPlay={playSong}
              isPlaying={isPlaying}
              currentSong={currentSong}
            />
          </div>
        </div>
      )}

      <JournalPanel open={journalOpen} onClose={() => setJournalOpen(false)} />

      <ConfirmDialog
        isOpen={confirmDialog !== null}
        title={confirmDialog?.title ?? ''}
        message={confirmDialog?.message ?? ''}
        onConfirm={() => confirmDialog?.onConfirm()}
        onCancel={() => setConfirmDialog(null)}
      />
    </div>
    </SongActionsProvider>
  );
}

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <I18nProvider>
        <AppContent />
      </I18nProvider>
    </QueryClientProvider>
  );
}

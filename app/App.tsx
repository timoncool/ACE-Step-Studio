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
import { aceLogProgress } from './services/engineProgress';
import { STUDIO } from './studio';
import { SettingsModal } from './components/SettingsModal';
import { Song, AceCreateRequest, AceJob, View, Playlist } from './types';
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
      className="hidden md:flex w-[5px] flex-shrink-0 items-center justify-center cursor-col-resize group z-20 relative bg-zinc-200/50 dark:bg-zinc-800 hover:bg-pink-500/30 focus-visible:bg-pink-500/40 focus-visible:outline-none transition-colors"
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
import { SearchPage } from './components/SearchPage';
import { NewsPage } from './components/NewsPage';
import { AdaptersPage } from './components/AdaptersPage';
import { ConfirmDialog } from './components/ConfirmDialog';
import { SetupGate } from './components/SetupGate';
import { EngineStarting } from './components/EngineStarting';
import { StudioOffline } from './components/StudioOffline';
import { StudioToolsPanel } from './components/StudioToolsPanel';
import { createNativePlaylist, deleteNativeSong, loadNativeLibrarySongs, loadNativePlaylists, updateNativePlaylist } from './services/nativeLibrary';

const NATIVE_LIKED_SONG_IDS_KEY = 'native-liked-song-ids';

function loadNativeLikedSongIds(): Set<string> {
  try {
    const stored = JSON.parse(localStorage.getItem(NATIVE_LIKED_SONG_IDS_KEY) || '[]');
    return new Set(Array.isArray(stored) ? stored.filter((id): id is string => typeof id === 'string') : []);
  } catch {
    return new Set();
  }
}

function saveNativeLikedSongIds(ids: Set<string>): void {
  localStorage.setItem(NATIVE_LIKED_SONG_IDS_KEY, JSON.stringify([...ids]));
}

function NativeUnavailableView({ title, detail }: { title: string; detail: string }): React.ReactElement {
  return (
    <div className="flex h-full min-h-0 flex-1 items-center justify-center overflow-y-auto bg-white px-6 py-10 dark:bg-suno">
      <section className="w-full max-w-xl rounded-2xl border border-amber-500/30 bg-amber-500/5 p-6 text-center shadow-sm">
        <p className="text-xs font-semibold uppercase tracking-[0.16em] text-amber-600 dark:text-amber-400">{STUDIO.name}</p>
        <h1 className="mt-2 text-xl font-bold text-zinc-950 dark:text-white">{title}</h1>
        <p className="mt-3 text-sm leading-6 text-zinc-600 dark:text-zinc-300">{detail}</p>
      </section>
    </div>
  );
}


function AppContent() {
  // i18n
  const { t } = useI18n();

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
  useEffect(() => {
    const read = () => void fetch('/setup/status')
      .then((response) => (response.ok ? response.json() : Promise.reject(new Error())))
      .then((status: { ready?: boolean; engine_ready?: boolean }) => {
        setNativeModels(status.ready === true ? 'installed' : 'missing');
        if (status.ready && status.engine_ready) setNativeSetupReady(true);
      })
      // A service that does not answer is not an engine that is still coming
      // up: the application has been closed, and saying "starting" at a dead
      // process is the one thing the window must not do.
      .catch(() => {
        setNativeModels('offline');
        setNativeSetupReady(false);
      });
    read();
    const timer = window.setInterval(read, 2000);
    return () => window.clearInterval(timer);
  }, []);
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
    window.addEventListener('studio:open-stems', open);
    // A page asked for by another page, the LoRA card sending the user to its library.
    const navigate = (event: Event) => {
      const view = (event as CustomEvent<View>).detail;
      if (view) setCurrentView(view);
    };
    window.addEventListener('studio:open-settings', openSettings);
    window.addEventListener('studio:process-song', process);
    window.addEventListener('studio:navigate', navigate);
    return () => {
      window.removeEventListener('studio:open-stems', open);
      window.removeEventListener('studio:open-settings', openSettings);
      window.removeEventListener('studio:process-song', process);
      window.removeEventListener('studio:navigate', navigate);
    };
  }, []);

  // Track multiple concurrent generation jobs
  const activeJobsRef = useRef<Map<string, { tempId: string; pollInterval: ReturnType<typeof setInterval> }>>(new Map());
  const [activeJobCount, setActiveJobCount] = useState(0);

  // FIFO drain barrier — handlers awaiting it block until the active-jobs
  // queue is empty. Used by CreatePanel to chain LLM pre-flight calls behind
  // the previous track's full completion (LLM + audio + cover) — that's the
  // user's "queue" mental model: gen N+1 starts only after gen N is done.
  const queueDrainResolversRef = useRef<Array<() => void>>([]);
  const waitForJobsToDrain = useCallback((): Promise<void> => {
    if (activeJobsRef.current.size === 0) return Promise.resolve();
    return new Promise((resolve) => {
      queueDrainResolversRef.current.push(resolve);
    });
  }, []);
  const drainQueueWaiters = useCallback(() => {
    if (activeJobsRef.current.size !== 0) return;
    const waiters = queueDrainResolversRef.current;
    queueDrainResolversRef.current = [];
    waiters.forEach((r) => r());
  }, []);

  // "Pending click" counter — bumped synchronously the moment the user
  // clicks Создать, so the button shows N/10 instantly even before the LLM
  // pre-flight completes. Decremented when the click hands off to a real
  // active job (beginPollingJob has registered it in activeJobsRef).
  const [pendingClickCount, setPendingClickCount] = useState(0);
  const incrementPendingClicks = useCallback((n = 1) => setPendingClickCount(c => c + n), []);

  // Pre-flight AbortController registry, keyed by the placeholder card's
  // tempId. CreatePanel registers a controller right before it starts the
  // OpenRouter pre-flight call; the cancel buttons (single + cancel-all)
  // pull from here to actually abort the in-flight HTTP request, otherwise
  // the user's only escape is reloading the page (the Promise chain that
  // park clicks via `waitForJobsToDrain` doesn't have an abort path of
  // its own — see handoff "Open issue #1").
  const preflightAbortersRef = useRef<Map<string, AbortController>>(new Map());
  const registerPreflightAbort = useCallback((tempId: string, ac: AbortController) => {
    preflightAbortersRef.current.set(tempId, ac);
  }, []);
  const unregisterPreflightAbort = useCallback((tempId: string) => {
    preflightAbortersRef.current.delete(tempId);
  }, []);
  const decrementPendingClicks = useCallback((n = 1) => setPendingClickCount(c => Math.max(0, c - n)), []);

  // Instant temp-song factory — called from CreatePanel at click time so the
  // user sees a card in the list IMMEDIATELY, then it's promoted with real
  // data when LLM pre-flight + POST complete. Returns the tempId so the
  // caller can stash it on the eventual `onGenerate` payload (`_tempId`).
  const createTempSongForClick = useCallback((descriptionPreview: string): string => {
    const tempId = `temp_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;
    const tempSong: Song = {
      id: tempId,
      title: descriptionPreview.slice(0, 60) || (t('generating') || 'Generating…'),
      lyrics: '',
      style: '',
      coverUrl: 'https://picsum.photos/200/200?blur=10',
      duration: '--:--',
      createdAt: new Date(),
      isGenerating: true,
      // Use the i18n key — SongList renders via t(song.stage) || song.stage.
      stage: 'stageWaitingInQueue',
      tags: ['queued'],
      isPublic: true,
    };
    setSongs(prev => [tempSong, ...prev]);
    return tempId;
  }, [t]);

  // Update placeholder fields as LLM streams data, e.g. style/lyrics.
  const updateTempSongForClick = useCallback((tempId: string, patch: Partial<Song>) => {
    setSongs(prev => prev.map(s => s.id === tempId ? { ...s, ...patch } : s));
  }, []);

  // Failure path — drop the placeholder so the user doesn't see a stuck "Queued…"
  // BUT only if the card is still a placeholder (no `jobId` yet). Once App.tsx
  // handleGenerate has POSTed and beginPollingJob set jobId on the song, the
  // card represents a real running backend job — wiping it would leave the
  // user with audio gen running invisibly. Skip in that case.
  const removeTempSongForClick = useCallback((tempId: string) => {
    setSongs(prev => prev.filter(s => {
      if (s.id !== tempId) return true;
      // Promoted to active job → keep
      if (s.jobId) return true;
      return false;
    }));
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

  // Content State
  const [songs, setSongs] = useState<Song[]>([]);
  const [playlists, setPlaylists] = useState<Playlist[]>([]);
  const [likedSongIds, setLikedSongIds] = useState<Set<string>>(new Set());
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
  const [isShuffle, setIsShuffle] = useState(false);
  const [repeatMode, setRepeatMode] = useState<'none' | 'all' | 'one'>('all');

  // UI State
  const [isGenerating, setIsGenerating] = useState(false);
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


  const showToast = (message: string, type: ToastType = 'success') => {
    setToast({ message, type, isVisible: true });
  };

  const closeToast = () => {
    setToast(prev => ({ ...prev, isVisible: false }));
  };

  const refreshNativeLibrary = useCallback(async (): Promise<boolean> => {
    try {
      const [nativeSongs, nativePlaylists] = await Promise.all([loadNativeLibrarySongs(), loadNativePlaylists()]);
      setSongs(prev => {
        const generatingSongs = prev.filter(song => song.isGenerating);
        return [...generatingSongs, ...nativeSongs];
      });
      setPlaylists(nativePlaylists);
      setLikedSongIds(loadNativeLikedSongIds());
      // A fresh native library is still the authoritative store. Falling back
      // to the retired ACE service when it is empty made ordinary first-run
      // actions issue requests to a server that is not part of this desktop app.
      return true;
    } catch {
      return false;
    }
  }, []);

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
      void refreshNativeLibrary();
    }
  }, [nativeModels, refreshNativeLibrary]);

  const handleNativeReplay = useCallback((song: Song) => {
    if (!song.nativeReplayAvailable) return;
    setSongForReplay(song);
  }, []);

  // Keep selectedSongRef in sync for use in callbacks without stale closures
  useEffect(() => { selectedSongRef.current = selectedSong; }, [selectedSong]);

  // Cleanup active jobs on unmount
  useEffect(() => {
    return () => {
      // Clear all polling intervals when component unmounts
      activeJobsRef.current.forEach(({ pollInterval }) => {
        clearInterval(pollInterval);
      });
      activeJobsRef.current.clear();
    };
  }, []);

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
    setSongs(prev => prev.map(s => s.id === updatedSong.id ? updatedSong : s));
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

  // Load the native library once at start and whenever the studio signals a
  // change: generation import, replay, audio import, a cover or karaoke timings
  // that finish after the track is already on screen.
  useEffect(() => {
    void refreshNativeLibrary();
    const reload = () => { void refreshNativeLibrary(); };
    window.addEventListener('studio:library-changed', reload);
    return () => window.removeEventListener('studio:library-changed', reload);
  }, [refreshNativeLibrary]);


  // Player Logic
  const getActiveQueue = (song?: Song) => {
    if (playQueue.length > 0) return playQueue;
    if (song && songs.some(s => s.id === song.id)) return songs;
    return songs;
  };

  const playNext = useCallback(() => {
    if (!currentSong) return;
    const queue = getActiveQueue(currentSong);
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
  }, [currentSong, queueIndex, isShuffle, repeatMode, playQueue, songs]);

  const playPrevious = useCallback(() => {
    if (!currentSong) return;
    const queue = getActiveQueue(currentSong);
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
  }, [currentSong, queueIndex, currentTime, isShuffle, repeatMode, playQueue, songs]);

  useEffect(() => {
    playNextRef.current = playNext;
  }, [playNext]);

  // Audio Setup
  useEffect(() => {
    audioRef.current = new Audio();
    audioRef.current.crossOrigin = "anonymous";
    const audio = audioRef.current;
    audio.volume = volume;

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

  // Spacebar play/pause
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.code !== 'Space') return;
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || (e.target as HTMLElement)?.isContentEditable) return;
      e.preventDefault();
      if (currentSong) {
        if (currentSong.audioUrl) {
          setIsPlaying(prev => !prev);
        }
      } else {
        // No song selected — play first available
        const available = songs.filter(s => s.audioUrl && !s.isGenerating);
        if (available.length > 0) {
          playSong(available[0], available);
        }
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [currentSong, songs]);

  // Helper to cleanup a job and check if all jobs are done
  const cleanupJob = useCallback((jobId: string, tempId: string) => {
    const jobData = activeJobsRef.current.get(jobId);
    if (jobData) {
      clearInterval(jobData.pollInterval);
      activeJobsRef.current.delete(jobId);
    }

    // Remove temp song
    setSongs(prev => prev.filter(s => s.id !== tempId));

    // Update active job count
    setActiveJobCount(activeJobsRef.current.size);

    // If no more active jobs, set isGenerating to false
    if (activeJobsRef.current.size === 0) {
      setIsGenerating(false);
    }
    drainQueueWaiters();
  }, []);

  // Cancel a single generation. The `id` may be either:
  //  - a backend jobId (track is past pre-flight, audio gen is running) → POST /cancel
  //  - a pre-flight tempId (still in OpenRouter LLM call, no jobId yet)  → abort the
  //    registered AbortController, drop the placeholder card, release slot
  //
  // We unify both paths under one handler because the SongList row only knows
  // `song.id` (= tempId) and `song.jobId`; a pre-flight card has tempId but
  // no jobId, so the cancel button passes whatever it has and we figure it
  // out here.
  /// One handler for both card kinds: a pre-flight placeholder only has a
  /// tempId (no engine job yet, so there is nothing to cancel remotely), while
  /// a submitted card carries the engine's job id.
  const stopEngineJob = useCallback(async (jobId: string) => {
    try {
      await fetch(`/v1/music/jobs/${encodeURIComponent(jobId)}`, { method: 'POST' });
    } catch (error) {
      console.error('Cancel request failed:', error);
    }
  }, []);

  const cancelGeneration = useCallback(async (id: string) => {
    const preflightAc = preflightAbortersRef.current.get(id);
    if (preflightAc) {
      preflightAc.abort();
      preflightAbortersRef.current.delete(id);
      setSongs(prev => prev.map(song => song.id === id ? { ...song, isGenerating: false, stage: 'cancelled' } : song));
      decrementPendingClicks(1);
      drainQueueWaiters();
      return;
    }

    await stopEngineJob(id);
    const jobData = activeJobsRef.current.get(id);
    if (jobData) {
      clearInterval(jobData.pollInterval);
      activeJobsRef.current.delete(id);
      setActiveJobCount(activeJobsRef.current.size);
      if (activeJobsRef.current.size === 0) setIsGenerating(false);
      drainQueueWaiters();
      setSongs(prev => prev.map(song =>
        song.id === jobData.tempId ? { ...song, isGenerating: false, stage: 'cancelled' } : song
      ));
    }
  }, [drainQueueWaiters, decrementPendingClicks, stopEngineJob]);

  /// Reset drops the card as well as the job: the engine is asked to stop, then
  /// the placeholder is removed so the list matches reality.
  const resetSingleJob = useCallback(async (id: string) => {
    const jobData = activeJobsRef.current.get(id);
    if (!jobData) {
      const aborter = preflightAbortersRef.current.get(id);
      if (aborter) {
        aborter.abort();
        preflightAbortersRef.current.delete(id);
      }
      setSongs(prev => prev.filter(song => song.id !== id));
      drainQueueWaiters();
      return;
    }

    await stopEngineJob(id);
    clearInterval(jobData.pollInterval);
    activeJobsRef.current.delete(id);
    setSongs(prev => prev.filter(song => song.id !== jobData.tempId));
    setActiveJobCount(activeJobsRef.current.size);
    if (activeJobsRef.current.size === 0) setIsGenerating(false);
    drainQueueWaiters();
  }, [drainQueueWaiters, stopEngineJob]);

  const cancelAllGenerations = useCallback(async () => {
    preflightAbortersRef.current.forEach(aborter => aborter.abort());
    preflightAbortersRef.current.clear();

    const running = [...activeJobsRef.current.entries()];
    await Promise.all(running.map(([jobId]) => stopEngineJob(jobId)));
    running.forEach(([, { pollInterval }]) => clearInterval(pollInterval));
    const tempIds = new Set(running.map(([, job]) => job.tempId));
    activeJobsRef.current.clear();
    setSongs(prev => prev.filter(song => !tempIds.has(song.id) && !(song.isGenerating && !song.jobId)));
    setActiveJobCount(0);
    setIsGenerating(false);
    drainQueueWaiters();
    setPendingClickCount(0);
  }, [drainQueueWaiters, stopEngineJob]);

  const resetGeneration = cancelAllGenerations;

  // Refresh songs list (called when any job completes successfully)
  const refreshSongsList = useCallback(async () => {
    await refreshNativeLibrary();
  }, [refreshNativeLibrary]);

  const beginPollingJob = useCallback((jobId: string, tempId: string) => {
    if (activeJobsRef.current.has(jobId)) return;

    const pollInterval = setInterval(async () => {
      try {
        const response = await fetch(`/v1/music/jobs/${encodeURIComponent(jobId)}`);
        if (!response.ok) throw new Error(`Job status request failed (${response.status})`);
        const job: AceJob = await response.json();

        // The stage comes from the engine-log poll, which knows the one job the
        // engine is rendering; this poll only reacts to the terminal states.
        if (job.status === 'completed') {
          cleanupJob(jobId, tempId);
          setSongs(prev => prev.filter(song => song.id !== tempId));
          await refreshSongsList();
          const finished = job.songs?.[0] ?? job.song;
          if (finished?.id) setSelectedSong(current => current?.id === tempId ? null : current);
          showToast(job.songs && job.songs.length > 1
            ? `${job.songs.length} ${t('tracksReady') || 'tracks ready'}`
            : (t('trackReady') || 'Track ready'));
          if (window.innerWidth < 768) setMobileShowList(true);
        } else if (job.status === 'failed' || job.status === 'cancelled') {
          cleanupJob(jobId, tempId);
          setSongs(prev => prev.filter(song => song.id !== tempId));
          showToast(job.message || `${t('generationFailed')}`, job.status === 'failed' ? 'error' : 'info');
        }
      } catch (error) {
        console.error(`Polling error for job ${jobId}:`, error);
        cleanupJob(jobId, tempId);
        setSongs(prev => prev.filter(song => song.id !== tempId));
        showToast(error instanceof Error ? error.message : String(error), 'error');
      }
    }, 1500);

    activeJobsRef.current.set(jobId, { tempId, pollInterval });
    setActiveJobCount(activeJobsRef.current.size);
  }, [cleanupJob, refreshSongsList, t]);

  /// Jobs keep running in the service when the window reloads, and an agent
  /// connected over MCP starts its own: both get a card, and the same poller
  /// lands their tracks. The service is asked every few seconds.
  useEffect(() => {
    if (!nativeSetupReady) return;
    let busy = false;
    const adopt = () => {
      if (busy) return;
      busy = true;
      void fetch('/v1/music/jobs')
        .then(response => (response.ok ? response.json() : []))
        .then((jobs: AceJob[]) => {
          const tracked = new Set(activeJobsRef.current.keys());
          // A job this window has just submitted is its own to track; the id is
          // a UUIDv7, whose first 48 bits are the moment it was made.
          const ageMs = (id: string) => {
            const hex = id.replace(/^ace-/, '').replace(/-/g, '').slice(0, 12);
            const made = Number.parseInt(hex, 16);
            return Number.isFinite(made) ? Date.now() - made : Number.POSITIVE_INFINITY;
          };
          const fresh = jobs.filter(job => !tracked.has(job.id) && ageMs(job.id) > 6000);
          if (fresh.length === 0) return;
          setSongs(prev => [
            ...fresh.map(job => ({
              id: `restored_${job.id}`,
              title: job.title || t('generating') || 'Generating...',
              lyrics: job.lyrics || '',
              style: job.caption || '',
              coverUrl: '',
              duration: '--:--',
              createdAt: new Date(),
              isGenerating: true,
              jobId: job.id,
              stage: 'stageWaitingInQueue',
              tags: ['ace-step'],
            })),
            ...prev,
          ]);
          setIsGenerating(true);
          fresh.forEach(job => beginPollingJob(job.id, `restored_${job.id}`));
        })
        .catch(() => undefined)
        .finally(() => { busy = false; });
    };
    adopt();
    const timer = window.setInterval(adopt, 4000);
    return () => window.clearInterval(timer);
  }, [nativeSetupReady, beginPollingJob, t]);

  /// The engine reports a job's state, not a percentage, but its log counts the
  /// language model's code steps and the DiT steps. Reading that gives the
  /// generating card a real progress bar instead of an invented one.
  useEffect(() => {
    if (activeJobCount === 0) return;
    let cancelled = false;

    const poll = async () => {
      try {
        const response = await fetch('/v1/engine/logs');
        if (!response.ok) return;
        const body: { lines?: string[] } = await response.json();
        const progressOf = aceLogProgress(body.lines ?? []);
        const progress = progressOf?.progress;
        const stage = progressOf?.stage;
        if (cancelled || progress === undefined) return;
        // The engine renders one job at a time, in the order they were submitted,
        // and its log reports that one job. Only the oldest still-generating song
        // is actually being worked on; the rest wait at nought.
        setSongs(prev => {
          const generating = prev.filter(song => song.isGenerating && song.jobId);
          if (generating.length === 0) return prev;
          const active = generating.reduce((oldest, song) =>
            (song.createdAt?.getTime() ?? 0) < (oldest.createdAt?.getTime() ?? 0) ? song : oldest,
          );
          return prev.map(song => {
            if (!song.isGenerating || !song.jobId) return song;
            if (song.id === active.id) return { ...song, progress, stage: stage ?? song.stage };
            return { ...song, progress: 0, stage: 'stageWaitingInQueue' };
          });
        });
      } catch {
        // Progress detail is a nicety; the job status poll remains the truth.
      }
    };

    void poll();
    const timer = window.setInterval(poll, 2000);
    return () => { cancelled = true; window.clearInterval(timer); };
  }, [activeJobCount]);

  const handleGenerate = async (params: AceCreateRequest & { _tempId?: string }) => {
    const tempId = params._tempId || `temp_${Date.now()}_${Math.random().toString(36).slice(2, 11)}`;
    if (!params._tempId) {
      setSongs(prev => [{
        id: tempId,
        title: params.title?.trim() || t('generating') || 'Generating...',
        lyrics: params.lyrics || '',
        style: params.caption || '',
        coverUrl: '',
        duration: '--:--',
        createdAt: new Date(),
        isGenerating: true,
        stage: 'stageWaitingInQueue',
        tags: [STUDIO.slug],
      }, ...prev]);
    } else {
      setSongs(prev => prev.map(song => song.id === tempId
        ? { ...song, title: params.title?.trim() || song.title, style: params.caption || song.style, lyrics: params.lyrics || song.lyrics }
        : song));
    }

    setIsGenerating(true);
    try {
      const { _tempId, ...request } = params;
      const response = await fetch('/v1/music/jobs', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(request),
      });
      const job: AceJob & { error?: string; message?: string } = await response.json().catch(() => ({}) as AceJob);
      if (!response.ok || job.status === 'failed') {
        throw new Error(job.message || job.error || `The engine rejected this request (${response.status})`);
      }
      setSongs(prev => prev.map(song => song.id === tempId ? { ...song, jobId: job.id } : song));
      beginPollingJob(job.id, tempId);
      decrementPendingClicks(1);
    } catch (error) {
      console.error('Generation error:', error);
      setSongs(prev => prev.filter(song => song.id !== tempId));
      decrementPendingClicks(1);
      if (activeJobsRef.current.size === 0) setIsGenerating(false);
      showToast(error instanceof Error ? error.message : t('generationFailed'), 'error');
    }
  };


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
    const available = songs.filter(s => s.audioUrl && !s.isGenerating);
    if (available.length > 0) {
      playSong(available[0], available);
    }
  };

  const playSong = (song: Song, list?: Song[]) => {
    const nextQueue = list && list.length > 0
      ? list
      : (playQueue.length > 0 && playQueue.some(s => s.id === song.id))
          ? playQueue
          : (songs.some(s => s.id === song.id) ? songs : [song]);
    const nextIndex = nextQueue.findIndex(s => s.id === song.id);
    setPlayQueue(nextQueue);
    setQueueIndex(nextIndex);

    if (currentSong?.id !== song.id) {
      const updatedSong = { ...song, viewCount: (song.viewCount || 0) + 1 };
      setCurrentSong(updatedSong);
      setSelectedSong(updatedSong);
      setIsPlaying(true);
      setSongs(prev => prev.map(s => s.id === song.id ? updatedSong : s));
    } else {
      togglePlay();
    }
    if (currentSong?.id === song.id) {
      setSelectedSong(song);
    }
    setShowRightSidebar(true);
  };

  // An agent connected over MCP works this window like a user
  const VIEWS: View[] = ['create', 'library', 'tools', 'adapters', 'playlist', 'search', 'news'];
  const songById = (id: unknown) => {
    const song = songs.find(entry => entry.id === id);
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
  useBridgeCommand('player_state', () => ({
    song: currentSong ? { id: currentSong.id, title: currentSong.title } : null,
    playing: isPlaying,
    position_seconds: Math.round(currentTime * 10) / 10,
    duration_seconds: Math.round(duration * 10) / 10,
    volume,
    shuffle: isShuffle,
    repeat: repeatMode,
    queue: playQueue.length,
  }));
  useBridgeCommand('player_play', ({ song_id, stem }) => {
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
    setIsPlaying(false);
    return { text: 'Paused.' };
  });
  useBridgeCommand('player_seek', ({ seconds }) => {
    handleSeek(Number(seconds) || 0);
    return { text: `At ${Number(seconds) || 0} s.` };
  });
  useBridgeCommand('player_next', () => {
    playNext();
    return { text: 'Next song.' };
  });
  useBridgeCommand('player_previous', () => {
    playPrevious();
    return { text: 'Previous song.' };
  });
  useBridgeCommand('player_set', ({ volume: level, shuffle, repeat }) => {
    if (level !== undefined) setVolume(Math.max(0, Math.min(1, Number(level))));
    if (shuffle !== undefined) setIsShuffle(Boolean(shuffle));
    if (repeat === 'none' || repeat === 'all' || repeat === 'one') setRepeatMode(repeat);
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

  /// Favourites are a local library flag: the desktop studio has no social
  /// service, so the star is persisted next to the library instead of being
  /// posted to a server that does not exist.
  const toggleLike = (songId: string) => {
    const isLiked = likedSongIds.has(songId);
    setLikedSongIds(prev => {
      const next = new Set(prev);
      if (isLiked) next.delete(songId);
      else next.add(songId);
      saveNativeLikedSongIds(next as Set<string>);
      return next;
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
      ? t('deleteSongConfirm').replace('{title}', songsToDelete[0].title)
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
          setSongs(prev => prev.filter(s => !idsToDelete.has(s.id) || failed.includes(s.id)));

          setLikedSongIds(prev => {
            const next = new Set(prev);
            succeeded.forEach(id => next.delete(id));
            return next;
          });

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
      setPlaylists(prev => [playlist, ...prev]);
      if (songToAddToPlaylist) setSongToAddToPlaylist(null);
      showToast(t('playlistCreated'));
    } catch (error) {
      console.error('Create playlist error:', error);
      showToast(t('failedToCreatePlaylist'), 'error');
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
      setPlaylists(prev => prev.map(item => item.id === updated.id ? updated : item));
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
    setSongs(prev => prev.map(s => s.id === songId ? { ...s, coverUrl: bust } : s));
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

  const renderContent = (view: typeof currentView = currentView) => {
    switch (view) {
      case 'tools':
        return <StudioToolsPanel initialSongId={stemsSongId} />;

      case 'library': {
        const allSongs = songs;
        return (
          <LibraryView
            allSongs={allSongs}
            likedSongs={songs.filter(s => likedSongIds.has(s.id))}
            playlists={playlists}
            onPlaySong={playSong}
            onCreatePlaylist={() => {
              setSongToAddToPlaylist(null);
              setIsCreatePlaylistModalOpen(true);
            }}
            onSelectPlaylist={(p) => handleNavigateToPlaylist(p.id)}
            onImported={() => { void refreshNativeLibrary(); }}
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
          />
        );

      case 'search':
        return (
          <SearchPage
            songs={songs}
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
                md:block min-h-0 min-w-0 flex-shrink-0 h-full bg-zinc-50 dark:bg-suno-panel relative z-10 transition-colors duration-300
              `}
              style={{ width: window.innerWidth >= 768 ? leftPanel.width : undefined }}
            >
              <CreatePanel
                onGenerate={handleGenerate}
                isGenerating={isGenerating}
                activeJobCount={activeJobCount + pendingClickCount}
                initialData={reuseData}
              />
            </div>
            {leftPanel.handle}

            {/* Song List */}
            <div className={`
              ${!mobileShowList ? 'hidden md:flex' : 'flex'}
              min-h-0 min-w-0 flex-1 flex-col h-full overflow-hidden bg-white dark:bg-suno transition-colors duration-300
            `}>
              <SongList
                songs={songs}
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
                onCancelJob={cancelGeneration}
                onResetJob={resetSingleJob}
                onCancelAll={cancelAllGenerations}
                onResetAll={resetGeneration}
                activeJobCount={activeJobCount}
              />
            </div>

            {/* Right Sidebar */}
            {showRightSidebar && selectedSong && (
              <>
              {rightPanel.handle}
              <div
                className="hidden xl:block min-h-0 min-w-0 flex-shrink-0 h-full bg-zinc-50 dark:bg-suno-panel relative z-10 transition-colors duration-300"
                style={{ width: rightPanel.width }}
              >
                <RightSidebar
                  song={selectedSong}
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

  return (
    <SongActionsProvider value={songActions}>
    <div className="flex h-[100dvh] min-h-0 min-w-0 flex-col overflow-hidden bg-white dark:bg-suno text-zinc-900 dark:text-white font-sans antialiased selection:bg-pink-500/30 transition-colors duration-300">
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
        onToggleRepeat={() => setRepeatMode(prev => prev === 'none' ? 'all' : prev === 'all' ? 'one' : 'none')}
        isLiked={currentSong ? likedSongIds.has(currentSong.id) : false}
        onToggleLike={() => currentSong && toggleLike(currentSong.id)}
        onPlayFirst={playFirst}
      />}

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
          onKept={() => { void refreshNativeLibrary(); }}
        />
      )}
      {songForReplay && (
        <ReplayModal
          song={songForReplay}
          onClose={() => setSongForReplay(null)}
          onQueued={(jobId) => {
            // A re-render is a generation like any other: it gets its own card
            // with the engine's stages, and lands in the library the same way.
            const tempId = `replay_${jobId}`;
            setSongs(prev => [{
              id: tempId,
              title: songForReplay.title,
              lyrics: songForReplay.lyrics,
              style: songForReplay.style,
              coverUrl: '',
              duration: '--:--',
              createdAt: new Date(),
              isGenerating: true,
              jobId,
              stage: 'stageWaitingInQueue',
              tags: ['ace-step'],
            }, ...prev]);
            setIsGenerating(true);
            beginPollingJob(jobId, tempId);
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
      {showMobileDetails && selectedSong && (
        <div className="fixed inset-0 z-[60] flex justify-end xl:hidden">
          <div
            className="absolute inset-0 bg-black/60 backdrop-blur-sm animate-in fade-in"
            onClick={() => setShowMobileDetails(false)}
          />
          <div className="relative w-full max-w-md h-full bg-zinc-50 dark:bg-suno-panel shadow-2xl animate-in slide-in-from-right duration-300 border-l border-white/10">
            <RightSidebar
              song={selectedSong}
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
    <I18nProvider>
      <AppContent />
    </I18nProvider>
  );
}

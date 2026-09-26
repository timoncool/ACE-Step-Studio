import React, { useEffect, useRef, useState } from 'react';
import type WebampLazy from 'webamp/lazy';
import { getCurrentWindow, LogicalSize, PhysicalPosition, PhysicalSize } from '@tauri-apps/api/window';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { useI18n } from '../../context/I18nContext';
import type { Song } from '../../types';
import { TRACK_ARTIST } from '../../services/studio';
import { isDesktop } from '../../services/externalLinks';
import { EQ_BANDS, equalizer, setEqualizer } from '../../services/audioGraph';
import { addWinampSkin, pickSkin, registerWinampControl, setWinampSettings, winampSettings, winampSkins, type WinampSkin } from '../../services/winamp';
import { loadMilkdropPresets } from './VisualizerView';

/**
 * The Winamp mode: the whole studio window turns into a Winamp 2 player
 * (Webamp) with its main window, equalizer, playlist and MilkDrop, wearing
 * .wsz skins. The window loses its frame and takes the size of the player;
 * dragging a title bar moves the window, as dragging Winamp moved Winamp.
 * The studio underneath keeps running, and leaving the mode hands the song,
 * the position, the volume and the equalizer back to the studio's player.
 */

export interface WinampExit {
  songId: string | null;
  seconds: number;
  playing: boolean;
  volume: number;
}

interface Props {
  queue: Song[];
  startIndex: number;
  startSeconds: number;
  playing: boolean;
  volume: number;
  library: Song[];
  onSavePlaylist: (songIds: string[]) => Promise<void>;
  onExit: (exit: WinampExit) => void;
}

/** Webamp's track and layout, as its constructor takes them. */
type Track = { url: string; metaData?: { artist: string; title: string; albumArtUrl?: string }; duration?: number } | { blob: Blob; defaultName?: string };
type Position = { top: number; left: number };
type WindowLayout = Record<'main' | 'equalizer' | 'playlist' | 'milkdrop', { position: Position; size?: { extraHeight: number; extraWidth: number }; closed?: boolean }>;

type Store = { getState: () => WebampState; dispatch: (action: Record<string, unknown>) => void };
interface WebampState {
  equalizer: { on: boolean; sliders: Record<string, number> };
  media: { volume: number; balance: number; timeElapsed: number; shuffle: boolean; repeat: boolean; status: string };
  display: { doubled: boolean };
  windows: { genWindows: Record<string, { open: boolean; shade?: boolean; position: { x: number; y: number } }> };
  milkdrop: { presets: ({ name: string; type: 'RESOLVED' } | { name: string; type: 'UNRESOLVED'; getPreset: () => Promise<object> })[]; currentPresetIndex: number | null; randomize: boolean; cycling: boolean };
  playlist: { trackOrder: number[]; currentTrack: number | null };
  tracks: Record<string, { url: string; title?: string; defaultName: string | null; duration: number | null }>;
}

const WINDOWS = ['main', 'equalizer', 'playlist', 'milkdrop'] as const;
/** The studio window's smallest size, as tauri.conf.json sets it. */
const STUDIO_MIN = { width: 1024, height: 720 };
const SELECTOR = '#main-window, #equalizer-window, #playlist-window, #playlist-window-shade, .gen-window';

/** Winamp's sliders are 0..100 with 50 flat; the studio's are decibels. */
const toSlider = (db: number) => Math.round(50 + (db / 12) * 50);
const fromSlider = (value: number) => Math.round(((value - 50) / 50) * 12 * 10) / 10;

function track(song: Song): Track {
  return {
    url: song.audioUrl as string,
    metaData: { artist: song.creator || TRACK_ARTIST, title: song.title, albumArtUrl: song.coverUrl || undefined },
    duration: Number(song.duration) || undefined,
  } as Track;
}

async function milkdropPresets() {
  return (await loadMilkdropPresets()).map(([name, preset]) => ({ name, butterchurnPresetObject: preset }));
}

type SavedWindow = { position: PhysicalPosition; size: PhysicalSize; maximized: boolean };

/** Where the window's shape before the mode waits, until the mode is left: a reload inside the mode finds it. */
const SAVED_WINDOW = 'studio:winamp-window';

/** The studio's window as it was before the mode: frame, size, place, maximised, page zoom. */
async function restoreWindow(win: ReturnType<typeof getCurrentWindow>, saved: SavedWindow) {
  const steps: [string, () => Promise<void>][] = [
    ['always on top', () => win.setAlwaysOnTop(false)],
    ['frame', () => win.setDecorations(true)],
    ['resizable', () => win.setResizable(true)],
    ['minimum size', () => win.setMinSize(new LogicalSize(STUDIO_MIN.width, STUDIO_MIN.height))],
    ['size', () => win.setSize(saved.size)],
    ['position', () => win.setPosition(saved.position)],
    ['maximised', () => (saved.maximized ? win.maximize() : Promise.resolve())],
    ['page zoom', () => getCurrentWebview().setZoom(1)],
  ];
  for (const [what, step] of steps) {
    try {
      await step();
    } catch (error) {
      console.error(`Leaving Winamp: the window's ${what} did not come back:`, error);
    }
  }
  sessionStorage.removeItem(SAVED_WINDOW);
}

/** A page that reloaded inside the Winamp mode gives the window its shape back. */
export async function restoreWindowAfterReload(): Promise<void> {
  const text = sessionStorage.getItem(SAVED_WINDOW);
  if (!text || !isDesktop()) return;
  const saved = JSON.parse(text) as { x: number; y: number; width: number; height: number; maximized: boolean };
  await restoreWindow(getCurrentWindow(), {
    position: new PhysicalPosition(saved.x, saved.y),
    size: new PhysicalSize(saved.width, saved.height),
    maximized: saved.maximized,
  });
}

export const WinampMode: React.FC<Props> = ({ queue, startIndex, startSeconds, playing, volume, library, onSavePlaylist, onExit }) => {
  const { t } = useI18n();
  const host = useRef<HTMLDivElement>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [hint, setHint] = useState(true);
  const exitRef = useRef(onExit);
  exitRef.current = onExit;
  // the library grows while the mode is on: songs made meanwhile are played from it too
  const libraryRef = useRef(library);
  libraryRef.current = library;

  useEffect(() => {
    const node = host.current;
    if (!node) return undefined;
    const desktop = isDesktop();
    const win = desktop ? getCurrentWindow() : null;
    let webamp: WebampLazy | null = null;
    let disposed = false;
    let saved: SavedWindow | null = null;
    const cleanups: (() => void)[] = [];
    const songById = (id: unknown) => libraryRef.current.find((entry) => entry.id === id) ?? queue.find((entry) => entry.id === id);
    const idOf = (url: string) => (libraryRef.current.find((entry) => entry.audioUrl === url) ?? queue.find((entry) => entry.audioUrl === url))?.id ?? null;

    const store = () => (webamp as unknown as { store: Store }).store;
    // what Winamp's own preset list does: mark the request, fetch the preset if it is not in memory, show it with a blend
    const showPreset = async (index: number) => {
      const preset = store().getState().milkdrop.presets[index];
      if (!preset) throw new Error(`MilkDrop has no preset at ${index}.`);
      store().dispatch({ type: 'PRESET_REQUESTED', index, addToHistory: true });
      if (preset.type === 'UNRESOLVED') store().dispatch({ type: 'RESOLVE_PRESET_AT_INDEX', index, json: await preset.getPreset() });
      store().dispatch({ type: 'SELECT_PRESET_AT_INDEX', index, transitionType: 1 });
    };
    // every change to the OS window runs in turn: a fit never overtakes the mode's setup, and
    // leaving always comes last, so nothing lands on the window after it got its shape back
    let windowWork: Promise<void> = Promise.resolve();
    const onWindow = (job: () => Promise<void>) => {
      windowWork = windowWork.then(job).catch((error) => {
        console.error('Winamp window:', error);
        if (!disposed) setProblem(`${t('winampWindowFailed')}: ${error instanceof Error ? error.message : String(error)}`);
      });
      return windowWork;
    };
    // Webamp's own setCurrentTrack takes the index for a track id; this goes by the playlist's order
    const playAt = (index: number) => {
      const state = store().getState();
      const id = state.playlist.trackOrder[index];
      if (id === undefined) throw new Error(`Winamp's playlist has no track ${index + 1}.`);
      store().dispatch({ type: state.media.status === 'STOPPED' ? 'BUFFER_TRACK' : 'PLAY_TRACK', id });
    };
    let zoom = 1;
    const setZoom = async (next: number) => {
      zoom = next;
      if (desktop) await onWindow(() => getCurrentWebview().setZoom(next));
      fit();
    };

    // the window takes the size of the player's windows, which start at its corner; requests
    // that come while one waits are one fit, measured when it runs
    let fitWanted = false;
    const fit = () => {
      if (fitWanted) return;
      fitWanted = true;
      void onWindow(measureAndFit);
    };
    const measureAndFit = async () => {
      fitWanted = false;
      if (!webamp || disposed) return;
      const boxes = [...node.querySelectorAll<HTMLElement>(SELECTOR)].map((element) => element.getBoundingClientRect()).filter((box) => box.width > 0 && box.height > 0);
      if (!boxes.length) return;
      const left = Math.min(...boxes.map((box) => box.left));
      const top = Math.min(...boxes.map((box) => box.top));
      const width = Math.ceil(Math.max(...boxes.map((box) => box.right)) - left);
      const height = Math.ceil(Math.max(...boxes.map((box) => box.bottom)) - top);
      if (left !== 0 || top !== 0) {
        const windows = store().getState().windows.genWindows;
        const positions = Object.fromEntries(Object.entries(windows).map(([id, info]) => [id, { x: info.position.x - left, y: info.position.y - top }]));
        store().dispatch({ type: 'UPDATE_WINDOW_POSITIONS', positions, absolute: true });
        if (win) {
          const scale = (await win.scaleFactor()) * zoom;
          const at = await win.outerPosition();
          await win.setPosition(new PhysicalPosition(Math.round(at.x + left * scale), Math.round(at.y + top * scale)));
        }
      }
      // the page is zoomed: its pixels are that much larger on the screen
      if (win) await win.setSize(new LogicalSize(Math.ceil(width * zoom), Math.ceil(height * zoom)));
    };

    // a press on a title bar moves the whole window once the mouse moves
    const press = (event: MouseEvent) => {
      const target = event.target as HTMLElement | null;
      if (!win || event.button !== 0 || !target?.classList.contains('draggable')) return;
      event.stopPropagation();
      const start = { x: event.screenX, y: event.screenY };
      const move = (moved: MouseEvent) => {
        if (Math.abs(moved.screenX - start.x) + Math.abs(moved.screenY - start.y) < 4) return;
        release();
        void win.startDragging();
      };
      const release = () => {
        window.removeEventListener('mousemove', move, true);
        window.removeEventListener('mouseup', release, true);
      };
      window.addEventListener('mousemove', move, true);
      window.addEventListener('mouseup', release, true);
    };
    node.parentElement?.addEventListener('mousedown', press, true);
    cleanups.push(() => node.parentElement?.removeEventListener('mousedown', press, true));

    const exit = () => {
      if (!webamp) return;
      const state = store().getState();
      const current = state.playlist.currentTrack != null ? state.tracks[state.playlist.currentTrack] : null;
      const eqState = state.equalizer;
      setEqualizer({
        enabled: eqState.on,
        preamp: fromSlider(eqState.sliders.preamp),
        gains: EQ_BANDS.map((band) => fromSlider(eqState.sliders[String(band)])),
        preset: null,
        balance: state.media.balance / 100,
      });
      exitRef.current({
        songId: current ? idOf(current.url) : null,
        seconds: state.media.timeElapsed,
        playing: state.media.status === 'PLAYING',
        volume: state.media.volume / 100,
      });
    };

    const start = async () => {
      const [{ default: Webamp }, skins] = await Promise.all([import('webamp/lazy'), winampSkins()]);
      if (disposed) return;
      const settings = winampSettings();
      const skin = pickSkin(skins);
      const playable = queue.filter((song) => song.audioUrl);
      const at = playable.findIndex((song) => song.id === queue[startIndex]?.id);
      const layout: WindowLayout = {
        main: { position: { top: 0, left: 0 } },
        equalizer: { position: { top: 116, left: 0 } },
        playlist: { position: { top: 232, left: 0 }, size: { extraHeight: 4, extraWidth: 0 } },
        milkdrop: { position: { top: 0, left: 275 }, size: { extraHeight: 12, extraWidth: 7 }, closed: true },
      };
      const listAll = () => libraryRef.current.filter((song) => song.audioUrl && !song.isGenerating).map(track);
      webamp = new Webamp({
        initialTracks: playable.map(track),
        initialSkin: skin ? { url: skin.url } : undefined,
        availableSkins: skins.map(({ name, url }) => ({ name, url })),
        windowLayout: layout,
        enableDoubleSizeMode: settings.doubleSize,
        enableHotkeys: true,
        zIndex: 1000,
        // the player's own requirements, from the studio's packages
        requireJSZip: async () => {
          const JSZip = (await import('jszip')).default;
          // a skin the user loads or drops is kept in the studio's skins folder
          return {
            loadAsync: (data: unknown, options?: unknown) => {
              if (data instanceof File && /\.(wsz|zip)$/i.test(data.name)) {
                void addWinampSkin(data, data.name.replace(/\.(wsz|zip)$/i, '')).then(async (added) => {
                  setWinampSettings({ skin: added.name });
                  store().dispatch({ type: 'SET_AVAILABLE_SKINS', skins: (await winampSkins()).map(({ name, url }) => ({ name, url })) });
                }).catch((error) => setProblem(`${t('winampSkinNotKept')}: ${error instanceof Error ? error.message : String(error)}`));
              }
              return JSZip.loadAsync(data as Blob, options as never);
            },
          } as never;
        },
        requireMusicMetadata: async () => (await import('music-metadata')) as never,
        __butterchurnOptions: {
          importButterchurn: () => import('butterchurn'),
          getPresets: milkdropPresets,
          butterchurnOpen: false,
        },
        // ADD URL: the studio's library; LOAD LIST: the whole library; SAVE LIST: a studio playlist
        handleAddUrlEvent: listAll,
        handleLoadListEvent: listAll,
        handleSaveListEvent: async (tracks: Track[]) => {
          const ids = tracks.map((entry) => ('url' in entry ? idOf(entry.url) : null)).filter((id): id is string => Boolean(id));
          await onSavePlaylist(ids);
          return null;
        },
        filePickers: [{ contextMenuName: t('winampStudioLibrary'), filePicker: async () => listAll(), requiresNetwork: false }],
      } as never);
      if (startSeconds > 0) {
        // the position is set once the studio's song plays in Winamp and its length is known:
        // Winamp seeks by a share of the length, and a seek before the file plays is dropped
        const wanted = store().getState().playlist.trackOrder[Math.max(0, at)];
        let stopWaiting: (() => void) | null = null;
        const seekWhenPlaying = () => {
          const state = store().getState();
          const length = state.tracks[String(wanted)]?.duration;
          if (state.playlist.currentTrack !== wanted || state.media.timeElapsed <= 0 || !length) return;
          stopWaiting?.();
          stopWaiting = null;
          store().dispatch({ type: 'SEEK_TO_PERCENT_COMPLETE', percent: Math.min(100, (startSeconds / length) * 100) });
        };
        stopWaiting = (webamp as WebampLazy).__onStateChange(seekWhenPlaying);
        cleanups.push(() => stopWaiting?.());
      }

      if (win) {
        await onWindow(async () => {
          if (disposed) return;
          saved = { position: await win.outerPosition(), size: await win.innerSize(), maximized: await win.isMaximized() };
          sessionStorage.setItem(SAVED_WINDOW, JSON.stringify({ x: saved.position.x, y: saved.position.y, width: saved.size.width, height: saved.size.height, maximized: saved.maximized }));
          if (saved.maximized) await win.unmaximize();
          await win.setMinSize(null);
          await win.setDecorations(false);
          await win.setResizable(false);
          await win.setAlwaysOnTop(settings.alwaysOnTop);
        });
      }
      if (disposed) return;
      await webamp.renderInto(node);
      if (disposed) return;
      await setZoom(settings.scale);

      // the studio's equalizer, volume and balance carry over
      const eq = equalizer();
      store().dispatch({ type: eq.enabled ? 'SET_EQ_ON' : 'SET_EQ_OFF' });
      store().dispatch({ type: 'SET_BAND_VALUE', band: 'preamp', value: toSlider(eq.preamp) });
      EQ_BANDS.forEach((band, index) => store().dispatch({ type: 'SET_BAND_VALUE', band, value: toSlider(eq.gains[index] ?? 0) }));
      store().dispatch({ type: 'SET_BALANCE', balance: Math.round(eq.balance * 100) });
      webamp.setVolume(Math.round(volume * 100));
      if (at > 0) playAt(at);
      if (playing) webamp.play();

      cleanups.push(webamp.onClose(exit));
      cleanups.push(webamp.onMinimize(() => void win?.minimize()));
      cleanups.push(webamp.__onStateChange(fit));
      const observer = new ResizeObserver(fit);
      node.querySelectorAll(SELECTOR).forEach((element) => observer.observe(element));
      const watch = new MutationObserver(() => {
        node.querySelectorAll(SELECTOR).forEach((element) => observer.observe(element));
        fit();
      });
      watch.observe(node, { childList: true, subtree: true });
      cleanups.push(() => {
        observer.disconnect();
        watch.disconnect();
      });
      fit();
      const hideHint = window.setTimeout(() => setHint(false), 6000);
      cleanups.push(() => window.clearTimeout(hideHint));

      registerWinampControl({
        state: () => {
          const state = store().getState();
          const current = state.playlist.currentTrack != null ? state.tracks[state.playlist.currentTrack] : null;
          return {
            on: true,
            skin: winampSettings().skin,
            random_skin: winampSettings().random,
            double_size: state.display.doubled,
            always_on_top: winampSettings().alwaysOnTop,
            scale: winampSettings().scale,
            skip_menu: winampSettings().skipMenu,
            windows: Object.fromEntries(WINDOWS.map((id) => [id, { open: Boolean(state.windows.genWindows[id]?.open), shade: Boolean(state.windows.genWindows[id]?.shade) }])),
            playing: state.media.status === 'PLAYING',
            status: state.media.status,
            song: current ? { id: idOf(current.url), title: current.title ?? current.defaultName } : null,
            position_seconds: Math.round(state.media.timeElapsed * 10) / 10,
            volume: state.media.volume / 100,
            balance: state.media.balance / 100,
            shuffle: state.media.shuffle,
            repeat: state.media.repeat,
            playlist: state.playlist.trackOrder.length,
            equalizer: {
              on: state.equalizer.on,
              preamp: fromSlider(state.equalizer.sliders.preamp),
              bands: EQ_BANDS.map((band) => fromSlider(state.equalizer.sliders[String(band)])),
            },
            milkdrop: {
              preset: state.milkdrop.currentPresetIndex != null ? state.milkdrop.presets[state.milkdrop.currentPresetIndex]?.name ?? null : null,
              random: state.milkdrop.randomize,
              cycling: state.milkdrop.cycling,
            },
          };
        },
        set: async (change) => {
          const player = webamp as WebampLazy;
          const state = store().getState();
          const done: string[] = [];
          if (typeof change.skin === 'string') {
            const all = await winampSkins();
            const chosen = all.find((entry) => entry.name.toLowerCase() === String(change.skin).toLowerCase());
            if (!chosen) throw new Error(`No skin ${String(change.skin)}; winamp_skins lists them.`);
            setWinampSettings({ skin: chosen.name, random: false });
            player.setSkinFromUrl(chosen.url);
            done.push(`skin ${chosen.name}`);
          }
          if (typeof change.random_skin === 'boolean') setWinampSettings({ random: change.random_skin });
          if (typeof change.scale === 'number') {
            setWinampSettings({ scale: change.scale });
            await setZoom(winampSettings().scale);
          }
          if (typeof change.skip_menu === 'boolean') setWinampSettings({ skipMenu: change.skip_menu });
          if (typeof change.double_size === 'boolean' && change.double_size !== state.display.doubled) {
            store().dispatch({ type: 'TOGGLE_DOUBLESIZE_MODE' });
            setWinampSettings({ doubleSize: change.double_size });
          }
          if (typeof change.always_on_top === 'boolean') {
            setWinampSettings({ alwaysOnTop: change.always_on_top });
            await win?.setAlwaysOnTop(change.always_on_top);
          }
          const windows = change.windows as Record<string, { open?: boolean; shade?: boolean }> | undefined;
          for (const id of WINDOWS) {
            const wanted = windows?.[id];
            const info = state.windows.genWindows[id];
            if (!wanted || !info) continue;
            if (typeof wanted.open === 'boolean' && wanted.open !== info.open) store().dispatch({ type: 'TOGGLE_WINDOW', windowId: id });
            if (typeof wanted.shade === 'boolean' && wanted.shade !== Boolean(info.shade) && id !== 'milkdrop') store().dispatch({ type: 'TOGGLE_WINDOW_SHADE_MODE', windowId: id });
          }
          if (Array.isArray(change.song_ids)) {
            const list = change.song_ids.map(songById).filter((song): song is Song => Boolean(song?.audioUrl));
            if (!list.length) throw new Error('None of those songs has audio to play.');
            player.setTracksToPlay(list.map(track) as never);
            done.push(`playing ${list.length} song(s)`);
          }
          if (typeof change.song_id === 'string') {
            const song = songById(change.song_id);
            if (!song?.audioUrl) throw new Error(`No playable song ${String(change.song_id)}; library_songs_list lists them.`);
            const order = store().getState().playlist.trackOrder;
            const tracks = store().getState().tracks;
            let at = order.findIndex((id) => tracks[id]?.url === song.audioUrl);
            if (at < 0) {
              player.appendTracks([track(song)]);
              at = store().getState().playlist.trackOrder.length - 1;
            }
            playAt(at);
            player.play();
            done.push(`playing ${song.title}`);
          }
          if (change.action === 'play') player.play();
          if (change.action === 'pause') player.pause();
          if (change.action === 'stop') player.stop();
          if (change.action === 'next') player.nextTrack();
          if (change.action === 'previous') player.previousTrack();
          if (typeof change.seek_seconds === 'number') player.seekToTime(Math.max(0, change.seek_seconds));
          if (typeof change.volume === 'number') player.setVolume(Math.round(Math.max(0, Math.min(1, change.volume)) * 100));
          if (typeof change.balance === 'number') store().dispatch({ type: 'SET_BALANCE', balance: Math.round(Math.max(-1, Math.min(1, change.balance)) * 100) });
          if (typeof change.shuffle === 'boolean' && change.shuffle !== player.isShuffleEnabled()) player.toggleShuffle();
          if (typeof change.repeat === 'boolean' && change.repeat !== player.isRepeatEnabled()) player.toggleRepeat();
          const eqChange = change.equalizer as { on?: boolean; preamp?: number; bands?: number[] } | undefined;
          if (eqChange) {
            if (typeof eqChange.on === 'boolean') store().dispatch({ type: eqChange.on ? 'SET_EQ_ON' : 'SET_EQ_OFF' });
            if (typeof eqChange.preamp === 'number') store().dispatch({ type: 'SET_BAND_VALUE', band: 'preamp', value: toSlider(eqChange.preamp) });
            eqChange.bands?.forEach((db, index) => {
              if (typeof db === 'number' && EQ_BANDS[index]) store().dispatch({ type: 'SET_BAND_VALUE', band: EQ_BANDS[index], value: toSlider(Math.max(-12, Math.min(12, db))) });
            });
          }
          const milkdrop = change.milkdrop as { preset?: string; next?: boolean; random?: boolean; cycling?: boolean } | undefined;
          if (milkdrop) {
            // right after the mode opens its presets are still on their way
            for (let waited = 0; waited < 15000 && !store().getState().milkdrop.presets.length; waited += 200) await new Promise((resolve) => setTimeout(resolve, 200));
            const presets = store().getState().milkdrop;
            if (!presets.presets.length) throw new Error('MilkDrop has no presets loaded yet; ui_console shows why.');
            if (typeof milkdrop.preset === 'string') {
              const index = presets.presets.findIndex((entry) => entry.name.toLowerCase() === milkdrop.preset!.toLowerCase());
              if (index < 0) throw new Error(`No MilkDrop preset ${milkdrop.preset}; visualizer_presets lists them.`);
              await showPreset(index);
            }
            if (milkdrop.next) {
              const count = presets.presets.length;
              const current = presets.currentPresetIndex ?? -1;
              await showPreset(presets.randomize ? Math.floor(Math.random() * count) : (current + 1) % count);
            }
            if (typeof milkdrop.random === 'boolean' && milkdrop.random !== presets.randomize) store().dispatch({ type: 'TOGGLE_RANDOMIZE_PRESETS' });
            if (typeof milkdrop.cycling === 'boolean' && milkdrop.cycling !== presets.cycling) store().dispatch({ type: 'TOGGLE_PRESET_CYCLING' });
          }
          return done.length ? `Set: ${done.join(', ')}.` : 'Set.';
        },
        leave: exit,
      });
      cleanups.push(() => registerWinampControl(null));
    };

    start().catch((error) => setProblem(error instanceof Error ? error.message : String(error)));

    return () => {
      disposed = true;
      // queued after the setup and any fit, so the window's shape back is the last thing it gets
      if (win) void onWindow(async () => {
        if (saved) await restoreWindow(win, saved);
      });
      cleanups.forEach((cleanup) => cleanup());
      try {
        webamp?.pause();
        webamp?.dispose();
      } catch (error) {
        console.error('Winamp did not shut down cleanly:', error);
      }
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className="fixed inset-0 z-[300] overflow-hidden bg-black">
      <div ref={host} className="absolute inset-0" />
      {hint && !problem && (
        <button type="button" onClick={() => setHint(false)} className="absolute bottom-1 left-1 right-1 z-[2000] rounded bg-black/85 px-2 py-1.5 text-left text-[11px] leading-snug text-white">
          {t('winampHowToLeave')}
        </button>
      )}
      {problem && (
        <div className="absolute bottom-2 left-2 right-2 z-[2000] rounded-md bg-rose-900/90 px-3 py-2 text-xs text-white">
          {problem}
          <button type="button" onClick={() => onExit({ songId: null, seconds: 0, playing: false, volume })} className="ml-3 underline">
            {t('winampLeave')}
          </button>
        </div>
      )}
    </div>
  );
};

export type { WinampSkin };

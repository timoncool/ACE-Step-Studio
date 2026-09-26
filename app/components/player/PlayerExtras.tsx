import React, { useEffect, useState } from 'react';
import { useBridgeCommand } from '../../services/mcpBridge';
import { EQ_BANDS, EQ_RANGE_DB, equalizer, setEqualizer } from '../../services/audioGraph';
import { SITUATION_PRESETS, WINAMP_PRESETS, ownPresets, readEqf, storeOwnPresets, writeEqf, type EqPreset } from '../../services/equalizerPresets';
import { equalizerPanelOpen, onEqualizerPanel, setEqualizerPanelOpen } from '../../services/playerPanels';
import { onVisualizer, setVisualizer, visualizer, visualizerCommand, type VisualizerState } from '../../services/visualizerState';
import { onWinamp, setWinampOn, setWinampSettings, winampControl, winampOn, winampSettings, winampSkins, SKIN_MUSEUM } from '../../services/winamp';
import { openExternal } from '../../services/externalLinks';
import type { Song } from '../../types';
import { EqualizerPanel } from './EqualizerPanel';
import { VisualizerPanel, openVisualizerWindow, setVisualizerPanelSize, visualizerPanelSize } from './VisualizerPanel';
import { SPECTRUM_LOOK_IDS, milkdropPresetNames } from './VisualizerView';
import { WinampMode, restoreWindowAfterReload, type WinampExit } from './WinampMode';
import { placePanel } from '../../services/useFloatable';

/**
 * The player's extras over the studio - the equalizer panel, the visualiser
 * and the Winamp mode - and the agent's commands for all three: everything a
 * click does here, a tool does too.
 */

interface Props {
  queue: Song[];
  currentSong: Song | null;
  currentTime: number;
  isPlaying: boolean;
  volume: number;
  library: Song[];
  /** Pauses the studio's player while Winamp plays. */
  onPause: () => void;
  onLeaveWinamp: (exit: WinampExit) => void;
  onSavePlaylist: (songIds: string[]) => Promise<void>;
}

const allPresets = (): EqPreset[] => [...ownPresets(), ...WINAMP_PRESETS, ...SITUATION_PRESETS];
const clampDb = (value: unknown) => Math.max(-EQ_RANGE_DB, Math.min(EQ_RANGE_DB, Number(value) || 0));

function equalizerReport() {
  const eq = equalizer();
  return {
    enabled: eq.enabled,
    preamp_db: eq.preamp,
    bands: EQ_BANDS.map((hz, index) => ({ hz, db: eq.gains[index] ?? 0 })),
    preset: eq.preset,
    balance: eq.balance,
    mono: eq.mono,
    panel_open: equalizerPanelOpen(),
    presets: {
      winamp: WINAMP_PRESETS.map((preset) => preset.name),
      situations: SITUATION_PRESETS.map((preset) => preset.name),
      own: ownPresets().map((preset) => preset.name),
    },
    range_db: EQ_RANGE_DB,
  };
}

function base64(bytes: ArrayBuffer): string {
  let text = '';
  new Uint8Array(bytes).forEach((byte) => {
    text += String.fromCharCode(byte);
  });
  return btoa(text);
}

export const PlayerExtras: React.FC<Props> = ({ queue, currentSong, currentTime, isPlaying, volume, library, onPause, onLeaveWinamp, onSavePlaylist }) => {
  const [eqOpen, setEqOpen] = useState(equalizerPanelOpen);
  const [view, setView] = useState<VisualizerState>(visualizer);
  const [winamp, setWinamp] = useState(winampOn);
  const [start, setStart] = useState<{ queue: Song[]; index: number; seconds: number; playing: boolean; volume: number } | null>(null);

  useEffect(() => onEqualizerPanel(setEqOpen), []);
  useEffect(() => onVisualizer(setView), []);
  useEffect(() => onWinamp(() => setWinamp(winampOn())), []);
  useEffect(() => {
    restoreWindowAfterReload().catch((error) => console.error('The window did not get its shape back after the Winamp mode:', error));
  }, []);

  // Ctrl+M switches the Winamp mode on and off, as in Spotifast
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (!event.ctrlKey || event.altKey || event.key.toLowerCase() !== 'm') return;
      event.preventDefault();
      if (winampControl()) winampControl()!.leave();
      else if (!winampOn()) setWinampOn(true);
    };
    window.addEventListener('keydown', key, true);
    return () => window.removeEventListener('keydown', key, true);
  }, []);

  // the mode starts from the studio's player: its queue, song, position and volume
  useEffect(() => {
    if (!winamp) {
      setStart(null);
      return;
    }
    const list = queue.length ? queue : library;
    const index = currentSong ? Math.max(0, list.findIndex((song) => song.id === currentSong.id)) : 0;
    setStart({ queue: list, index, seconds: currentTime, playing: isPlaying, volume });
    onPause();
  }, [winamp]); // eslint-disable-line react-hooks/exhaustive-deps

  // ---------------------------------------------------------------- the equalizer
  useBridgeCommand('equalizer_get', () => equalizerReport());
  useBridgeCommand('equalizer_set', (args) => {
    const change: Parameters<typeof setEqualizer>[0] = {};
    if (typeof args.preset === 'string') {
      const preset = allPresets().find((entry) => entry.name.toLowerCase() === String(args.preset).toLowerCase());
      if (!preset) throw new Error(`No preset ${String(args.preset)}; equalizer_get lists them.`);
      Object.assign(change, { preamp: preset.preamp, gains: [...preset.gains], preset: preset.name, enabled: true });
    }
    if (args.bands !== undefined) {
      const gains = [...(change.gains ?? equalizer().gains)];
      if (Array.isArray(args.bands)) {
        if (args.bands.length !== EQ_BANDS.length) throw new Error(`bands takes ${EQ_BANDS.length} values in dB, 60 Hz to 16 kHz.`);
        args.bands.forEach((db: unknown, index: number) => { gains[index] = clampDb(db); });
      } else if (typeof args.bands === 'object' && args.bands) {
        Object.entries(args.bands as Record<string, unknown>).forEach(([hz, db]) => {
          const index = EQ_BANDS.indexOf(Number(hz) as (typeof EQ_BANDS)[number]);
          if (index < 0) throw new Error(`No band at ${hz} Hz; the bands are ${EQ_BANDS.join(', ')}.`);
          gains[index] = clampDb(db);
        });
      }
      Object.assign(change, { gains, preset: null });
    }
    if (args.preamp_db !== undefined) Object.assign(change, { preamp: clampDb(args.preamp_db), preset: null });
    if (typeof args.enabled === 'boolean') change.enabled = args.enabled;
    if (args.balance !== undefined) change.balance = Math.max(-1, Math.min(1, Number(args.balance) || 0));
    if (typeof args.mono === 'boolean') change.mono = args.mono;
    setEqualizer(change);
    if (typeof args.panel_open === 'boolean') setEqualizerPanelOpen(args.panel_open);
    const eqPlace = args.panel_position as { x?: number; y?: number } | undefined;
    if (eqPlace) placePanel('equalizer', Number(eqPlace.x) || 0, Number(eqPlace.y) || 0);
    if (typeof args.save_preset === 'string' && args.save_preset.trim()) {
      const name = args.save_preset.trim();
      const eq = equalizer();
      storeOwnPresets([...ownPresets().filter((preset) => preset.name !== name), { name, preamp: eq.preamp, gains: [...eq.gains] }]);
      setEqualizer({ preset: name });
    }
    if (typeof args.delete_preset === 'string') {
      const before = ownPresets();
      const after = before.filter((preset) => preset.name !== args.delete_preset);
      if (after.length === before.length) throw new Error(`No preset of the user's named ${String(args.delete_preset)}.`);
      storeOwnPresets(after);
    }
    return equalizerReport();
  });
  useBridgeCommand('equalizer_import', ({ name, data }) => {
    const bytes = Uint8Array.from(atob(String(data)), (char) => char.charCodeAt(0));
    const presets = readEqf(bytes.buffer);
    if (!presets.length) throw new Error(`${String(name)} has no presets.`);
    const names = new Set(presets.map((preset) => preset.name));
    storeOwnPresets([...ownPresets().filter((preset) => !names.has(preset.name)), ...presets]);
    setEqualizer({ preamp: presets[0].preamp, gains: [...presets[0].gains], preset: presets[0].name, enabled: true });
    return { text: `Imported ${presets.length} preset(s) from ${String(name)}: ${presets.map((preset) => preset.name).join(', ')}. ${presets[0].name} is on.` };
  });
  useBridgeCommand('equalizer_export', async ({ name }) => {
    const eq = equalizer();
    const label = typeof name === 'string' && name.trim() ? name.trim() : eq.preset ?? 'Studio';
    return { name: label, eqf_base64: base64(await writeEqf([{ name: label, preamp: eq.preamp, gains: [...eq.gains] }]).arrayBuffer()) };
  });

  // ---------------------------------------------------------------- the visualiser
  const visualizerReport = () => ({ ...visualizer(), looks: SPECTRUM_LOOK_IDS, panel_size: visualizerPanelSize() });
  useBridgeCommand('visualizer_get', () => visualizerReport());
  useBridgeCommand('visualizer_set', async (args) => {
    const change: Partial<VisualizerState> = {};
    if (args.engine === 'milkdrop' || args.engine === 'spectrum') change.engine = args.engine;
    if (typeof args.preset === 'string') {
      const names = await milkdropPresetNames();
      const found = names.find((entry) => entry.toLowerCase() === String(args.preset).toLowerCase())
        ?? names.find((entry) => entry.toLowerCase().includes(String(args.preset).toLowerCase()));
      if (!found) throw new Error(`No MilkDrop preset like ${String(args.preset)}; visualizer_presets lists them.`);
      Object.assign(change, { preset: found, engine: 'milkdrop' });
    }
    if (typeof args.look === 'string') {
      if (!SPECTRUM_LOOK_IDS.includes(args.look)) throw new Error(`No look ${args.look}; the looks are ${SPECTRUM_LOOK_IDS.join(', ')}.`);
      Object.assign(change, { look: args.look, engine: 'spectrum' });
    }
    for (const key of ['locked', 'random', 'showName', 'fullscreen'] as const) {
      const value = args[key === 'showName' ? 'show_name' : key];
      if (typeof value === 'boolean') change[key] = value;
    }
    if (args.cycle_seconds !== undefined) change.cycleSeconds = Math.max(3, Number(args.cycle_seconds) || 15);
    const vizPlace = args.panel_position as { x?: number; y?: number } | undefined;
    if (vizPlace) placePanel('visualizer', Number(vizPlace.x) || 0, Number(vizPlace.y) || 0);
    if (args.panel_size && typeof args.panel_size === 'object') {
      const size = args.panel_size as { width?: number; height?: number };
      setVisualizerPanelSize({ width: Math.max(240, Number(size.width) || 480), height: Math.max(160, Number(size.height) || 300) });
    }
    if (args.place === 'window') {
      setVisualizer(change);
      await openVisualizerWindow();
    } else {
      if (args.place === 'panel' || args.place === 'closed') change.place = args.place;
      setVisualizer(change);
    }
    if (args.step === 'next' || args.step === 'previous') visualizerCommand(args.step);
    return visualizerReport();
  });
  useBridgeCommand('visualizer_presets', async ({ search }) => {
    const names = await milkdropPresetNames();
    const query = typeof search === 'string' ? search.toLowerCase() : '';
    const found = query ? names.filter((name) => name.toLowerCase().includes(query)) : names;
    return { count: found.length, of: names.length, presets: found };
  });

  // ---------------------------------------------------------------- the Winamp mode
  const winampReport = () => {
    if (winampControl()) return winampControl()!.state();
    const settings = winampSettings();
    return { on: false, skin: settings.skin, random_skin: settings.random, double_size: settings.doubleSize, always_on_top: settings.alwaysOnTop, scale: settings.scale, skip_menu: settings.skipMenu };
  };
  useBridgeCommand('winamp_get', () => winampReport());
  useBridgeCommand('winamp_set', async (args) => {
    if (args.on === false && winampControl()) {
      winampControl()!.leave();
      return winampReport();
    }
    if (typeof args.on === 'boolean' && args.on !== winampOn()) {
      setWinampOn(args.on);
      if (args.on) {
        // the player comes up, then takes the rest
        for (let waited = 0; waited < 8000 && !winampControl(); waited += 100) await new Promise((resolve) => setTimeout(resolve, 100));
        if (!winampControl()) throw new Error('The Winamp player did not come up; ui_console shows why.');
      }
    }
    const rest = { ...args };
    delete rest.on;
    if (winampControl() && Object.keys(rest).length) await winampControl()!.set(rest);
    else {
      if (typeof args.skin === 'string') {
        const skins = await winampSkins();
        const chosen = skins.find((skin) => skin.name.toLowerCase() === String(args.skin).toLowerCase());
        if (!chosen) throw new Error(`No skin ${String(args.skin)}; winamp_skins lists them.`);
        setWinampSettings({ skin: chosen.name, random: false });
      }
      if (typeof args.random_skin === 'boolean') setWinampSettings({ random: args.random_skin });
      if (typeof args.double_size === 'boolean') setWinampSettings({ doubleSize: args.double_size });
      if (typeof args.always_on_top === 'boolean') setWinampSettings({ alwaysOnTop: args.always_on_top });
      if (typeof args.scale === 'number') setWinampSettings({ scale: args.scale });
      if (typeof args.skip_menu === 'boolean') setWinampSettings({ skipMenu: args.skip_menu });
    }
    return winampReport();
  });
  useBridgeCommand('winamp_skins', async () => ({ chosen: winampSettings().skin, random: winampSettings().random, museum: SKIN_MUSEUM, skins: (await winampSkins()).map(({ name, builtIn }) => ({ name, built_in: builtIn })) }));
  useBridgeCommand('winamp_museum', async () => {
    await openExternal(SKIN_MUSEUM);
    return { text: `The Winamp Skin Museum is open in the browser: ${SKIN_MUSEUM}. A downloaded .wsz goes in with winamp_skin_add.` };
  });

  return (
    <>
      {eqOpen && !winamp && <EqualizerPanel onClose={() => setEqualizerPanelOpen(false)} />}
      {view.place === 'panel' && !winamp && <VisualizerPanel />}
      {winamp && start && (
        <WinampMode
          queue={start.queue}
          startIndex={start.index}
          startSeconds={start.seconds}
          playing={start.playing}
          volume={start.volume}
          library={library}
          onSavePlaylist={onSavePlaylist}
          onExit={(exit) => {
            setWinampOn(false);
            onLeaveWinamp(exit);
          }}
        />
      )}
    </>
  );
};

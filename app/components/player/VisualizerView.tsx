import React, { useCallback, useEffect, useRef, useState } from 'react';
import type { ButterchurnVisualizer } from 'butterchurn';
import type AudioMotionAnalyzer from 'audiomotion-analyzer';
import type { ConstructorOptions } from 'audiomotion-analyzer';
import { useI18n } from '../../context/I18nContext';
import { onVisualizer, onVisualizerCommand, setVisualizer, visualizer, type VisualizerEngine, type VisualizerState } from '../../services/visualizerState';

/**
 * One picture of the music, from a Web Audio source: MilkDrop through
 * Butterchurn, with its preset packs, or a spectrum analyser through
 * audioMotion-analyzer, in a handful of classic looks. The same view runs in
 * the panel over the studio, fullscreen and in a window of its own.
 *
 * Keys, as in spotifast: N / right - next, P / left - previous, L - keep this
 * one, R - random or in order, T - show the name, F - fullscreen, Esc - back.
 */

export type { VisualizerEngine };

const SPECTRUM_LOOKS: { id: string; options: ConstructorOptions }[] = [
  { id: 'winamp', options: { mode: 6, gradient: 'classic', barSpace: 0.2, showPeaks: true, ledBars: false, reflexRatio: 0 } },
  { id: 'led', options: { mode: 3, gradient: 'classic', ledBars: true, trueLeds: true, showPeaks: true, reflexRatio: 0 } },
  { id: 'hifi', options: { mode: 8, gradient: 'classic', ledBars: true, trueLeds: true, barSpace: 0.3, showScaleX: true, reflexRatio: 0 } },
  { id: 'rainbow', options: { mode: 5, gradient: 'rainbow', barSpace: 0.1, reflexRatio: 0.3, reflexAlpha: 0.25 } },
  { id: 'radial', options: { mode: 4, gradient: 'prism', radial: true, spinSpeed: 1, reflexRatio: 0 } },
  { id: 'mirror', options: { mode: 6, gradient: 'orangered', mirror: -1, reflexRatio: 0.5, reflexAlpha: 0.3 } },
  { id: 'wave', options: { mode: 10, gradient: 'steelblue', lineWidth: 2, fillAlpha: 0.3, channelLayout: 'dual-combined', reflexRatio: 0 } },
  { id: 'stereo', options: { mode: 5, gradient: 'prism', channelLayout: 'dual-vertical', barSpace: 0.15, reflexRatio: 0 } },
  { id: 'lumi', options: { mode: 4, gradient: 'rainbow', lumiBars: true, reflexRatio: 0 } },
  { id: 'discrete', options: { mode: 0, gradient: 'prism', frequencyScale: 'log', reflexRatio: 0 } },
];

const BASE: ConstructorOptions = {
  connectSpeakers: false,
  showBgColor: true,
  bgAlpha: 1,
  overlay: false,
  showScaleX: false,
  showScaleY: false,
  smoothing: 0.7,
  maxFPS: 60,
  minFreq: 30,
  maxFreq: 16000,
  radial: false,
  mirror: 0,
  ledBars: false,
  lumiBars: false,
  channelLayout: 'single',
  frequencyScale: 'log',
};

let milkdropPresets: Promise<[string, object][]> | null = null;

/** The four Butterchurn packs, loaded the first time MilkDrop is shown. */
export function loadMilkdropPresets(): Promise<[string, object][]> {
  milkdropPresets ??= Promise.all([
    import('butterchurn-presets'),
    import('butterchurn-presets/lib/butterchurnPresetsExtra.min.js'),
    import('butterchurn-presets/lib/butterchurnPresetsExtra2.min.js'),
    import('butterchurn-presets/lib/butterchurnPresetsMD1.min.js'),
  ]).then((packs) => {
    const all = new Map<string, object>();
    packs.forEach((pack) => Object.entries(pack.default.getPresets()).forEach(([name, preset]) => all.set(name, preset)));
    return [...all.entries()].sort(([a], [b]) => a.localeCompare(b));
  });
  return milkdropPresets;
}

const BLEND = 2.7;

/** The MilkDrop presets by name, for the agent and the lists. */
export async function milkdropPresetNames(): Promise<string[]> {
  return (await loadMilkdropPresets()).map(([name]) => name);
}

export const VisualizerView: React.FC<{
  context: AudioContext;
  source: AudioNode;
  onFullscreen?: () => void;
  onEscape?: () => void;
}> = ({ context, source, onFullscreen, onEscape }) => {
  const { t } = useI18n();
  const box = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const milkdrop = useRef<ButterchurnVisualizer | null>(null);
  const analyser = useRef<AudioMotionAnalyzer | null>(null);
  const [view, setView] = useState<VisualizerState>(visualizer);
  const [presets, setPresets] = useState<[string, object][]>([]);
  const [ready, setReady] = useState(false);
  const [flash, setFlash] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const { engine, preset, look, locked, random, showName, cycleSeconds } = view;

  useEffect(() => onVisualizer(setView), []);

  const lookIndex = Math.max(0, SPECTRUM_LOOKS.findIndex((entry) => entry.id === look));
  const name = engine === 'milkdrop' ? preset ?? '' : t(`vizLook_${SPECTRUM_LOOKS[lookIndex].id}` as never);

  const step = useCallback((direction: 1 | -1) => {
    if (engine === 'spectrum') {
      setVisualizer({ look: SPECTRUM_LOOKS[(lookIndex + direction + SPECTRUM_LOOKS.length) % SPECTRUM_LOOKS.length].id });
      return;
    }
    if (!presets.length) return;
    const current = presets.findIndex(([entry]) => entry === preset);
    let next = (current + direction + presets.length) % presets.length;
    if (random && direction === 1) {
      next = Math.floor(Math.random() * presets.length);
      if (next === current && presets.length > 1) next = (next + 1) % presets.length;
    }
    setVisualizer({ preset: presets[next][0] });
  }, [engine, lookIndex, presets, preset, random]);

  useEffect(() => onVisualizerCommand((command) => step(command === 'next' ? 1 : -1)), [step]);

  // the engines
  useEffect(() => {
    setProblem(null);
    setReady(false);
    if (engine === 'milkdrop') {
      let cancelled = false;
      let frame = 0;
      void Promise.all([import('butterchurn'), loadMilkdropPresets()]).then(([module, list]) => {
        if (cancelled || !canvas.current || !box.current) return;
        const width = box.current.clientWidth || 640;
        const height = box.current.clientHeight || 360;
        const drawer = module.default.createVisualizer(context, canvas.current, { width, height, pixelRatio: window.devicePixelRatio || 1, textureRatio: 1 });
        drawer.connectAudio(source);
        milkdrop.current = drawer;
        setPresets(list);
        setReady(true);
        const draw = () => {
          drawer.render();
          frame = requestAnimationFrame(draw);
        };
        frame = requestAnimationFrame(draw);
      }).catch((error) => setProblem(error instanceof Error ? error.message : String(error)));
      return () => {
        cancelled = true;
        cancelAnimationFrame(frame);
        if (milkdrop.current) milkdrop.current.disconnectAudio(source);
        milkdrop.current = null;
      };
    }
    let cancelled = false;
    void import('audiomotion-analyzer').then((module) => {
      if (cancelled || !box.current) return;
      analyser.current = new module.default(box.current, { ...BASE, ...SPECTRUM_LOOKS[lookIndex].options, source });
      setReady(true);
    }).catch((error) => setProblem(error instanceof Error ? error.message : String(error)));
    return () => {
      cancelled = true;
      analyser.current?.destroy();
      analyser.current = null;
    };
  }, [engine, context, source]); // eslint-disable-line react-hooks/exhaustive-deps

  // the chosen preset or look; a name MilkDrop does not have gets a random one
  useEffect(() => {
    if (!ready) return undefined;
    if (engine === 'milkdrop') {
      const found = presets.find(([entry]) => entry === preset);
      if (!found) {
        if (presets.length) setVisualizer({ preset: presets[Math.floor(Math.random() * presets.length)][0] });
        return undefined;
      }
      milkdrop.current?.loadPreset(found[1], BLEND);
    } else {
      analyser.current?.setOptions({ ...BASE, ...SPECTRUM_LOOKS[lookIndex].options });
    }
    if (!name) return undefined;
    setFlash(name);
    const hide = window.setTimeout(() => setFlash(null), 2500);
    return () => window.clearTimeout(hide);
  }, [ready, engine, preset, lookIndex, presets]); // eslint-disable-line react-hooks/exhaustive-deps

  // MilkDrop moves on by itself unless held
  useEffect(() => {
    if (engine !== 'milkdrop' || locked || !presets.length) return undefined;
    const timer = window.setInterval(() => step(1), Math.max(3, cycleSeconds) * 1000);
    return () => window.clearInterval(timer);
  }, [engine, locked, presets.length, step, cycleSeconds]);

  // the drawing follows the size of its box
  useEffect(() => {
    const node = box.current;
    if (!node) return undefined;
    const observer = new ResizeObserver(() => {
      if (milkdrop.current && canvas.current) {
        const ratio = window.devicePixelRatio || 1;
        canvas.current.width = Math.round(node.clientWidth * ratio);
        canvas.current.height = Math.round(node.clientHeight * ratio);
        milkdrop.current.setRendererSize(node.clientWidth, node.clientHeight);
      }
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [engine]);

  const onKey = (event: React.KeyboardEvent) => {
    const key = event.key.toLowerCase();
    if (key === 'n' || event.key === 'ArrowRight') step(1);
    else if (key === 'p' || event.key === 'ArrowLeft') step(-1);
    else if (key === 'l') setVisualizer({ locked: !locked });
    else if (key === 'r') setVisualizer({ random: !random });
    else if (key === 't') setVisualizer({ showName: !showName });
    else if (key === 'f') onFullscreen?.();
    else if (event.key === 'Escape') onEscape?.();
    else return;
    event.preventDefault();
    event.stopPropagation();
  };

  return (
    <div
      ref={box}
      tabIndex={0}
      onKeyDown={onKey}
      onDoubleClick={onFullscreen}
      className="relative h-full w-full overflow-hidden bg-black outline-none"
    >
      {engine === 'milkdrop' && <canvas ref={canvas} data-webgl className="absolute inset-0 h-full w-full" />}
      {showName && flash && (
        <div className="pointer-events-none absolute left-3 top-3 z-10 max-w-[80%] truncate rounded-md bg-black/60 px-2 py-1 text-[11px] text-white">{flash}</div>
      )}
      {(locked || !random) && engine === 'milkdrop' && (
        <div className="pointer-events-none absolute right-3 top-3 z-10 rounded-md bg-black/60 px-2 py-1 text-[10px] font-semibold uppercase text-white">
          {[locked ? t('vizLocked') : null, !random ? t('vizInOrder') : null].filter(Boolean).join(' · ')}
        </div>
      )}
      {!ready && !problem && (
        <div className="absolute inset-0 grid place-items-center text-xs text-zinc-400">{t('vizLoading')}</div>
      )}
      {problem && <div className="absolute inset-0 grid place-items-center p-4 text-center text-xs text-rose-300">{t('vizFailed')}: {problem}</div>}
    </div>
  );
};

export const SPECTRUM_LOOK_IDS = SPECTRUM_LOOKS.map((look) => look.id);

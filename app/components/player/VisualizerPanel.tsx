import React, { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import { ChevronLeft, ChevronRight, ExternalLink, ListOrdered, Lock, LockOpen, Maximize2, Minimize2, Move, Shuffle, X } from 'lucide-react';
import { useI18n } from '../../context/I18nContext';
import { ensureAudioGraph, onAudioGraph, type AudioGraph } from '../../services/audioGraph';
import { onVisualizer, setVisualizer, visualizer, visualizerCommand, type VisualizerEngine, type VisualizerState } from '../../services/visualizerState';
import { useFloatable } from '../../services/useFloatable';
import { VisualizerView } from './VisualizerView';

/**
 * The visualiser over the studio: a panel that is dragged by its title and
 * resized by its corner, goes fullscreen, or moves out into a window of its
 * own. It listens to the player after the equalizer.
 */

const SIZE_KEY = 'studio:visualizer-size';

function savedSize(): { width: number; height: number } {
  try {
    const size = JSON.parse(localStorage.getItem(SIZE_KEY) ?? 'null') as { width: number; height: number } | null;
    if (size && size.width >= 240 && size.height >= 160) return size;
  } catch {
    // the default size
  }
  return { width: 480, height: 300 };
}

/** The panel's size, also for the agent. */
export function visualizerPanelSize(): { width: number; height: number } {
  return savedSize();
}

export function setVisualizerPanelSize(size: { width: number; height: number }): void {
  try {
    localStorage.setItem(SIZE_KEY, JSON.stringify(size));
  } catch {
    // kept for this run only
  }
  window.dispatchEvent(new CustomEvent('studio:visualizer-size'));
}

/** Opens the visualiser in its own window. */
export async function openVisualizerWindow(): Promise<void> {
  const already = await WebviewWindow.getByLabel('visualizer');
  await invoke('open_visualizer_window');
  setVisualizer({ place: 'window' });
  if (already) return;
  // closed by its own frame, the window says nothing on its way out: its end is heard here
  const opened = await WebviewWindow.getByLabel('visualizer');
  if (!opened) throw new Error('The visualiser window did not open.');
  await opened.once('tauri://destroyed', () => {
    if (visualizer().place === 'window') setVisualizer({ place: 'closed', fullscreen: false });
  });
}

export const EngineTabs: React.FC = () => {
  const { t } = useI18n();
  const [engine, setEngine] = useState<VisualizerEngine>(() => visualizer().engine);
  useEffect(() => onVisualizer((state) => setEngine(state.engine)), []);
  return (
    <div className="flex rounded-md bg-black/5 p-0.5 dark:bg-white/5">
      {(['milkdrop', 'spectrum'] as const).map((id) => (
        <button
          key={id}
          type="button"
          onPointerDown={(event) => event.stopPropagation()}
          onClick={() => setVisualizer({ engine: id })}
          className={`rounded px-2 py-0.5 text-[10px] font-semibold ${engine === id ? 'bg-pink-500 text-white' : 'text-zinc-500 hover:text-zinc-900 dark:hover:text-white'}`}
        >
          {id === 'milkdrop' ? 'MilkDrop' : t('vizSpectrum')}
        </button>
      ))}
    </div>
  );
};

/**
 * The preset controls both places share: previous, next, hold this one,
 * random or in order, fullscreen. The name of what shows sits between.
 */
export const VisualizerControls: React.FC<{ onFullscreen: () => void; tone?: 'panel' | 'dark' }> = ({ onFullscreen, tone = 'panel' }) => {
  const { t } = useI18n();
  const [view, setView] = useState<VisualizerState>(visualizer);
  useEffect(() => onVisualizer(setView), []);
  const button = tone === 'dark' ? 'rounded p-1 text-zinc-300 hover:bg-white/10 hover:text-white' : 'text-zinc-400 hover:text-zinc-900 dark:hover:text-white';
  const on = tone === 'dark' ? 'rounded p-1 bg-pink-500 text-white' : 'text-pink-600 dark:text-pink-500';
  const milkdrop = view.engine === 'milkdrop';
  const stop = (event: React.PointerEvent) => event.stopPropagation();
  return (
    <>
      <button type="button" onPointerDown={stop} onClick={() => visualizerCommand('previous')} title={t('vizPrevious')} className={button}>
        <ChevronLeft size={15} />
      </button>
      <button type="button" onPointerDown={stop} onClick={() => visualizerCommand('next')} title={t('vizNext')} className={button}>
        <ChevronRight size={15} />
      </button>
      {milkdrop && (
        <button type="button" onPointerDown={stop} onClick={() => setVisualizer({ locked: !view.locked })} title={t('vizLockHint')} className={view.locked ? on : button}>
          {view.locked ? <Lock size={14} /> : <LockOpen size={14} />}
        </button>
      )}
      {milkdrop && (
        <button type="button" onPointerDown={stop} onClick={() => setVisualizer({ random: !view.random })} title={view.random ? t('vizRandomHint') : t('vizInOrderHint')} className={button}>
          {view.random ? <Shuffle size={14} /> : <ListOrdered size={14} />}
        </button>
      )}
      <button type="button" onPointerDown={stop} onClick={onFullscreen} title={t('vizFullscreen')} className={button}>
        {view.fullscreen ? <Minimize2 size={14} /> : <Maximize2 size={14} />}
      </button>
    </>
  );
};

export const VisualizerPanel: React.FC = () => {
  const { t } = useI18n();
  const panel = useFloatable('visualizer', { x: window.innerWidth - 520, y: 72 });
  const [graph, setGraph] = useState<AudioGraph | null>(null);
  const [view, setView] = useState<VisualizerState>(visualizer);
  const [size, setSize] = useState(savedSize);
  const [problem, setProblem] = useState<string | null>(null);
  const body = useRef<HTMLDivElement>(null);
  const resize = useRef<{ x: number; y: number; width: number; height: number } | null>(null);

  useEffect(() => {
    ensureAudioGraph();
    return onAudioGraph(setGraph);
  }, []);
  useEffect(() => onVisualizer(setView), []);
  useEffect(() => {
    const reread = () => setSize(savedSize());
    window.addEventListener('studio:visualizer-size', reread);
    return () => window.removeEventListener('studio:visualizer-size', reread);
  }, []);

  // fullscreen follows the state, so the agent can ask for it too
  useEffect(() => {
    if (view.fullscreen && !document.fullscreenElement) void body.current?.requestFullscreen().catch(() => setVisualizer({ fullscreen: false }));
    if (!view.fullscreen && document.fullscreenElement) void document.exitFullscreen();
  }, [view.fullscreen]);
  useEffect(() => {
    const left = () => {
      if (!document.fullscreenElement && visualizer().fullscreen) setVisualizer({ fullscreen: false });
    };
    document.addEventListener('fullscreenchange', left);
    return () => document.removeEventListener('fullscreenchange', left);
  }, []);

  const close = () => setVisualizer({ place: 'closed', fullscreen: false });
  const fullscreen = () => setVisualizer({ fullscreen: !visualizer().fullscreen });

  const popOut = async () => {
    setProblem(null);
    try {
      await openVisualizerWindow();
    } catch (error) {
      setProblem(error instanceof Error ? error.message : String(error));
    }
  };

  const startResize = (event: React.PointerEvent) => {
    event.preventDefault();
    event.stopPropagation();
    resize.current = { x: event.clientX, y: event.clientY, ...size };
    const move = (moved: PointerEvent) => {
      if (!resize.current) return;
      setSize({
        width: Math.max(240, Math.min(window.innerWidth - 16, resize.current.width + moved.clientX - resize.current.x)),
        height: Math.max(160, Math.min(window.innerHeight - 16, resize.current.height + moved.clientY - resize.current.y)),
      });
    };
    const up = () => {
      resize.current = null;
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      setSize((current) => {
        setVisualizerPanelSize(current);
        return current;
      });
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  };

  const name = view.engine === 'milkdrop' ? view.preset ?? '' : t(`vizLook_${view.look}` as never);
  const button = 'text-zinc-400 hover:text-zinc-900 dark:hover:text-white';

  return (
    <div className="fixed z-[94]" style={{ left: panel.pos.x, top: panel.pos.y, width: size.width }}>
      <div className="overflow-hidden rounded-xl border border-zinc-200 bg-white shadow-2xl dark:border-white/10 dark:bg-suno-card">
        <div
          onPointerDown={panel.onDragStart}
          className={`flex items-center gap-2 border-b border-zinc-200 px-2.5 py-1.5 dark:border-white/10 ${panel.dragging ? 'cursor-grabbing' : 'cursor-grab'}`}
        >
          <Move size={12} className="shrink-0 text-zinc-400" />
          <EngineTabs />
          <span className="min-w-0 flex-1 truncate text-[11px] text-zinc-500" title={name}>{name}</span>
          <VisualizerControls onFullscreen={fullscreen} />
          <button type="button" onPointerDown={(event) => event.stopPropagation()} onClick={() => void popOut()} title={t('vizPopOut')} className={button}>
            <ExternalLink size={14} />
          </button>
          <button type="button" onPointerDown={(event) => event.stopPropagation()} onClick={close} title={t('close')} className={button}>
            <X size={15} />
          </button>
        </div>
        <div ref={body} className="relative bg-black" style={{ height: size.height }}>
          {graph ? (
            <VisualizerView
              context={graph.context}
              source={graph.output}
              onFullscreen={fullscreen}
              onEscape={() => (document.fullscreenElement ? setVisualizer({ fullscreen: false }) : close())}
            />
          ) : (
            <div className="grid h-full place-items-center px-6 text-center text-xs text-zinc-400">{t('vizPressPlay')}</div>
          )}
          <div onPointerDown={startResize} title={t('vizResize')} className="absolute bottom-0 right-0 z-20 h-4 w-4 cursor-nwse-resize bg-[linear-gradient(135deg,transparent_50%,rgba(255,255,255,0.35)_50%)]" />
        </div>
        {problem && <p className="px-3 py-2 text-[11px] text-rose-600 dark:text-rose-300">{problem}</p>}
      </div>
    </div>
  );
};

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Minimize2, Move, PictureInPicture2 } from 'lucide-react';
import { useI18n } from '../context/I18nContext';

/**
 * Live resource readout for local generation.
 *
 * Music inference is the heaviest thing on the machine, so the studio shows
 * what it is actually costing: GPU load, VRAM, temperature, power draw, system
 * RAM and the resident memory of the native engine process. Every value comes
 * from `/v1/system/resources`, which measures rather than estimates — a counter
 * the card does not report is hidden instead of being drawn as zero.
 *
 * The panel lives in the sidebar and can be popped out into a draggable window
 * so it stays visible while working in another view.
 */

interface GpuSnapshot {
  name: string;
  vram_used_mb: number;
  vram_total_mb: number;
  utilization_percent?: number | null;
  temperature_c?: number | null;
  power_draw_w?: number | null;
  power_limit_w?: number | null;
}

interface ResourceSnapshot {
  cpu_percent: number;
  ram_used_mb: number;
  ram_total_mb: number;
  gpus: GpuSnapshot[];
  engine_process?: { name: string; memory_mb: number; cpu_percent: number } | null;
  studio_process_mb: number;
}

const POSITION_KEY = 'studio.resourceMonitor.position';
const gb = (megabytes: number) => `${(megabytes / 1024).toFixed(1)} GB`;

function heat(percent: number): string {
  if (percent >= 90) return '#ef4444';
  if (percent >= 70) return '#f59e0b';
  return '#22c55e';
}

function Dot({ percent }: { percent: number }) {
  const color = heat(percent);
  return <span className="h-1.5 w-1.5 shrink-0 rounded-full" style={{ background: color, boxShadow: `0 0 8px ${color}` }} />;
}

function Bar({ label, percent, value }: { label: string; percent: number; value: string }) {
  const clamped = Math.max(0, Math.min(100, percent));
  const color = heat(clamped);
  return (
    <div>
      <div className="mb-1 flex items-baseline justify-between text-[10px]">
        <span className="text-zinc-500 dark:text-zinc-400">{label}</span>
        <span className="tabular-nums text-zinc-700 dark:text-zinc-200">{value}</span>
      </div>
      <div className="h-1.5 overflow-hidden rounded-full bg-zinc-200 dark:bg-white/10">
        <div className="h-full rounded-full transition-[width] duration-500 ease-out" style={{ width: `${clamped}%`, background: color }} />
      </div>
    </div>
  );
}

function storedPosition(): { x: number; y: number } {
  try {
    const raw = localStorage.getItem(POSITION_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as { x: number; y: number };
      if (Number.isFinite(parsed.x) && Number.isFinite(parsed.y)) return parsed;
    }
  } catch {
    // A blocked or corrupt storage entry must not stop the monitor rendering.
  }
  return { x: Math.max(16, window.innerWidth - 280), y: 72 };
}

export const ResourceMonitor: React.FC<{ isOpen?: boolean }> = ({ isOpen = true }) => {
  const { t } = useI18n();
  const [snapshot, setSnapshot] = useState<ResourceSnapshot | null>(null);
  const [unavailable, setUnavailable] = useState(false);
  const [floating, setFloating] = useState(false);
  const [position, setPosition] = useState(storedPosition);
  const dragOffset = useRef<{ x: number; y: number } | null>(null);

  useEffect(() => {
    let alive = true;
    let interval = 1000;
    let timer = 0;

    const poll = async () => {
      try {
        const response = await fetch('/v1/system/resources');
        if (!response.ok) throw new Error(String(response.status));
        const body: { poll_interval_ms?: number; resources: ResourceSnapshot } = await response.json();
        if (!alive) return;
        setSnapshot(body.resources);
        setUnavailable(false);
        if (body.poll_interval_ms && body.poll_interval_ms !== interval) {
          interval = body.poll_interval_ms;
          window.clearInterval(timer);
          timer = window.setInterval(poll, interval);
        }
      } catch {
        if (alive) setUnavailable(true);
      }
    };

    void poll();
    timer = window.setInterval(poll, interval);
    return () => { alive = false; window.clearInterval(timer); };
  }, []);

  const onDragStart = useCallback((event: React.PointerEvent) => {
    dragOffset.current = { x: event.clientX - position.x, y: event.clientY - position.y };
    (event.target as HTMLElement).setPointerCapture(event.pointerId);
  }, [position]);

  const onDragMove = useCallback((event: React.PointerEvent) => {
    if (!dragOffset.current) return;
    const next = {
      x: Math.max(0, Math.min(window.innerWidth - 260, event.clientX - dragOffset.current.x)),
      y: Math.max(0, Math.min(window.innerHeight - 80, event.clientY - dragOffset.current.y)),
    };
    setPosition(next);
  }, []);

  const onDragEnd = useCallback(() => {
    if (!dragOffset.current) return;
    dragOffset.current = null;
    try {
      localStorage.setItem(POSITION_KEY, JSON.stringify(position));
    } catch {
      // Position is a convenience, not state worth failing over.
    }
  }, [position]);

  if (unavailable || !snapshot) return null;

  const gpu = snapshot.gpus[0];
  const vramPercent = gpu && gpu.vram_total_mb > 0 ? (gpu.vram_used_mb / gpu.vram_total_mb) * 100 : 0;
  const ramPercent = snapshot.ram_total_mb > 0 ? (snapshot.ram_used_mb / snapshot.ram_total_mb) * 100 : 0;
  const lead = gpu?.utilization_percent ?? Math.round(vramPercent || ramPercent);
  const powerPercent = gpu?.power_draw_w && gpu?.power_limit_w ? (gpu.power_draw_w / gpu.power_limit_w) * 100 : null;

  const body = (
    <div className="space-y-2.5">
      {gpu && (
        <>
          {typeof gpu.utilization_percent === 'number' && <Bar label="GPU" percent={gpu.utilization_percent} value={`${gpu.utilization_percent}%`} />}
          <Bar label="VRAM" percent={vramPercent} value={`${gb(gpu.vram_used_mb)} / ${gb(gpu.vram_total_mb)}`} />
          {powerPercent !== null && <Bar label={t('power')} percent={powerPercent} value={`${Math.round(gpu.power_draw_w!)} / ${Math.round(gpu.power_limit_w!)} W`} />}
        </>
      )}
      <Bar label="RAM" percent={ramPercent} value={`${gb(snapshot.ram_used_mb)} / ${gb(snapshot.ram_total_mb)}`} />
      <Bar label="CPU" percent={snapshot.cpu_percent} value={`${Math.round(snapshot.cpu_percent)}%`} />
      <div className="flex items-center justify-between text-[10px] text-zinc-500 dark:text-zinc-400">
        <span>{snapshot.engine_process ? t('engineProcess') : t('engineOffline')}</span>
        <span className="tabular-nums">{snapshot.engine_process ? gb(snapshot.engine_process.memory_mb) : '—'}</span>
      </div>
    </div>
  );

  if (floating) {
    return createPortal(
      <div className="fixed z-[70] w-[252px] select-none" style={{ left: position.x, top: position.y }}>
        <div className="overflow-hidden rounded-xl border border-zinc-200 bg-white/95 shadow-2xl backdrop-blur dark:border-white/10 dark:bg-zinc-900/95">
          <div
            onPointerDown={onDragStart}
            onPointerMove={onDragMove}
            onPointerUp={onDragEnd}
            className="flex cursor-grab items-center gap-2 border-b border-zinc-200 px-3 py-2 active:cursor-grabbing dark:border-white/5"
          >
            <Move size={12} className="text-zinc-400" />
            <Dot percent={lead} />
            <span className="min-w-0 flex-1 truncate text-[11px] font-semibold text-zinc-800 dark:text-zinc-100">
              {gpu ? gpu.name.replace(/NVIDIA GeForce /i, '') : 'Resources'}
            </span>
            {typeof gpu?.temperature_c === 'number' && <span className="text-[10px] tabular-nums text-zinc-500">{gpu.temperature_c}°</span>}
            <button type="button" onClick={() => setFloating(false)} title={t('dockMonitor')} className="text-zinc-400 hover:text-pink-500">
              <Minimize2 size={13} />
            </button>
          </div>
          <div className="px-3 pb-3 pt-2">{body}</div>
        </div>
      </div>,
      document.body,
    );
  }

  if (!isOpen) {
    return (
      <button
        type="button"
        onClick={() => setFloating(true)}
        title={`${gpu ? `${gpu.name}: ` : ''}VRAM ${gb(gpu?.vram_used_mb ?? 0)} / ${gb(gpu?.vram_total_mb ?? 0)}`}
        className="flex w-full justify-center py-2"
      >
        <Dot percent={lead} />
      </button>
    );
  }

  return (
    <div className="rounded-xl border border-zinc-200/70 bg-zinc-50/80 px-3 py-2 dark:border-white/5 dark:bg-zinc-900/50">
      <div className="mb-2 flex items-center gap-2">
        <Dot percent={lead} />
        <span className="min-w-0 flex-1 truncate text-[10px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">
          {gpu ? gpu.name.replace(/NVIDIA GeForce /i, '') : 'Resources'}
        </span>
        <button type="button" onClick={() => setFloating(true)} title={t('popOutMonitor')} className="text-zinc-400 hover:text-pink-500">
          <PictureInPicture2 size={12} />
        </button>
      </div>
      {body}
    </div>
  );
};

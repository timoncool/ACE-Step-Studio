import React, { useCallback, useEffect, useRef, useState } from 'react';
import { Pause, Play } from 'lucide-react';
import { AudioWaveform } from './AudioWaveform';

/**
 * A stem, playable.
 *
 * The browser's own `<audio controls>` was used here first and it was the wrong
 * tool: it draws a different widget in every engine, its scrub bar is a few
 * pixels tall, and seeking a WAV served without range support does nothing at
 * all. This is the studio's transport - the recording's waveform, click or drag
 * on it to move - over one hidden audio element.
 */

const clock = (seconds: number): string => {
  if (!Number.isFinite(seconds) || seconds < 0) return '0:00';
  const whole = Math.floor(seconds);
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, '0')}`;
};

export const StemPlayer: React.FC<{ src: string; label: string }> = ({ src, label }) => {
  const audio = useRef<HTMLAudioElement | null>(null);
  const bar = useRef<HTMLDivElement | null>(null);
  const [playing, setPlaying] = useState(false);
  const [position, setPosition] = useState(0);
  const [length, setLength] = useState(0);

  useEffect(() => {
    const element = audio.current;
    if (!element) return;
    const onTime = () => setPosition(element.currentTime);
    const onMeta = () => setLength(element.duration);
    const onEnd = () => setPlaying(false);
    element.addEventListener('timeupdate', onTime);
    element.addEventListener('loadedmetadata', onMeta);
    element.addEventListener('durationchange', onMeta);
    element.addEventListener('ended', onEnd);
    return () => {
      element.removeEventListener('timeupdate', onTime);
      element.removeEventListener('loadedmetadata', onMeta);
      element.removeEventListener('durationchange', onMeta);
      element.removeEventListener('ended', onEnd);
    };
  }, []);

  const toggle = useCallback(() => {
    const element = audio.current;
    if (!element) return;
    if (element.paused) {
      void element.play();
      setPlaying(true);
    } else {
      element.pause();
      setPlaying(false);
    }
  }, []);

  // Seeking from a pointer position, used by both the click and the drag.
  const seekTo = useCallback((clientX: number) => {
    const element = audio.current;
    const track = bar.current;
    if (!element || !track || !Number.isFinite(element.duration)) return;
    const box = track.getBoundingClientRect();
    const share = Math.min(1, Math.max(0, (clientX - box.left) / box.width));
    element.currentTime = share * element.duration;
    setPosition(element.currentTime);
  }, []);

  return (
    <div className="flex items-center gap-3 rounded-lg border border-zinc-200 px-3 py-2 dark:border-white/10">
      <span className="w-24 shrink-0 truncate text-xs font-semibold uppercase tracking-wide text-zinc-500">{label}</span>
      <button
        type="button"
        onClick={toggle}
        className="grid h-8 w-8 shrink-0 place-items-center rounded-full bg-gradient-to-r from-orange-500 to-pink-600 text-white"
        aria-label={label}
      >
        {playing ? <Pause size={14} /> : <Play size={14} className="ml-0.5" />}
      </button>
      <span className="w-11 shrink-0 text-right text-[11px] tabular-nums text-zinc-500">{clock(position)}</span>
      <div
        ref={bar}
        role="slider"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={Math.round(length)}
        aria-valuenow={Math.round(position)}
        tabIndex={0}
        onPointerMove={(event) => {
          if (event.buttons === 1) seekTo(event.clientX);
        }}
        onKeyDown={(event) => {
          const element = audio.current;
          if (!element) return;
          if (event.key === 'ArrowRight') element.currentTime = Math.min(element.duration, element.currentTime + 5);
          if (event.key === 'ArrowLeft') element.currentTime = Math.max(0, element.currentTime - 5);
        }}
        className="min-w-0 flex-1"
      >
        <AudioWaveform
          url={src}
          currentTime={position}
          duration={length}
          height={32}
          onSeek={(share) => {
            const element = audio.current;
            if (!element || !Number.isFinite(element.duration)) return;
            element.currentTime = share * element.duration;
            setPosition(element.currentTime);
          }}
        />
      </div>
      <span className="w-11 shrink-0 text-[11px] tabular-nums text-zinc-500">{clock(length)}</span>
      <audio ref={audio} src={src} preload="metadata" className="hidden" />
    </div>
  );
};

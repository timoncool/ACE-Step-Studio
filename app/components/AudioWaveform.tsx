import React, { useEffect, useRef, useState } from 'react';

interface AudioWaveformProps {
  url: string;
  currentTime: number;
  duration: number;
  activeColor?: string;
  inactiveColor?: string;
  height?: number;
  onSeek?: (fraction: number) => void;
}

const BARS = 80;

/** A bar waveform of a recording, filled up to the playback position; a click seeks. */
export const AudioWaveform: React.FC<AudioWaveformProps> = ({
  url,
  currentTime,
  duration,
  activeColor = '#ec4899',
  inactiveColor = 'rgba(113,113,122,0.3)',
  height = 28,
  onSeek,
}) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [bars, setBars] = useState<number[]>([]);

  useEffect(() => {
    if (!url) return;
    let cancelled = false;
    setBars([]);
    (async () => {
      const response = await fetch(url);
      if (!response.ok) throw new Error(`waveform: ${url} returned ${response.status}`);
      const context = new AudioContext();
      try {
        const audio = await context.decodeAudioData(await response.arrayBuffer());
        if (cancelled) return;
        const samples = audio.getChannelData(0);
        const perBar = Math.max(1, Math.floor(samples.length / BARS));
        const levels: number[] = [];
        for (let bar = 0; bar < BARS; bar++) {
          const start = bar * perBar;
          const end = Math.min(start + perBar, samples.length);
          let sum = 0;
          for (let i = start; i < end; i++) sum += samples[i] * samples[i];
          levels.push(Math.sqrt(sum / Math.max(1, end - start)));
        }
        const peak = Math.max(...levels, 0.01);
        setBars(levels.map(level => Math.max(level / peak, 0.05)));
      } finally {
        void context.close();
      }
    })().catch(error => console.error('[ERROR] waveform', error));
    return () => { cancelled = true; };
  }, [url]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || bars.length === 0) return;
    const ratio = window.devicePixelRatio || 1;
    const width = canvas.clientWidth;
    const tall = canvas.clientHeight;
    canvas.width = width * ratio;
    canvas.height = tall * ratio;
    const context = canvas.getContext('2d');
    if (!context) return;
    context.scale(ratio, ratio);
    context.clearRect(0, 0, width, tall);
    const gap = 1;
    const barWidth = (width - gap * (bars.length - 1)) / bars.length;
    const progress = duration > 0 ? currentTime / duration : 0;
    bars.forEach((level, index) => {
      const barHeight = Math.max(level * tall * 0.9, 2);
      context.fillStyle = (index + 0.5) / bars.length <= progress ? activeColor : inactiveColor;
      context.beginPath();
      context.roundRect(index * (barWidth + gap), (tall - barHeight) / 2, barWidth, barHeight, 1);
      context.fill();
    });
  }, [bars, currentTime, duration, activeColor, inactiveColor]);

  return (
    <div
      className="w-full cursor-pointer select-none"
      style={{ height }}
      onMouseDown={event => {
        if (!onSeek) return;
        const rect = event.currentTarget.getBoundingClientRect();
        onSeek(Math.max(0, Math.min(1, (event.clientX - rect.left) / rect.width)));
      }}
    >
      <canvas ref={canvasRef} style={{ width: '100%', height: '100%', display: 'block' }} />
    </div>
  );
};

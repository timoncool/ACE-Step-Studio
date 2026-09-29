import React, { useEffect, useRef, useState } from 'react';
import { ChevronsRight, Square } from 'lucide-react';

/** Past this share of the way the slide counts; short of it the knob goes back. */
const COMMIT_AT = 0.9;

interface SlideToEnableProps {
  on: boolean;
  onChange: (on: boolean) => void;
  /** What the track says while off: the slide it asks for. */
  offLabel: string;
  /** The button that switches it off again. */
  stopLabel: string;
  title?: string;
}

/**
 * A mode that is risky to start and safe to stop. Starting takes a slide of
 * the knob to the far end, so a stray click cannot set it going; once on, the
 * same place is a plain, bright button that stops it. The keyboard starts it
 * with the right arrow, Enter or Space on the knob: the way without dragging
 * that WCAG 2.5.7 asks for.
 */
export const SlideToEnable: React.FC<SlideToEnableProps> = ({ on, onChange, offLabel, stopLabel, title }) => {
  const track = useRef<HTMLDivElement>(null);
  const knob = useRef<HTMLButtonElement>(null);
  const stop = useRef<HTMLButtonElement>(null);
  const drag = useRef<{ startX: number; travel: number } | null>(null);
  // how far the knob has been pulled, in pixels, while a slide is under way
  const [pulled, setPulled] = useState<number | null>(null);
  // the control had the focus when it switched: the new face of it takes it
  const keepFocus = useRef(false);

  useEffect(() => {
    if (!keepFocus.current) return;
    keepFocus.current = false;
    (on ? stop.current : knob.current)?.focus();
  }, [on]);

  const change = (next: boolean) => {
    keepFocus.current = document.activeElement === knob.current || document.activeElement === stop.current;
    onChange(next);
  };

  if (on) {
    return (
      <button
        ref={stop}
        type="button"
        role="switch"
        aria-checked
        title={title}
        onClick={() => change(false)}
        className="flex h-10 w-full items-center justify-center gap-2 rounded-xl bg-rose-600 text-sm font-bold text-white shadow-md transition hover:bg-rose-500 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-rose-400"
      >
        <Square size={14} fill="currentColor" />
        {stopLabel}
      </button>
    );
  }

  const reach = (clientX: number) => {
    const slide = drag.current;
    return slide ? Math.min(slide.travel, Math.max(0, clientX - slide.startX)) : 0;
  };

  const start = (event: React.PointerEvent<HTMLButtonElement>) => {
    if (event.button !== 0 || !track.current) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { startX: event.clientX, travel: track.current.clientWidth - event.currentTarget.offsetWidth - 4 };
    setPulled(0);
  };

  const move = (event: React.PointerEvent<HTMLButtonElement>) => {
    if (drag.current) setPulled(reach(event.clientX));
  };

  const release = (event: React.PointerEvent<HTMLButtonElement>) => {
    const slide = drag.current;
    if (!slide) return;
    const reached = reach(event.clientX);
    drag.current = null;
    setPulled(null);
    if (slide.travel > 0 && reached >= slide.travel * COMMIT_AT) change(true);
  };

  const cancel = () => {
    drag.current = null;
    setPulled(null);
  };

  // A pointer click does nothing: starting takes the slide. A click the
  // keyboard made (Enter or Space on the focused knob) has no click count.
  const click = (event: React.MouseEvent<HTMLButtonElement>) => {
    if (event.detail === 0) change(true);
  };

  const key = (event: React.KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === 'ArrowRight' || event.key === 'End') {
      event.preventDefault();
      change(true);
    }
  };

  const sliding = pulled !== null;
  return (
    <div
      ref={track}
      title={title}
      className="relative h-10 w-full select-none overflow-hidden rounded-xl border border-zinc-200 bg-zinc-100 text-xs text-zinc-500 dark:border-white/10 dark:bg-white/5 dark:text-zinc-400"
    >
      {sliding && <div className="absolute inset-y-0 left-0 rounded-xl bg-rose-500/20" style={{ width: pulled + 38 }} />}
      <span className="pointer-events-none absolute inset-0 flex items-center justify-center px-12 text-center font-medium">
        {offLabel}
      </span>
      <button
        ref={knob}
        type="button"
        role="switch"
        aria-checked={false}
        aria-label={offLabel}
        onPointerDown={start}
        onPointerMove={move}
        onPointerUp={release}
        onPointerCancel={cancel}
        onLostPointerCapture={cancel}
        onClick={click}
        onKeyDown={key}
        className={`absolute left-0.5 top-0.5 flex h-8.5 w-8.5 cursor-grab touch-none items-center justify-center rounded-lg bg-white text-zinc-500 shadow-sm active:cursor-grabbing focus-visible:outline-2 focus-visible:outline-rose-500 dark:bg-zinc-700 dark:text-zinc-200 ${sliding ? '' : 'transition-transform duration-200'}`}
        style={sliding ? { transform: `translateX(${pulled}px)` } : undefined}
      >
        <ChevronsRight size={16} />
      </button>
    </div>
  );
};

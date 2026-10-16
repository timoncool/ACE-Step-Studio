import React, { useEffect, useRef, useState } from 'react';

/** Decimals a slider's value is typed with: whole steps take whole numbers, finer steps at least two places. */
export const decimalsOf = (step: number): number => (step >= 1 ? 0 : Math.max(2, String(step).split('.')[1]?.length ?? 2));

/** `2:10` as seconds, else the number the text starts with (a comma as the decimal point, a unit after it ignored); NaN for anything else. */
const parse = (text: string): number => {
  const clock = /^\s*(\d+):(\d{1,2})\s*$/.exec(text);
  if (clock) return Number(clock[1]) * 60 + Number(clock[2]);
  const number = /^\s*([-+]?\d*[.,]?\d+)/.exec(text);
  return number ? Number(number[1].replace(',', '.')) : NaN;
};

/**
 * A slider's value, typed. Enter or leaving the field keeps the number within
 * the slider's range; Escape, or text that is not a number, puts back the
 * value it had.
 */
export const NumberField: React.FC<{
  value: number;
  min: number;
  max: number;
  step: number;
  label: string;
  onChange: (value: number) => void;
  format?: (value: number) => string;
  disabled?: boolean;
  className?: string;
}> = ({ value, min, max, step, label, onChange, format, disabled, className }) => {
  const decimals = decimalsOf(step);
  const width = /(^|\s)w-/.test(className ?? '') ? '' : 'w-14 ';
  const shown = format ? format(value) : value.toFixed(decimals);
  const [draft, setDraft] = useState<string | null>(null);
  // Escape blurs the field too; this keeps that blur from saving what was typed
  const cancelled = useRef(false);
  useEffect(() => setDraft(null), [value]);
  const commit = () => {
    if (draft === null || cancelled.current) {
      cancelled.current = false;
      setDraft(null);
      return;
    }
    setDraft(null);
    if (draft.trim() === '' || draft.trim() === shown) return;
    const typed = draft.trim() === '∞' ? 0 : parse(draft);
    if (!Number.isFinite(typed)) return;
    const next = Number(Math.min(max, Math.max(min, typed)).toFixed(decimals));
    if (next !== value) onChange(next);
  };
  return (
    <input
      type="text"
      inputMode="decimal"
      value={draft ?? shown}
      disabled={disabled}
      aria-label={label}
      onFocus={event => event.currentTarget.select()}
      onChange={event => setDraft(event.target.value)}
      onBlur={commit}
      onKeyDown={event => {
        if (event.key === 'Enter') event.currentTarget.blur();
        if (event.key === 'Escape') {
          cancelled.current = true;
          event.currentTarget.blur();
        }
      }}
      className={`${width}rounded border border-zinc-200 bg-transparent px-1 py-px text-right text-[11px] tabular-nums text-zinc-700 outline-none transition-colors hover:border-zinc-300 focus:border-pink-400 disabled:opacity-50 dark:border-white/10 dark:text-zinc-200 dark:hover:border-white/20 ${className ?? ''}`}
    />
  );
};

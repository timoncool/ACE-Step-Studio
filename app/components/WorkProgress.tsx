import React, { useEffect, useState } from 'react';
import { Loader2 } from 'lucide-react';
import { useI18n } from '../context/I18nContext';

export const elapsedClock = (seconds: number) => {
  const whole = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(whole / 3600);
  const minutes = Math.floor((whole % 3600) / 60);
  const rest = String(whole % 60).padStart(2, '0');
  return hours > 0 ? `${hours}:${String(minutes).padStart(2, '0')}:${rest}` : `${minutes}:${rest}`;
};

/**
 * A running job's line and bar. With a percent the bar fills to it; without one (the server does not report how far
 * the work is) the bar runs without a figure and the line shows how long the job has been going.
 */
export const WorkProgress: React.FC<{ label: string; percent: number | null; startedMs?: number }> = ({ label, percent, startedMs }) => {
  const { t } = useI18n();
  const [now, setNow] = useState(() => Date.now());
  const counting = percent === null && !!startedMs;
  useEffect(() => {
    if (!counting) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [counting]);

  return (
    <div className="mt-3">
      <div className="flex items-center justify-between text-xs font-medium text-zinc-700 dark:text-zinc-200">
        <span className="flex items-center gap-2"><Loader2 size={14} className="animate-spin text-pink-500" />{label}</span>
        {percent !== null ? (
          <span className="tabular-nums text-zinc-500">{percent}%</span>
        ) : (
          counting && <span className="tabular-nums text-zinc-500">{t('workElapsed').replace('{time}', elapsedClock((now - startedMs!) / 1000))}</span>
        )}
      </div>
      <div
        className="mt-2 h-1.5 overflow-hidden rounded-full bg-zinc-200 dark:bg-black/30"
        role="progressbar"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent ?? undefined}
      >
        {percent !== null ? (
          <div className="h-full bg-linear-to-r from-orange-500 to-pink-500 transition-[width]" style={{ width: `${percent}%` }} />
        ) : (
          <div className="h-full w-full animate-pulse bg-linear-to-r from-orange-500 to-pink-500" />
        )}
      </div>
    </div>
  );
};

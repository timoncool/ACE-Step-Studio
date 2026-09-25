import React from 'react';
import { useI18n } from '../context/I18nContext';

/**
 * What a local model runs on.
 *
 * One control, used everywhere the question is asked - the download page and
 * the tools panel had grown two versions of it that looked different and
 * behaved the same, which is how an interface starts teaching people that the
 * same thing in two places might mean two things.
 */
export type Device = 'auto' | 'cuda' | 'cpu';

export const DEVICES: Device[] = ['auto', 'cuda', 'cpu'];

export const DevicePicker: React.FC<{
  value: Device;
  onChange: (value: Device) => void;
  /** The card is not offered when its runtime is not installed. */
  cudaAvailable?: boolean;
}> = ({ value, onChange, cudaAvailable = true }) => {
  const { t } = useI18n();
  return (
    <div className="flex gap-1.5">
      {DEVICES.map(device => (
        <button
          key={device}
          type="button"
          onClick={() => onChange(device)}
          disabled={device === 'cuda' && !cudaAvailable}
          className={`flex-1 cursor-pointer rounded-lg border px-3 py-2 text-xs font-semibold transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
            value === device
              ? 'border-pink-400 bg-pink-500/10 text-zinc-900 dark:text-white'
              : 'border-zinc-200 text-zinc-500 hover:border-pink-300 hover:bg-pink-500/5 hover:text-zinc-900 dark:border-white/10 dark:hover:border-pink-500/40 dark:hover:text-white'
          }`}
        >
          {t(device === 'auto' ? 'separationRuntimeAuto' : device === 'cuda' ? 'separationRuntimeGpu' : 'separationRuntimeCpu')}
        </button>
      ))}
    </div>
  );
};

/**
 * The same row of choices, wherever a choice is made.
 *
 * The settings panels had each drawn their own - indigo, thicker borders,
 * different radius - so the same decision looked like a different control
 * depending on which page you were on.
 */
export const ChoiceTabs = <T extends string>({ options, value, onChange, columns = 3 }: {
  options: { id: T; label: string; disabled?: boolean }[];
  value: T;
  onChange: (value: T) => void;
  columns?: number;
}) => (
  <div className={`grid gap-1.5 ${columns === 2 ? 'grid-cols-2' : 'grid-cols-3'}`}>
    {options.map(option => (
      <button
        key={option.id}
        type="button"
        disabled={option.disabled}
        onClick={() => onChange(option.id)}
        className={`cursor-pointer rounded-lg border px-3 py-2 text-xs font-semibold transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
          value === option.id
            ? 'border-pink-400 bg-pink-500/10 text-zinc-900 dark:text-white'
            : 'border-zinc-200 text-zinc-500 hover:border-pink-300 hover:bg-pink-500/5 hover:text-zinc-900 dark:border-white/10 dark:hover:border-pink-500/40 dark:hover:text-white'
        }`}
      >
        {option.label}
      </button>
    ))}
  </div>
);

import React, { useEffect, useRef, useState } from 'react';
import { ArrowUpDown, Check } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import type { TranslationKey } from '../i18n/translations';
import type { SongOrder } from '../services/songOrder';
import { named } from '../services/accessibleName';

const LABELS: Record<SongOrder, TranslationKey> = {
  newest: 'sortNewest',
  oldest: 'sortOldest',
  titleAsc: 'sortTitleAsc',
  titleDesc: 'sortTitleDesc',
  longest: 'sortLongest',
  shortest: 'sortShortest',
  likedNewest: 'sortLikedNewest',
};

/** "Sort: newest first", opening the orders a list can be read in. */
export const SortMenu: React.FC<{ order: SongOrder; orders: SongOrder[]; onChange: (order: SongOrder) => void }> = ({ order, orders, onChange }) => {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const menu = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const outside = (event: MouseEvent) => {
      if (menu.current && !menu.current.contains(event.target as Node)) setOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false);
    };
    document.addEventListener('mousedown', outside);
    document.addEventListener('keydown', escape);
    return () => {
      document.removeEventListener('mousedown', outside);
      document.removeEventListener('keydown', escape);
    };
  }, [open]);

  return (
    <div className="relative" ref={menu}>
      <button
        type="button"
        onClick={() => setOpen(prev => !prev)}
        aria-haspopup="menu"
        aria-expanded={open}
        {...named(`${t('sortBy')}: ${t(LABELS[order])}`)}
        className={`flex select-none items-center gap-2 rounded-lg border px-4 py-2.5 text-xs font-bold transition-all ${open
          ? 'border-transparent bg-zinc-900 text-white dark:bg-white dark:text-black'
          : 'border-zinc-200 bg-zinc-100 text-zinc-700 hover:bg-zinc-200 dark:border-white/10 dark:bg-suno-panel dark:text-white dark:hover:bg-white/5'}`}
      >
        <ArrowUpDown size={14} />
        <span className="whitespace-nowrap">{t(LABELS[order])}</span>
      </button>
      {open && (
        <div role="menu" className="absolute right-0 top-full z-50 mt-2 w-60 origin-top-right overflow-hidden rounded-xl border border-zinc-200 bg-white py-1 shadow-2xl dark:border-white/10 dark:bg-suno-card">
          <div className="px-3 py-2 text-[10px] font-bold uppercase tracking-wider text-zinc-500">{t('sortBy')}</div>
          {orders.map(option => (
            <button
              key={option}
              type="button"
              role="menuitemradio"
              aria-checked={option === order}
              onClick={() => { onChange(option); setOpen(false); }}
              className="flex w-full items-center justify-between px-4 py-2.5 text-left text-sm font-medium text-zinc-700 transition-colors hover:bg-zinc-100 hover:text-black dark:text-zinc-300 dark:hover:bg-white/5 dark:hover:text-white"
            >
              {t(LABELS[option])}
              {option === order && <Check size={14} className="text-pink-600 dark:text-pink-500" />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
};

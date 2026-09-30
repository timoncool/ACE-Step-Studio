import React from 'react';
import { AudioLines, ChevronRight, Drum, Guitar, MicVocal, Music, Piano } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import type { TranslationKey } from '../i18n/translations';
import { stemOf } from '../services/songStems';
import type { Song } from '../types';

const ICONS: Record<string, React.ComponentType<{ size?: number }>> = {
  vocals: MicVocal,
  drums: Drum,
  bass: AudioLines,
  guitar: Guitar,
  piano: Piano,
};

/** The stem's name in the window's language. */
export function useStemName(): (song: Song) => string {
  const { t } = useI18n();
  return song => {
    const stem = stemOf(song) ?? 'other';
    return t(`stem_${stem}` as TranslationKey) || stem;
  };
}

export const StemIcon: React.FC<{ song: Song; size?: number }> = ({ song, size = 14 }) => {
  const Icon = ICONS[stemOf(song) ?? 'other'] ?? Music;
  return <Icon size={size} />;
};

/** The row under a song that folds its stems away; closed, it still shows which parts there are. */
export const StemsToggle: React.FC<{ stems: Song[]; open: boolean; onToggle: () => void }> = ({ stems, open, onToggle }) => {
  const { t } = useI18n();
  const stemName = useStemName();
  return (
    <button
      type="button"
      aria-expanded={open}
      onClick={event => { event.stopPropagation(); onToggle(); }}
      className="flex items-center gap-2 rounded-md px-2 py-1 text-xs font-medium text-zinc-500 transition-colors hover:bg-zinc-100 hover:text-black dark:text-zinc-400 dark:hover:bg-white/5 dark:hover:text-white"
    >
      <ChevronRight size={14} className={`transition-transform ${open ? 'rotate-90' : ''}`} />
      <span>{t('stems')} · {stems.length}</span>
      {!open && (
        <span className="flex items-center gap-1.5 text-zinc-400 dark:text-zinc-500">
          {stems.map(stem => (
            <span key={stem.id} title={stemName(stem)} className="flex">
              <StemIcon song={stem} />
            </span>
          ))}
        </span>
      )}
    </button>
  );
};

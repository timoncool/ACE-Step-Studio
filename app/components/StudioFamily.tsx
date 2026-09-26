import React from 'react';
import { ExternalLink } from 'lucide-react';
import { useI18n } from '../context/I18nContext';

/**
 * The three music studios on one core, for the About page: each on its own
 * model, with the same interface and agent tools. The one open is marked.
 */

export type StudioId = 'yue2' | 'minimax' | 'ace';

const STUDIOS: { id: StudioId; name: string; page: string; line: string }[] = [
  { id: 'yue2', name: 'YuE2 Studio', page: 'https://timoncool.github.io/YuE2-Studio/', line: 'studioYue2' },
  { id: 'minimax', name: 'MiniMax Music3 Studio', page: 'https://timoncool.github.io/MiniMax-Music3-Studio/', line: 'studioMiniMax' },
  { id: 'ace', name: 'ACE-Step Studio', page: 'https://timoncool.github.io/ACE-Step-Studio/', line: 'studioAce' },
];

export const StudioFamily: React.FC<{ current: StudioId }> = ({ current }) => {
  const { t } = useI18n();
  const tr = t as unknown as (key: string) => string;
  return (
    <div className="space-y-3 border-t border-zinc-200 pt-4 dark:border-zinc-700/50">
      <p className="font-medium text-zinc-900 dark:text-white">{tr('studioFamily')}</p>
      <p className="text-xs leading-5">{tr('studioFamilyLine')}</p>
      <div className="grid gap-2">
        {STUDIOS.map((studio) => (
          <a
            key={studio.id}
            href={studio.page}
            target="_blank"
            rel="noopener noreferrer"
            className={`flex items-start gap-3 rounded-lg border px-3 py-2 transition-colors ${
              studio.id === current
                ? 'border-pink-400/60 bg-pink-500/5'
                : 'border-zinc-200 hover:border-pink-400 dark:border-zinc-700'
            }`}
          >
            <span className="min-w-0 flex-1">
              <span className="block text-xs font-semibold text-zinc-900 dark:text-white">
                {studio.name}
                {studio.id === current && <span className="ml-2 text-[10px] font-medium text-pink-600 dark:text-pink-400">{tr('studioThis')}</span>}
              </span>
              <span className="block text-[11px] leading-4 text-zinc-500">{tr(studio.line)}</span>
            </span>
            <ExternalLink size={13} className="mt-0.5 shrink-0 text-zinc-400" />
          </a>
        ))}
      </div>
    </div>
  );
};

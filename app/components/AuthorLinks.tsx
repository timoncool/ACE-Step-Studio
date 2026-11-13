import React from 'react';
import { Globe } from 'lucide-react';
import { SiGithub, SiTelegram, SiX } from '@icons-pack/react-simple-icons';
import { useI18n } from '../context/I18nContext';

type Network = 'Telegram' | 'X' | 'GitHub' | 'site';

/** Where to find the author, one list for every studio, each link signed "Network · name". */
export const AUTHOR_LINKS: { network: Network; name: string; url: string }[] = [
  { network: 'Telegram', name: '@nerual_dreming', url: 'https://t.me/nerual_dreming' },
  { network: 'Telegram', name: '@neuroport', url: 'https://t.me/neuroport' },
  { network: 'X', name: '@nerualdreming', url: 'https://x.com/nerualdreming' },
  { network: 'GitHub', name: 'timoncool', url: 'https://github.com/timoncool' },
  { network: 'site', name: 'artgeneration.me', url: 'https://artgeneration.me' },
];

const ICONS: Record<Network, React.ReactNode> = {
  Telegram: <SiTelegram size={14} aria-hidden="true" />,
  X: <SiX size={13} aria-hidden="true" />,
  GitHub: <SiGithub size={14} aria-hidden="true" />,
  site: <Globe size={14} aria-hidden="true" />,
};

/** The button look of the About page: a neutral plate with a border, 44 pixels high on a phone. */
export const ABOUT_LINK = 'inline-flex items-center gap-1.5 max-sm:min-h-11 rounded-lg border border-zinc-300 px-3 py-1.5 text-xs font-medium text-zinc-700 hover:border-pink-500 hover:text-pink-500 dark:border-zinc-600 dark:text-zinc-200';

/** The heading of each block of the About page: the author, support. */
export const ABOUT_HEADING = 'text-xs font-semibold uppercase tracking-wide text-zinc-900 dark:text-white';

/** The author's links, the same buttons as the rest of the page, in one row that wraps. */
export const AuthorLinks: React.FC = () => {
  const { t } = useI18n();
  return (
    <div className="flex flex-wrap gap-2">
      {AUTHOR_LINKS.map(link => (
        <a key={link.url} href={link.url} target="_blank" rel="noopener noreferrer" className={ABOUT_LINK}>
          {ICONS[link.network]}
          <span>{link.network === 'site' ? t('aboutSite') : link.network} · {link.name}</span>
        </a>
      ))}
    </div>
  );
};

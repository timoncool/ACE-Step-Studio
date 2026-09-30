import React, { useEffect } from 'react';
import { Bot, MonitorSpeaker, X } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import type { TranslationKey } from '../i18n/translations';
import { named } from '../services/accessibleName';
import { exactMoment, relativeMoment } from '../services/dates';
import { clearJournal, removeJournalLine, useJournal, type JournalLine } from '../services/journal';

const TONE: Record<string, string> = {
  error: 'text-rose-500',
  success: 'text-emerald-500',
};

/** The journal: what agents changed and what the studio said, newest first. */
export const JournalPanel: React.FC<{ open: boolean; onClose: () => void }> = ({ open, onClose }) => {
  const { t, language } = useI18n();
  const journal = useJournal();
  const lines = journal.data ?? [];

  useEffect(() => {
    if (!open) return;
    const escape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', escape);
    return () => document.removeEventListener('keydown', escape);
  }, [open, onClose]);

  if (!open) return null;

  // an agent's change in the window's words, never in a tool's name; the words
  // go in as they are, a $ in a title included
  const sentence = (line: JournalLine) => {
    if (line.source !== 'agent') return line.text;
    const verb = t(`journalVerb_${line.verb}` as TranslationKey);
    const kind = t(`journalKind_${line.kind}` as TranslationKey);
    return (line.target ? t('journalLine') : t('journalLineNoName'))
      .replace('{verb}', () => verb)
      .replace('{kind}', () => kind)
      .replace('{name}', () => line.target);
  };

  const report = (problem: unknown) => console.error('[ERROR] the journal:', problem);

  return (
    <aside
      aria-label={t('journal')}
      className="fixed inset-y-0 right-0 z-[60] flex w-[min(24rem,100vw)] flex-col border-l border-zinc-200 bg-white shadow-2xl dark:border-white/10 dark:bg-suno-panel"
    >
      <div className="flex h-14 shrink-0 items-center gap-2 border-b border-zinc-200 px-4 dark:border-white/5">
        <h2 className="text-sm font-semibold text-zinc-900 dark:text-white">{t('journal')}</h2>
        <span className="text-xs text-zinc-500 dark:text-zinc-400">{lines.length}</span>
        {lines.length > 0 && (
          <button
            type="button"
            onClick={() => void clearJournal().catch(report)}
            className="ml-auto rounded-md px-2 py-1 text-xs font-medium text-zinc-500 transition-colors hover:bg-zinc-100 hover:text-black dark:text-zinc-400 dark:hover:bg-white/10 dark:hover:text-white"
          >
            {t('journalClear')}
          </button>
        )}
        <button
          type="button"
          onClick={onClose}
          {...named(t('close'))}
          className={`rounded-full p-1.5 text-zinc-500 transition-colors hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-white/10 ${lines.length > 0 ? '' : 'ml-auto'}`}
        >
          <X size={18} />
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-3 py-2">
        {journal.isError ? (
          <p role="alert" className="px-1 py-3 text-xs leading-5 text-rose-500">
            {t('journalUnreadable').replace('{error}', () => (journal.error instanceof Error ? journal.error.message : String(journal.error)))}
          </p>
        ) : lines.length === 0 ? (
          <p className="px-1 py-3 text-xs leading-5 text-zinc-500 dark:text-zinc-400">{t('journalEmpty')}</p>
        ) : (
          <ul className="space-y-1">
            {lines.map(line => {
              const when = new Date(Number(line.at) * 1000);
              const agent = line.source === 'agent';
              return (
                <li key={line.id} className="group flex items-start gap-2 rounded-md px-1 py-1.5 hover:bg-zinc-50 dark:hover:bg-white/5">
                  <span
                    role="img"
                    {...named(agent ? t('journalAgent') : t('journalStudio'))}
                    className={`mt-0.5 shrink-0 ${agent ? 'text-violet-500' : TONE[line.tone] ?? 'text-zinc-400'}`}
                  >
                    {agent ? <Bot size={15} /> : <MonitorSpeaker size={15} />}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block break-words text-sm leading-snug text-zinc-800 dark:text-zinc-100">{sentence(line)}</span>
                    <time dateTime={when.toISOString()} title={exactMoment(when, language)} className="block text-[11px] text-zinc-500">
                      {relativeMoment(when, language)}
                    </time>
                  </span>
                  <button
                    type="button"
                    onClick={() => void removeJournalLine(line.id).catch(report)}
                    {...named(t('journalRemove'))}
                    className="shrink-0 rounded-md p-1 text-zinc-400 opacity-0 transition-opacity hover:bg-zinc-200 hover:text-black focus-visible:opacity-100 group-hover:opacity-100 dark:hover:bg-white/10 dark:hover:text-white"
                  >
                    <X size={13} />
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </aside>
  );
};

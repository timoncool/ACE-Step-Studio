import React, { useEffect, useRef, useState } from 'react';
import { FileUp } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { assistantStatusChanged } from '../services/studioQueries';

interface OwnRules { song: string; style: string; lyrics: string }
const EMPTY: OwnRules = { song: '', style: '', lyrics: '' };
const FIELD = 'w-full resize-y rounded-lg border border-zinc-200 bg-white px-3 py-2 text-xs text-zinc-900 outline-hidden focus:border-pink-400 dark:border-white/10 dark:bg-black/20 dark:text-white';

/** The person's own rules for each task of the writing assistant, and the words it must not write. */
export const AssistantRules: React.FC = () => {
  const { t } = useI18n();
  const [rules, setRules] = useState<OwnRules>(EMPTY);
  const [avoid, setAvoid] = useState('');
  const [error, setError] = useState<string | null>(null);
  const loaded = useRef(false);
  const picker = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    void fetch('/v1/assistant/status')
      .then(response => (response.ok ? response.json() : Promise.reject(new Error(String(response.status)))))
      .then((status: { own_rules?: Partial<Record<keyof OwnRules, string | null>>; avoid_words?: string | null }) => {
        const own = status.own_rules ?? {};
        setRules({ song: own.song ?? '', style: own.style ?? '', lyrics: own.lyrics ?? '' });
        setAvoid(status.avoid_words ?? '');
        loaded.current = true;
      })
      .catch((reason: unknown) => setError(reason instanceof Error ? reason.message : String(reason)));
  }, []);

  // saved as typed, once the typing stops
  useEffect(() => {
    if (!loaded.current) return;
    const timer = window.setTimeout(() => {
      const text = (value: string) => value.trim() || null;
      void fetch('/v1/assistant/status', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        // an empty list is sent as "", which the service stores as none: a null there reads as "keep what is stored"
        body: JSON.stringify({ own_rules: { song: text(rules.song), style: text(rules.style), lyrics: text(rules.lyrics) }, avoid_words: avoid.trim() }),
      })
        .then(async response => {
          if (!response.ok) throw new Error((await response.json().catch(() => null))?.error || String(response.status));
          setError(null);
          assistantStatusChanged();
        })
        .catch((reason: unknown) => setError(reason instanceof Error ? reason.message : String(reason)));
    }, 600);
    return () => window.clearTimeout(timer);
  }, [rules, avoid]);

  const field = (key: keyof OwnRules, label: string) => (
    <label className="block space-y-1">
      <span className="text-[11px] font-semibold text-zinc-600 dark:text-zinc-300">{label}</span>
      <textarea rows={2} value={rules[key]} onChange={event => setRules(current => ({ ...current, [key]: event.target.value }))} placeholder={t('assistantRulesPlaceholder')} className={FIELD} />
    </label>
  );

  return (
    <div className="space-y-3 border-t border-zinc-200 pt-3 dark:border-white/10">
      <div>
        <p className="text-xs font-semibold text-zinc-900 dark:text-white">{t('assistantRulesTitle')}</p>
        <p className="text-[11px] text-zinc-500">{t('assistantRulesHint')}</p>
      </div>
      {field('song', t('assistantRulesSong'))}
      {field('style', t('assistantRulesStyle'))}
      {field('lyrics', t('assistantRulesLyrics'))}
      <label className="block space-y-1">
        <span className="flex items-center justify-between gap-2">
          <span className="text-[11px] font-semibold text-zinc-600 dark:text-zinc-300">{t('assistantAvoidWords')}</span>
          <button type="button" onClick={() => picker.current?.click()} className="inline-flex items-center gap-1 text-[11px] font-semibold text-pink-600 hover:underline dark:text-pink-300">
            <FileUp size={12} /> {t('assistantAvoidWordsLoad')}
          </button>
        </span>
        <textarea rows={3} value={avoid} onChange={event => setAvoid(event.target.value)} placeholder={t('assistantAvoidWordsPlaceholder')} className={FIELD} />
      </label>
      <input
        ref={picker}
        type="file"
        accept=".txt,text/plain"
        className="hidden"
        onChange={event => {
          const file = event.target.files?.[0];
          event.target.value = '';
          if (!file) return;
          void file.text().then(text => setAvoid(text.trim()), (reason: unknown) => setError(reason instanceof Error ? reason.message : String(reason)));
        }}
      />
      {error && <p role="alert" className="text-[11px] text-red-500">{error}</p>}
    </div>
  );
};

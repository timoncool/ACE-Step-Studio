import React, { useState } from 'react';
import { Check, Copy } from 'lucide-react';
import { useI18n } from '../context/I18nContext';

/** One model file as the catalogue names it, with its pinned addresses. */
export type ManualFile = { id: string; filename: string; bytes: number; url?: string; mirror_url?: string };

const CopyButton: React.FC<{ value: string }> = ({ value }) => {
  const { t } = useI18n();
  const [outcome, setOutcome] = useState<'copied' | 'failed' | null>(null);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      setOutcome('copied');
    } catch (error) {
      console.error('[ERROR] copy failed:', error);
      setOutcome('failed');
    }
    window.setTimeout(() => setOutcome(null), 2500);
  };
  return (
    <button
      type="button"
      onClick={() => void copy()}
      className={`inline-flex shrink-0 items-center gap-1 rounded-md border px-1.5 py-0.5 text-[10px] font-medium transition-colors hover:border-pink-400 hover:text-pink-600 dark:border-white/10 ${outcome === 'failed' ? 'border-rose-300 text-rose-600 dark:text-rose-300' : 'border-zinc-200 text-zinc-600 dark:text-zinc-300'}`}
    >
      {outcome === 'copied' ? <Check size={11} /> : <Copy size={11} />}
      {outcome === 'copied' ? t('agentCopied') : outcome === 'failed' ? t('manualCopyFailed') : t('agentCopy')}
    </button>
  );
};

const Address: React.FC<{ label: string; url: string }> = ({ label, url }) => (
  <div className="flex items-start gap-2">
    <span className="w-24 shrink-0 text-zinc-500 dark:text-zinc-400">{label}</span>
    <a href={url} className="min-w-0 flex-1 break-all font-mono text-pink-600 hover:underline dark:text-pink-400">{url}</a>
    <CopyButton value={url} />
  </div>
);

/**
 * The files of the chosen set that are not on disk, each with its exact name, size and pinned address, and the
 * folder they go into: for a network the studio's own download does not get through, and for anyone who fetched
 * a file from the repository's latest revision, which can be another build under the same name.
 */
export const ManualDownloads: React.FC<{ files: ManualFile[]; folder?: string | null; mirror: boolean }> = ({ files, folder, mirror }) => {
  const { t } = useI18n();
  if (files.length === 0) return null;
  return (
    <details className="mt-2 rounded-lg border border-zinc-200 p-3 text-[11px] leading-5 dark:border-white/10">
      <summary className="cursor-pointer font-semibold text-zinc-700 dark:text-zinc-200">
        {t('manualTitle').replace('{count}', String(files.length))}
      </summary>
      <p className="mt-2 text-zinc-500 dark:text-zinc-400">{t('manualHint')}</p>
      <div className="mt-2 space-y-3">
        {files.map((file) => (
          <div key={file.id} className="space-y-1">
            <div className="flex flex-wrap items-baseline gap-x-2">
              <span className="break-all font-mono font-semibold text-zinc-700 dark:text-zinc-200">{file.filename}</span>
              <span className="text-zinc-500 dark:text-zinc-400">{t('manualSize').replace('{bytes}', file.bytes.toLocaleString())}</span>
            </div>
            {file.url && <Address label="Hugging Face" url={file.url} />}
            {mirror && file.mirror_url && <Address label="HF-Mirror" url={file.mirror_url} />}
          </div>
        ))}
        {folder && (
          <div className="flex items-start gap-2 border-t border-zinc-200 pt-2 dark:border-white/10">
            <span className="w-24 shrink-0 text-zinc-500 dark:text-zinc-400">{t('manualFolder')}</span>
            <span className="min-w-0 flex-1 break-all font-mono text-zinc-700 dark:text-zinc-200">{folder}</span>
            <CopyButton value={folder} />
          </div>
        )}
      </div>
    </details>
  );
};

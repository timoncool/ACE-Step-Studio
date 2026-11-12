import React, { useRef, useState } from 'react';
import { FileUp, Loader2, PencilLine } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { karaokeReason } from '../services/karaoke';
import { Song } from '../types';
import { ConfirmDialog } from './ConfirmDialog';

const LINE = /^\[(\d{2}:\d{2}\.\d{2})\](.*)$/;
const BUTTON = 'flex-1 flex items-center justify-center gap-2 px-3 py-2 rounded-xl bg-zinc-100 dark:bg-white/5 hover:bg-zinc-200 dark:hover:bg-white/10 text-zinc-600 dark:text-zinc-400 text-xs font-medium transition-colors disabled:opacity-50';

interface Row { raw: string; time: string; text: string; start: string; words: string }

/** The karaoke's lines as rows to edit: the start and the text, word times aside. */
export function karaokeRows(lrc: string): Row[] {
  return lrc.split('\n').flatMap(raw => {
    const match = raw.trim().match(LINE);
    if (!match) return [];
    const text = match[2].replace(/<\d{2}:\d{2}\.\d{2}>/g, '').replace(/\s+/g, ' ').trim();
    return [{ raw: raw.trim(), time: match[1], text, start: match[1], words: text }];
  });
}

/** The rows back as LRC: a row left alone keeps its line as it was, the times of its words with it. */
export function karaokeFromRows(rows: Row[]): string {
  return rows.map(row => (row.time === row.start && row.text === row.words ? row.raw : `[${row.time}]${row.text.trim()}`)).join('\n');
}

/** A song's karaoke from the person's own subtitles, or its lines edited by hand. */
export const KaraokeFile: React.FC<{ song: Song; onDone: (lrc: string) => void }> = ({ song, onDone }) => {
  const { t } = useI18n();
  const picker = useRef<HTMLInputElement | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [replacing, setReplacing] = useState<File | null>(null);
  const [rows, setRows] = useState<Row[] | null>(null);

  if (!song.audioUrl) return null;
  const hasKaraoke = Boolean(song.lrcContent?.trim());

  const send = async (body: object) => {
    setBusy(true);
    setError(null);
    try {
      const response = await fetch(`/v1/library/songs/${encodeURIComponent(song.id)}/karaoke`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      const answer = await response.json().catch(() => null);
      if (!response.ok) throw new Error(answer?.error || String(response.status));
      onDone(answer.lrc as string);
      return true;
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : String(reason);
      setError(karaokeReason(t, message) ?? message);
      return false;
    } finally {
      setBusy(false);
    }
  };

  const load = async (file: File) => send({ content: await file.text(), file_name: file.name });

  const save = async () => {
    if (!rows) return;
    if (rows.some(row => !/^\d{2}:\d{2}\.\d{2}$/.test(row.time))) {
      setError(t('karaokeEditBadTime'));
      return;
    }
    if (await send({ lrc: karaokeFromRows(rows) })) setRows(null);
  };

  return (
    <div className="space-y-1">
      <div className="flex gap-2">
        <button type="button" onClick={() => picker.current?.click()} disabled={busy} className={BUTTON} title={t('karaokeLoadFileHint')}>
          {busy && !rows ? <Loader2 size={14} className="animate-spin" /> : <FileUp size={14} />} {t('karaokeLoadFile')}
        </button>
        {hasKaraoke && (
          <button type="button" onClick={() => { setError(null); setRows(rows ? null : karaokeRows(song.lrcContent ?? '')); }} disabled={busy} className={BUTTON} aria-expanded={rows !== null}>
            <PencilLine size={14} /> {t('karaokeEdit')}
          </button>
        )}
      </div>
      <input
        ref={picker}
        type="file"
        accept=".srt,.lrc,.ass,.ssa"
        className="hidden"
        onChange={event => {
          const file = event.target.files?.[0];
          event.target.value = '';
          if (!file) return;
          if (hasKaraoke) setReplacing(file);
          else void load(file);
        }}
      />
      {rows && (
        <div className="space-y-1.5 rounded-xl border border-zinc-200 p-2 dark:border-white/10">
          <div className="max-h-[calc(var(--vh)*40)] space-y-1 overflow-y-auto">
            {rows.map((row, index) => (
              <div key={index} className="flex gap-1.5">
                <input
                  value={row.time}
                  onChange={event => setRows(current => current && current.map((other, at) => (at === index ? { ...other, time: event.target.value } : other)))}
                  aria-label={t('karaokeEditTime')}
                  className="w-20 shrink-0 rounded-md border border-zinc-200 bg-white px-1.5 py-1 font-mono text-[11px] dark:border-white/10 dark:bg-black/30"
                />
                <input
                  value={row.text}
                  onChange={event => setRows(current => current && current.map((other, at) => (at === index ? { ...other, text: event.target.value } : other)))}
                  aria-label={t('karaokeEditText')}
                  className="min-w-0 flex-1 rounded-md border border-zinc-200 bg-white px-1.5 py-1 text-[11px] dark:border-white/10 dark:bg-black/30"
                />
              </div>
            ))}
          </div>
          <div className="flex justify-end gap-2">
            <button type="button" onClick={() => setRows(null)} className="rounded-lg px-3 py-1 text-xs text-zinc-500 hover:text-zinc-900 dark:hover:text-white">{t('cancel')}</button>
            <button type="button" onClick={() => void save()} disabled={busy} className="inline-flex items-center gap-1.5 rounded-lg bg-pink-600 px-3 py-1 text-xs font-bold text-white disabled:opacity-50">
              {busy && <Loader2 size={12} className="animate-spin" />} {t('save')}
            </button>
          </div>
        </div>
      )}
      {error && <p className="text-[11px] text-red-500">{error}</p>}
      <ConfirmDialog
        isOpen={replacing !== null}
        title={t('karaokeLoadFile')}
        message={t('karaokeLoadFileReplace')}
        confirmLabel={t('karaokeLoadFile')}
        onCancel={() => setReplacing(null)}
        onConfirm={() => { const file = replacing; setReplacing(null); if (file) void load(file); }}
      />
    </div>
  );
};

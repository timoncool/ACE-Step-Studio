import React, { useEffect, useState } from 'react';
import { Loader2, X } from 'lucide-react';
import { SiGithub } from '@icons-pack/react-simple-icons';
import { useI18n } from '../context/I18nContext';
import { issueReportUrl, type IssueKind, type ReportFacts } from '../services/issueReport';
import { STUDIO_REPO_URL } from '../studio';

/** The service's refusal, by kind: each has its own words in the window. */
export interface ProblemRefusal {
  code: 'field' | 'rate' | 'not_configured' | 'busy' | 'unreachable';
  field?: string;
  retry_after_s?: number;
  detail?: string;
}

const MIN_WHAT = 10;

/** The labels of the three text fields of each kind, as the GitHub form of that kind names them; a question has one. */
export const FIELDS: Record<IssueKind, [string, string | null, string | null]> = {
  bug: ['problemWhat', 'problemSteps', 'problemExpected'],
  idea: ['problemIdeaWhat', 'problemIdeaHow', 'problemIdeaExamples'],
  question: ['problemQuestion', null, null],
};

/** The text the window says for a refusal; every kind but a field leaves the GitHub form as the way on. */
export function refusalText(refusal: ProblemRefusal, t: (key: string) => string): string {
  switch (refusal.code) {
    case 'field':
      return t('problemErrField').replace('{field}', refusal.field ?? '');
    case 'rate':
      return t('problemErrRate').replace('{min}', String(Math.max(1, Math.ceil((refusal.retry_after_s ?? 60) / 60))));
    case 'not_configured':
      return t('problemErrNotConfigured');
    case 'busy':
      return t('problemErrBusy');
    default:
      return t('problemErrUnreachable').replace('{detail}', refusal.detail ?? '');
  }
}

/** The request as the hub will receive it, the log as one block of text. */
export function sentText(body: Record<string, unknown>): string {
  const { log, ...rest } = body;
  const head = JSON.stringify(rest, null, 2);
  return typeof log === 'string' && log ? `${head}\n\nlog:\n${log}` : head;
}

interface Props {
  facts?: ReportFacts;
  onClose: () => void;
}

/**
 * "Report a problem" without a GitHub account: the studio sends the person's words with the facts it knows, and the
 * hub's bot opens the issue. The GitHub form stays one click away for those who have an account.
 */
export const ProblemReportDialog: React.FC<Props> = ({ facts, onClose }) => {
  const { t, language } = useI18n();
  const tt = t as (key: string) => string;
  const [kind, setKind] = useState<IssueKind>('bug');
  const [what, setWhat] = useState('');
  const [steps, setSteps] = useState('');
  const [expected, setExpected] = useState('');
  const [attachLog, setAttachLog] = useState(true);
  const [whatLabel, stepsLabel, expectedLabel] = FIELDS[kind];
  // the log goes with a bug unless unticked; an idea or a question takes it only when ticked
  const choose = (next: IssueKind) => {
    setKind(next);
    setAttachLog(next === 'bug');
  };
  const [preview, setPreview] = useState<Record<string, unknown> | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [sent, setSent] = useState<{ number: number; url: string } | null>(null);
  const [refusal, setRefusal] = useState<ProblemRefusal | null>(null);
  const request = () => ({ kind, what: what.trim(), steps: stepsLabel ? steps.trim() : '', expected: expectedLabel ? expected.trim() : '', attach_log: attachLog });

  const [previewOpen, setPreviewOpen] = useState(false);
  const previewBody = JSON.stringify(request());

  // what will be sent, as the service builds the request, kept in step with every edit while it is open
  useEffect(() => {
    if (!previewOpen) return;
    const controller = new AbortController();
    setPreview(null);
    setPreviewError(null);
    const timer = window.setTimeout(() => {
      fetch('/v1/problem/preview', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: previewBody, signal: controller.signal })
        .then(async response => {
          const body = await response.json().catch(() => null);
          if (!response.ok) throw new Error(body ? refusalText(body as ProblemRefusal, tt) : `HTTP ${response.status}`);
          setPreview(body as Record<string, unknown>);
        })
        .catch((error: unknown) => {
          if (controller.signal.aborted) return;
          setPreviewError(error instanceof Error ? error.message : String(error));
        });
    }, 300);
    return () => {
      window.clearTimeout(timer);
      controller.abort();
    };
  }, [previewOpen, previewBody]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const send = async () => {
    setSending(true);
    setRefusal(null);
    try {
      const response = await fetch('/v1/problem/report', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(request()),
      });
      const body = await response.json().catch(() => null);
      if (response.ok) setSent(body as { number: number; url: string });
      else setRefusal((body as ProblemRefusal | null) ?? { code: 'unreachable', detail: `HTTP ${response.status}` });
    } catch (error) {
      setRefusal({ code: 'unreachable', detail: error instanceof Error ? error.message : String(error) });
    } finally {
      setSending(false);
    }
  };

  const field = 'w-full rounded-lg border border-zinc-300 bg-white px-3 py-2 text-sm text-zinc-900 dark:border-zinc-600 dark:bg-zinc-800 dark:text-zinc-100';
  // what the person typed goes into the fields of the form, so it is not written twice
  const githubUrl = issueReportUrl(STUDIO_REPO_URL, __APP_VERSION__, language, facts, kind, { what, steps: stepsLabel ? steps : '', expected: expectedLabel ? expected : '' });
  const tooShort = what.trim().length < MIN_WHAT;

  return (
    <div className="fixed inset-0 z-100 flex items-center justify-center bg-black/60 backdrop-blur-xs" onClick={event => { if (event.target === event.currentTarget) onClose(); }}>
      <div role="dialog" aria-label={tt('problemTitle')} className="mx-4 max-h-[90vh] w-full max-w-lg overflow-y-auto rounded-2xl border border-zinc-200 bg-white p-6 shadow-2xl dark:border-white/10 dark:bg-zinc-900">
        <div className="mb-4 flex items-start justify-between gap-3">
          <h3 className="text-base font-semibold text-zinc-900 dark:text-white">{tt('problemTitle')}</h3>
          <button type="button" onClick={onClose} aria-label={tt('close')} className="text-zinc-400 hover:text-zinc-600"><X size={16} /></button>
        </div>
        {sent ? (
          <p className="text-sm text-zinc-700 dark:text-zinc-200">
            {tt('problemSent').replace('{n}', String(sent.number))}{' '}
            <a href={sent.url} target="_blank" rel="noopener noreferrer" className="text-pink-600 underline">{sent.url}</a>
          </p>
        ) : (
          <div className="space-y-3">
            <div role="radiogroup" aria-label={tt('problemKind')} className="flex gap-1">
              {(['bug', 'idea', 'question'] as const).map(option => (
                <button key={option} type="button" role="radio" aria-checked={kind === option} onClick={() => choose(option)}
                        className={`rounded-lg border px-3 py-1 text-xs font-medium ${kind === option ? 'border-pink-500 bg-pink-500 text-white' : 'border-zinc-300 text-zinc-700 dark:border-zinc-600 dark:text-zinc-200'}`}>
                  {tt(`problemKind_${option}`)}
                </button>
              ))}
            </div>
            <label className="block text-xs font-medium text-zinc-600 dark:text-zinc-300">
              <span className="mb-1 block">{tt(whatLabel)} *</span>
              <textarea value={what} onChange={event => setWhat(event.target.value)} rows={4} required className={field} />
              {what.length > 0 && tooShort && <span className="mt-1 block text-amber-600">{tt('problemWhatShort')}</span>}
            </label>
            {stepsLabel && (
              <label className="block text-xs font-medium text-zinc-600 dark:text-zinc-300">
                <span className="mb-1 block">{tt(stepsLabel)}</span>
                <textarea value={steps} onChange={event => setSteps(event.target.value)} rows={3} className={field} />
              </label>
            )}
            {expectedLabel && (
              <label className="block text-xs font-medium text-zinc-600 dark:text-zinc-300">
                <span className="mb-1 block">{tt(expectedLabel)}</span>
                <textarea value={expected} onChange={event => setExpected(event.target.value)} rows={2} className={field} />
              </label>
            )}
            <label className="flex items-center gap-2 text-xs text-zinc-700 dark:text-zinc-200">
              <input type="checkbox" checked={attachLog} onChange={event => setAttachLog(event.target.checked)} />
              {tt('problemAttachLog')}
            </label>
            <details className="rounded-lg border border-zinc-200 p-2 text-xs dark:border-white/10" onToggle={event => setPreviewOpen((event.target as HTMLDetailsElement).open)}>
              <summary className="cursor-pointer text-zinc-600 dark:text-zinc-300">{tt('problemWillSend')}</summary>
              {preview ? (
                <pre className="mt-2 max-h-48 overflow-auto whitespace-pre-wrap text-[11px] text-zinc-600 dark:text-zinc-300">{sentText(preview)}</pre>
              ) : (
                <span className="mt-2 block text-zinc-500">{previewError ?? <Loader2 size={12} className="inline animate-spin" />}</span>
              )}
            </details>
            <p className="text-xs text-zinc-500 dark:text-zinc-400">{tt('problemPublic')}</p>
            {refusal && <p role="alert" className="text-xs text-rose-600 dark:text-rose-400">{refusalText(refusal, tt)}</p>}
          </div>
        )}
        <div className="mt-5 flex flex-wrap justify-end gap-2">
          <a href={githubUrl} target="_blank" rel="noopener noreferrer" className="inline-flex items-center gap-1.5 rounded-lg border border-zinc-300 px-3 py-2 text-sm font-medium text-zinc-700 dark:border-zinc-600 dark:text-zinc-200">
            <SiGithub size={14} />{tt('problemOpenGithub')}
          </a>
          {!sent && (
            <button type="button" onClick={() => void send()} disabled={sending || tooShort} className="inline-flex items-center gap-1.5 rounded-lg bg-pink-600 px-4 py-2 text-sm font-medium text-white hover:bg-pink-700 disabled:opacity-40">
              {sending && <Loader2 size={14} className="animate-spin" />}{tt('problemSend')}
            </button>
          )}
        </div>
      </div>
    </div>
  );
};

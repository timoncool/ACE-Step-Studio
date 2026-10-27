import React, { useCallback, useEffect, useState } from 'react';
import { Bug, Check, Copy, FolderOpen, Loader2, Package, RefreshCw } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { issueReportUrl, type ReportFacts } from '../services/issueReport';
import { SELF_READ, useSetupStatus } from '../services/studioQueries';
import { STUDIO_REPO_URL } from '../studio';

type Diagnostics = { text: string };

const failure = async (response: Response) => {
  const body = await response.json().catch(() => null);
  return body?.error || `HTTP ${response.status}`;
};

const post = async <T,>(path: string, body: unknown = {}): Promise<T> => {
  const response = await fetch(path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
  if (!response.ok) throw new Error(await failure(response));
  return response.json() as Promise<T>;
};

/**
 * The system as the studio sees it (card, driver, CUDA build, models, optional parts, missing DLLs, the latest log),
 * to copy into an issue or to pack with the journal and the logs for a bug report; keys never get in.
 */
export const DiagnosticsPanel: React.FC = () => {
  const { t } = useI18n();
  const setup = useSetupStatus<ReportFacts>();
  const [diagnostics, setDiagnostics] = useState<Diagnostics | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [packing, setPacking] = useState(false);
  const [pack, setPack] = useState<string | null>(null);
  const reason = (error: unknown) => (error instanceof Error ? error.message : String(error));

  const load = useCallback((byItself: boolean) => {
    fetch('/v1/diagnostics', byItself ? { headers: SELF_READ } : undefined)
      .then(async (response) => {
        if (!response.ok) throw new Error(await failure(response));
        return response.json() as Promise<Diagnostics>;
      })
      .then((next) => { setDiagnostics(next); setProblem(null); }, (error: unknown) => setProblem(reason(error)));
  }, []);
  useEffect(() => load(true), [load]);

  const copy = () => {
    if (!diagnostics) return;
    navigator.clipboard.writeText(diagnostics.text).then(
      () => { setCopied(true); window.setTimeout(() => setCopied(false), 1500); },
      (error: unknown) => setProblem(reason(error)),
    );
  };
  const makePack = async () => {
    setPacking(true);
    try {
      setPack((await post<{ path: string }>('/v1/diagnostics/report')).path);
      setProblem(null);
    } catch (error) {
      setProblem(reason(error));
    } finally {
      setPacking(false);
    }
  };
  const reveal = () => { if (pack) post('/v1/diagnostics/report/reveal', { path: pack }).catch((error: unknown) => setProblem(reason(error))); };
  const revealLogs = () => { post('/v1/diagnostics/logs/reveal').catch((error: unknown) => setProblem(reason(error))); };

  const button = 'inline-flex items-center gap-1.5 rounded-lg border border-zinc-200 px-2.5 py-1.5 text-xs font-medium text-zinc-700 hover:border-pink-500 hover:text-pink-500 disabled:opacity-40 dark:border-white/10 dark:text-zinc-200';
  return (
    <section className="space-y-2 rounded-xl border border-zinc-200 p-3 dark:border-white/10" data-settings-part="diagnostics">
      <h4 className="text-[11px] font-bold uppercase tracking-wide text-zinc-500">{t('diagnosticsTitle')}</h4>
      <p className="text-xs leading-5 text-zinc-500 dark:text-zinc-400">{t('diagnosticsHint')}</p>
      <div className="flex flex-wrap gap-2">
        <button type="button" onClick={copy} disabled={!diagnostics} className={button}>
          {copied ? <Check size={13} className="text-emerald-500" /> : <Copy size={13} />}{copied ? t('diagnosticsCopied') : t('diagnosticsCopy')}
        </button>
        <button type="button" onClick={() => void makePack()} disabled={packing} className={button}>
          {packing ? <Loader2 size={13} className="animate-spin" /> : <Package size={13} />}{t('diagnosticsPack')}
        </button>
        <a href={issueReportUrl(STUDIO_REPO_URL, __APP_VERSION__, setup.data)} target="_blank" rel="noopener noreferrer" className={button}>
          <Bug size={13} />{t('diagnosticsReport')}
        </a>
        <button type="button" onClick={revealLogs} className={button}><FolderOpen size={13} />{t('diagnosticsLogs')}</button>
        <button type="button" onClick={() => load(false)} className={button}><RefreshCw size={13} />{t('diagnosticsRefresh')}</button>
      </div>
      {pack && (
        <p className="flex flex-wrap items-center gap-2 text-[11px]">
          <span className="text-zinc-500">{t('diagnosticsPacked')}</span>
          <span className="break-all font-mono">{pack}</span>
          <button type="button" onClick={reveal} className="inline-flex items-center gap-1 text-zinc-500 underline hover:text-pink-500"><FolderOpen size={12} />{t('diagnosticsShow')}</button>
        </p>
      )}
      {problem && <p role="alert" className="break-words font-mono text-[11px] text-rose-600 dark:text-rose-300">{problem}</p>}
      {diagnostics
        ? <pre className="max-h-72 overflow-auto whitespace-pre-wrap break-words rounded-lg border border-zinc-200 bg-zinc-50 p-2 font-mono text-[11px] leading-snug text-zinc-600 dark:border-white/10 dark:bg-black/30 dark:text-zinc-400">{diagnostics.text}</pre>
        : !problem && <Loader2 size={14} className="animate-spin text-zinc-400" />}
    </section>
  );
};

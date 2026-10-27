import React, { useEffect, useState } from 'react';
import { Check, Globe, Loader2 } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { SELF_READ } from '../services/studioQueries';

type ProxyForm = { mode: string; kind: string; address: string | null; huggingface_mirror: boolean };
type Probe = { huggingface: boolean; huggingface_error?: string | null; mirror_chosen: boolean; mirror: boolean | null; mirror_error?: string | null };

const failure = async (response: Response) => {
  const body = await response.json().catch(() => null);
  return body?.error || `Request failed (${response.status})`;
};

/**
 * The start screen asks whether Hugging Face answers through the studio's
 * proxy settings; when it does not and HF-Mirror does, it offers the mirror.
 * Nothing switches by itself: the mirror is a third party, so the user picks it.
 * `recheck` changes when a download fails, and the question is asked again.
 */
export const MirrorOffer: React.FC<{ recheck: string; onSwitched: () => void }> = ({ recheck, onSwitched }) => {
  const { t } = useI18n();
  const [form, setForm] = useState<ProxyForm | null>(null);
  const [probe, setProbe] = useState<Probe | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [switching, setSwitching] = useState(false);
  const [switched, setSwitched] = useState(false);

  useEffect(() => {
    let alive = true;
    setProblem(null);
    void (async () => {
      const current = await fetch('/v1/network/proxy', { headers: SELF_READ });
      if (!current.ok) throw new Error(await failure(current));
      const { mode, kind, address, huggingface_mirror } = (await current.json()) as ProxyForm;
      const settings: ProxyForm = { mode, kind, address, huggingface_mirror: huggingface_mirror === true };
      if (!alive) return;
      setForm(settings);
      if (settings.huggingface_mirror) {
        setProbe(null);
        return;
      }
      const tested = await fetch('/v1/network/proxy/test', {
        method: 'POST',
        headers: { ...SELF_READ, 'Content-Type': 'application/json' },
        body: JSON.stringify(settings),
      });
      if (!tested.ok) throw new Error(await failure(tested));
      const answer = (await tested.json()) as Probe;
      if (alive) setProbe(answer);
    })().catch((reason) => {
      console.error('[ERROR] the Hugging Face check failed:', reason);
      if (alive) setProblem(reason instanceof Error ? reason.message : String(reason));
    });
    return () => { alive = false; };
  }, [recheck]);

  const takeMirror = async () => {
    if (!form || switching) return;
    setSwitching(true);
    setProblem(null);
    try {
      const response = await fetch('/v1/network/proxy', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ ...form, huggingface_mirror: true }),
      });
      if (!response.ok) throw new Error(await failure(response));
      setSwitched(true);
      setProbe(null);
      onSwitched();
    } catch (reason) {
      console.error('[ERROR] HF-Mirror was not switched on:', reason);
      setProblem(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setSwitching(false);
    }
  };

  if (switched) {
    return (
      <div className="mt-5 flex gap-2 rounded-xl border border-emerald-300 bg-emerald-50 px-4 py-3 text-sm text-emerald-800 dark:border-emerald-500/30 dark:bg-emerald-500/10 dark:text-emerald-200">
        <Check size={16} className="mt-0.5 shrink-0" />{t('mirrorOfferOn')}
      </div>
    );
  }
  if (problem) {
    return (
      <div className="mt-5 rounded-xl border border-amber-300 bg-amber-50 px-4 py-3 text-sm text-amber-800 dark:border-amber-500/30 dark:bg-amber-500/10 dark:text-amber-200">
        {t('mirrorOfferCheckFailed').replace('{error}', problem)}
      </div>
    );
  }
  if (!probe || probe.huggingface || probe.mirror_chosen) return null;
  return (
    <div className="mt-5 rounded-xl border border-amber-300 bg-amber-50 px-4 py-3 text-sm text-zinc-700 dark:border-amber-500/30 dark:bg-amber-500/10 dark:text-zinc-200">
      <div className="flex items-center gap-2 font-semibold text-amber-800 dark:text-amber-200">
        <Globe size={16} className="shrink-0" />{t('mirrorOfferTitle')}
      </div>
      {probe.huggingface_error && <p className="mt-1 break-words text-xs text-zinc-500 dark:text-zinc-400">{probe.huggingface_error}</p>}
      {probe.mirror ? (
        <>
          <p className="mt-2 leading-6">{t('mirrorOfferBody')}</p>
          <button
            type="button"
            onClick={() => void takeMirror()}
            disabled={switching}
            className="mt-3 inline-flex items-center gap-2 rounded-lg bg-pink-500 px-3 py-1.5 text-sm font-semibold text-white transition-colors hover:bg-pink-600 disabled:opacity-60"
          >
            {switching ? <Loader2 size={14} className="animate-spin" /> : <Globe size={14} />}
            {t('mirrorOfferButton')}
          </button>
        </>
      ) : (
        <p className="mt-2 leading-6">{t('mirrorOfferNone').replace('{error}', probe.mirror_error ?? '')}</p>
      )}
    </div>
  );
};

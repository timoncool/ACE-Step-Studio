import React, { useEffect, useState } from 'react';
import { AlertTriangle, Check, Eye, EyeOff, Globe, Loader2, X } from 'lucide-react';
import { useI18n } from '../context/I18nContext';

/**
 * The proxy for everything the studio fetches from the internet.
 *
 * The service routes each request by what is saved here, at once and without a
 * restart; the address is written in whatever form the proxy seller gave it,
 * and the service reads it. "Check" asks Hugging Face and OpenRouter through
 * the proxy on the form before anything is saved.
 */

type Mode = 'system' | 'custom' | 'off';
type Kind = 'http' | 'https' | 'socks5' | 'socks4';

interface Settings {
  mode: Mode;
  address: string | null;
  kind: Kind;
}

interface Probe {
  huggingface: boolean;
  huggingface_error: string | null;
  openrouter: boolean;
  openrouter_error: string | null;
}

const CONTROL =
  'w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:border-pink-500 disabled:opacity-50 dark:border-white/10 dark:bg-black/20 dark:text-white';

const KINDS: { id: Kind; label: string }[] = [
  { id: 'http', label: 'HTTP' },
  { id: 'https', label: 'HTTPS' },
  { id: 'socks5', label: 'SOCKS5' },
  { id: 'socks4', label: 'SOCKS4' },
];

export const ProxySettings: React.FC = () => {
  const { t } = useI18n();
  const [mode, setMode] = useState<Mode>('system');
  const [kind, setKind] = useState<Kind>('http');
  const [address, setAddress] = useState('');
  const [reveal, setReveal] = useState(false);
  const [busy, setBusy] = useState<'test' | 'save' | null>(null);
  const [probe, setProbe] = useState<Probe | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    void fetch('/v1/network/proxy')
      .then(response => (response.ok ? response.json() : Promise.reject(new Error(String(response.status)))))
      .then((body: Settings) => {
        setMode(body.mode);
        setKind(body.kind);
        setAddress(body.address ?? '');
      })
      .catch(reason => setError(reason instanceof Error ? reason.message : String(reason)));
  }, []);

  const form = (): Settings => ({ mode, kind, address: address.trim() || null });

  const send = async (path: string, method: 'PUT' | 'POST') => {
    const response = await fetch(path, {
      method,
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(form()),
    });
    const body = await response.json().catch(() => null);
    if (!response.ok) throw new Error(body?.error || String(response.status));
    return body;
  };

  const check = async () => {
    setBusy('test');
    setError(null);
    setProbe(null);
    setSaved(false);
    try {
      setProbe(await send('/v1/network/proxy/test', 'POST'));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(null);
    }
  };

  const save = async () => {
    setBusy('save');
    setError(null);
    setSaved(false);
    try {
      await send('/v1/network/proxy', 'PUT');
      setSaved(true);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(null);
    }
  };

  const modes: { id: Mode; label: string }[] = [
    { id: 'system', label: t('proxyModeSystem') },
    { id: 'custom', label: t('proxyModeCustom') },
    { id: 'off', label: t('proxyModeOff') },
  ];

  const target = (name: string, ok: boolean, detail: string | null) => (
    <div className="flex items-start gap-2 text-xs">
      {ok ? <Check size={14} className="mt-0.5 shrink-0 text-emerald-500" /> : <X size={14} className="mt-0.5 shrink-0 text-rose-500" />}
      <span className="min-w-0">
        <span className="font-semibold text-zinc-800 dark:text-zinc-100">{name}</span>{' '}
        <span className={ok ? 'text-emerald-600 dark:text-emerald-300' : 'text-rose-600 dark:text-rose-300'}>{ok ? t('proxyReachable') : t('proxyUnreachable')}</span>
        {!ok && detail && <span className="block break-words text-[11px] text-zinc-500">{detail}</span>}
      </span>
    </div>
  );

  return (
    <section className="rounded-xl border border-zinc-200 p-4 dark:border-white/10">
      <h4 className="flex items-center gap-2 text-sm font-semibold text-zinc-900 dark:text-white">
        <Globe size={16} className="text-pink-500" /> {t('proxyTitle')}
      </h4>
      <p className="mt-1 text-xs leading-5 text-zinc-500 dark:text-zinc-400">{t('proxyIntro')}</p>

      <div className="mt-3 flex w-fit rounded-lg border border-zinc-200 p-0.5 dark:border-white/10">
        {modes.map(choice => (
          <button
            key={choice.id}
            type="button"
            onClick={() => { setMode(choice.id); setProbe(null); setSaved(false); }}
            className={`rounded-md px-3 py-1 text-xs font-semibold transition-colors ${mode === choice.id ? 'bg-zinc-900 text-white dark:bg-white dark:text-zinc-900' : 'text-zinc-500'}`}
          >
            {choice.label}
          </button>
        ))}
      </div>

      {mode === 'custom' && (
        <div className="mt-3 space-y-3">
          <div>
            <span className="mb-1 block text-[11px] font-semibold uppercase tracking-wide text-zinc-500">{t('proxyKind')}</span>
            <div className="flex w-fit rounded-lg border border-zinc-200 p-0.5 dark:border-white/10">
              {KINDS.map(choice => (
                <button
                  key={choice.id}
                  type="button"
                  onClick={() => { setKind(choice.id); setProbe(null); setSaved(false); }}
                  className={`rounded-md px-3 py-1 text-xs font-semibold transition-colors ${kind === choice.id ? 'bg-zinc-900 text-white dark:bg-white dark:text-zinc-900' : 'text-zinc-500'}`}
                >
                  {choice.label}
                </button>
              ))}
            </div>
            <p className="mt-1 text-[11px] leading-4 text-zinc-500">{t('proxyKindHint')}</p>
          </div>
          <div>
            <span className="mb-1 block text-[11px] font-semibold uppercase tracking-wide text-zinc-500">{t('proxyAddress')}</span>
            <div className="relative">
              <input
                type={reveal ? 'text' : 'password'}
                value={address}
                onChange={event => { setAddress(event.target.value); setProbe(null); setSaved(false); }}
                placeholder={t('proxyAddressPlaceholder')}
                autoComplete="off"
                spellCheck={false}
                className={`${CONTROL} pr-10 font-mono`}
              />
              <button
                type="button"
                onClick={() => setReveal(value => !value)}
                className="absolute right-2 top-1/2 -translate-y-1/2 text-zinc-400 hover:text-pink-500"
              >
                {reveal ? <EyeOff size={15} /> : <Eye size={15} />}
              </button>
            </div>
            <p className="mt-1 text-[11px] leading-4 text-zinc-500">{t('proxyAddressHint')}</p>
          </div>
        </div>
      )}

      <div className="mt-3 flex flex-wrap items-center gap-2">
        <button
          type="button"
          onClick={() => void check()}
          disabled={busy !== null || mode === 'off'}
          className="inline-flex items-center gap-2 rounded-lg border border-zinc-300 px-3 py-2 text-xs font-semibold text-zinc-700 hover:border-pink-400 hover:text-pink-600 disabled:opacity-50 dark:border-white/15 dark:text-zinc-200"
        >
          {busy === 'test' && <Loader2 size={13} className="animate-spin" />} {t('proxyTest')}
        </button>
        <button
          type="button"
          onClick={() => void save()}
          disabled={busy !== null}
          className="rounded-lg bg-gradient-to-r from-orange-500 to-pink-600 px-4 py-2 text-xs font-bold text-white disabled:opacity-50"
        >
          {busy === 'save' ? <Loader2 size={14} className="animate-spin" /> : t('proxySave')}
        </button>
        {saved && (
          <span className="flex items-center gap-1.5 text-xs text-emerald-600 dark:text-emerald-300">
            <Check size={14} /> {t('proxySaved')}
          </span>
        )}
      </div>

      {probe && (
        <div className="mt-3 space-y-1.5 rounded-lg bg-zinc-50 p-3 dark:bg-black/20">
          {target('Hugging Face', probe.huggingface, probe.huggingface_error)}
          {target('OpenRouter', probe.openrouter, probe.openrouter_error)}
        </div>
      )}
      {error && (
        <p role="alert" className="mt-3 flex items-center gap-2 rounded-lg bg-rose-500/10 px-3 py-2 text-xs text-rose-700 dark:text-rose-300">
          <AlertTriangle size={14} className="shrink-0" /> {error}
        </p>
      )}
      <p className="mt-3 text-[11px] leading-4 text-zinc-500">{t('proxyWindowNote')}</p>
    </section>
  );
};

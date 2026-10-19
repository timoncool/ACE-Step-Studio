import React, { useEffect, useState } from 'react';
import { Check, Loader2 } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { apiUrl } from '../services/apiBase';
import { assistantStatusChanged } from '../services/studioQueries';
import { CLI_SIGNIN, explainCliFailure } from '../services/cliFailure';

interface CliAgent {
  id: string;
  display_name: string;
  installed: boolean;
  path: string | null;
  version: string | null;
  default_model: string | null;
  features: string[];
  suggested_models: string[];
  install: string;
  signin: string;
  docs: string | null;
}

interface AssistantStatus {
  provider?: string;
  cli_agent?: string | null;
  cli_model?: string | null;
  cli_path?: string | null;
}

/** CLIs that are not installed by a command. */
const INSTALL_PROSE: Record<string, 'cliInstallAgy' | 'cliInstallMuse'> = {
  antigravity: 'cliInstallAgy',
  muse: 'cliInstallMuse',
};

const INPUT =
  'w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-hidden focus:border-pink-400 dark:border-white/10 dark:bg-black/20 dark:text-white';

export const CliAgentPanel: React.FC = () => {
  const { t } = useI18n();
  const [agents, setAgents] = useState<CliAgent[] | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [failed, setFailed] = useState<string | null>(null);
  const [provider, setProvider] = useState('');
  const [chosen, setChosen] = useState('');
  const [model, setModel] = useState('');
  const [path, setPath] = useState('');
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ tone: 'ok' | 'error'; text: string } | null>(null);

  useEffect(() => {
    let alive = true;
    const load = async () => {
      try {
        const [agentsResponse, statusResponse] = await Promise.all([fetch(apiUrl('/v1/assistant/cli-agents')), fetch(apiUrl('/v1/assistant/status'))]);
        if (!agentsResponse.ok) throw new Error(`HTTP ${agentsResponse.status}`);
        const body = (await agentsResponse.json()) as { agents?: CliAgent[] };
        const status = statusResponse.ok ? ((await statusResponse.json()) as AssistantStatus) : null;
        if (!alive) return;
        setAgents(body.agents ?? []);
        setProvider(status?.provider ?? '');
        setChosen(status?.cli_agent ?? '');
        setModel(status?.cli_model ?? '');
        setPath(status?.cli_path ?? '');
        setLoaded(true);
        setFailed(null);
      } catch (error) {
        if (alive) setFailed(error instanceof Error ? error.message : String(error));
      }
    };
    void load();
    return () => {
      alive = false;
    };
  }, []);

  const save = React.useCallback((cliAgent: string, cliModel: string, cliPath: string) => {
    void fetch(apiUrl('/v1/assistant/status'), {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        provider: 'cli',
        cli_agent: cliAgent || null,
        // Null keeps the stored value, so clearing a field means sending it empty.
        cli_model: cliModel.trim(),
        cli_path: cliPath.trim(),
      }),
    }).then(assistantStatusChanged, () => undefined);
  }, []);

  useEffect(() => {
    if (!loaded || provider !== 'cli') return;
    const timer = window.setTimeout(() => save(chosen, model, path), 500);
    return () => window.clearTimeout(timer);
  }, [loaded, provider, chosen, model, path, save]);

  const choose = (id: string) => {
    setChosen(id);
    setProvider('cli');
    setTestResult(null);
    save(id, model, path);
  };

  // the server answers with the first installed CLI when none is chosen
  const effective = chosen || agents?.find((candidate) => candidate.installed)?.id || '';

  const explain = (raw: string): string => explainCliFailure(raw, t, effective);

  const signinHow = (id: string) => {
    const [before, after] = t('cliSigninHow').split('{command}');
    return (
      <>
        {before}
        <code className="rounded bg-zinc-100 px-1 py-0.5 font-mono text-[11px] dark:bg-black/30">{CLI_SIGNIN[id] ?? id}</code>
        {after}
      </>
    );
  };

  const test = async () => {
    const agent = agents?.find((candidate) => candidate.id === effective && candidate.installed);
    if (!agent) {
      setTestResult({ tone: 'error', text: t('cliTestNeedsChoice') });
      return;
    }
    setTesting(true);
    setTestResult(null);
    try {
      await fetch(apiUrl('/v1/assistant/status'), {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ provider: 'cli', cli_agent: agent.id, cli_model: model.trim(), cli_path: path.trim() }),
      }).then(assistantStatusChanged, () => undefined);
      const response = await fetch(apiUrl('/v1/assistant/write'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ target: 'style', description: 'A cheerful acoustic pop song about a summer road trip' }),
      });
      const body = await response.json().catch(() => null);
      if (!response.ok) throw new Error(explain(body?.error || `HTTP ${response.status}`));
      const style = typeof body?.style === 'string' ? body.style : JSON.stringify(body);
      setTestResult({ tone: 'ok', text: style });
    } catch (error) {
      setTestResult({ tone: 'error', text: explain(error instanceof Error ? error.message : String(error)) });
    } finally {
      setTesting(false);
    }
  };

  const dot = (on: boolean) => <span className={`h-2 w-2 shrink-0 rounded-full ${on ? 'bg-emerald-500' : 'bg-zinc-400'}`} />;

  return (
    <div className="max-w-2xl space-y-4">
      <p className="text-sm leading-6 text-zinc-600 dark:text-zinc-300">{t('cliIntro')}</p>
      {failed && <div className="text-sm text-red-600 dark:text-red-300">{failed}</div>}

      {agents !== null && agents.every((agent) => !agent.installed) && (
        <div className="rounded-xl border border-zinc-200 p-3 text-sm text-zinc-600 dark:border-white/10 dark:text-zinc-300">{t('cliNone')}</div>
      )}

      <div className="space-y-2">
        {(agents ?? []).map((agent) => {
          const selected = provider === 'cli' && effective === agent.id;
          return (
            <div key={agent.id} className={`rounded-xl border p-3 ${selected ? 'border-pink-400 dark:border-pink-500/60' : 'border-zinc-200 dark:border-white/10'}`}>
              <div className="flex items-center gap-2">
                {agent.installed ? (
                  <button
                    type="button"
                    onClick={() => choose(agent.id)}
                    className={`flex h-5 w-5 items-center justify-center rounded-full border ${selected ? 'border-pink-500 bg-pink-500 text-white' : 'border-zinc-300 text-transparent dark:border-white/20'}`}
                    aria-label={agent.display_name}
                  >
                    <Check size={12} />
                  </button>
                ) : (
                  dot(false)
                )}
                <span className="text-sm font-semibold text-zinc-900 dark:text-white">{agent.display_name}</span>
                {agent.installed ? (
                  <span className="text-xs text-emerald-600 dark:text-emerald-300">
                    {t('installed')} · {t('version')} {agent.version}
                  </span>
                ) : (
                  <span className="text-xs text-zinc-500">{t('cliNotInstalled')}</span>
                )}
                {selected && <span className="text-xs font-medium text-pink-600 dark:text-pink-300">{t('cliSelected')}</span>}
              </div>

              {agent.installed ? (
                <div className="mt-1.5 space-y-1 pl-7 text-xs leading-5 text-zinc-500 dark:text-zinc-400">
                  <div className="break-all font-mono text-[11px]">{agent.path}</div>
                  {agent.default_model && <div>{t('cliModel')}: {agent.default_model}</div>}
                  <div>{t('cliSignin')}: {signinHow(agent.id)}</div>
                </div>
              ) : (
                <div className="mt-1.5 space-y-1 pl-7 text-xs leading-5 text-zinc-500 dark:text-zinc-400">
                  {INSTALL_PROSE[agent.id] ? (
                    <div>{t(INSTALL_PROSE[agent.id])}</div>
                  ) : (
                    <div>
                      {t('cliInstallLabel')}: <code className="rounded bg-zinc-100 px-1 py-0.5 font-mono text-[11px] dark:bg-black/30">{agent.install}</code>
                    </div>
                  )}
                  {agent.docs && (
                    <div>
                      <a href={agent.docs} target="_blank" rel="noreferrer" className="underline underline-offset-2 hover:text-zinc-900 dark:hover:text-white">
                        {t('cliDocs')}
                      </a>
                    </div>
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>

      <div className="space-y-2">
        <div>
          <label className="block text-xs font-medium text-zinc-500 dark:text-zinc-400">{t('cliModel')}</label>
          <input
            value={model}
            onChange={(event) => setModel(event.target.value)}
            placeholder={agents?.find((agent) => agent.id === effective)?.default_model ?? ''}
            list="cli-suggested-models"
            className={`${INPUT} mt-1`}
          />
          <datalist id="cli-suggested-models">
            {(agents?.find((agent) => agent.id === effective)?.suggested_models ?? []).map((suggestion) => (
              <option key={suggestion} value={suggestion} />
            ))}
          </datalist>
        </div>
        <div>
          <label className="block text-xs font-medium text-zinc-500 dark:text-zinc-400">{t('cliPath')}</label>
          <input value={path} onChange={(event) => setPath(event.target.value)} placeholder="" className={`${INPUT} mt-1`} />
        </div>
      </div>

      <div>
        <button
          type="button"
          onClick={() => void test()}
          disabled={testing}
          className="inline-flex items-center gap-2 rounded-xl bg-linear-to-r from-orange-500 to-pink-600 px-5 py-2.5 text-sm font-bold text-white shadow-lg transition-all hover:brightness-110 active:scale-[0.98] disabled:cursor-not-allowed disabled:opacity-50"
        >
          {testing && <Loader2 size={16} className="animate-spin" />}
          {testing ? t('cliTesting') : t('cliTest')}
        </button>
        {testResult && (
          <div className={`mt-2 whitespace-pre-wrap rounded-xl border p-3 text-sm leading-6 ${testResult.tone === 'ok' ? 'border-emerald-300 text-zinc-800 dark:border-emerald-700 dark:text-zinc-200' : 'border-red-300 text-red-700 dark:border-red-700 dark:text-red-300'}`}>
            {testResult.text}
          </div>
        )}
      </div>
    </div>
  );
};

import React, { useEffect, useState } from 'react';
import { Check, Copy } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { apiUrl } from '../services/apiBase';
import { STUDIO } from '../studio';

/**
 * The studio's MCP server, for the user connecting an agent to it: whether
 * the window and an agent are connected, the address, and what to paste into
 * Claude Code or any other client. The server runs with the studio itself.
 */

const SERVER_NAME = STUDIO.slug;

interface McpStatus {
  window_open: boolean;
  agent_connected: boolean;
  agent_last_call: string | null;
  agent_seconds_ago: number | null;
  requests_waiting: number;
}

function endpoint(): string {
  const url = apiUrl('/mcp');
  return url.startsWith('/') ? `${window.location.origin}${url}` : url;
}

const CopyField: React.FC<{ label: string; value: string; multiline?: boolean }> = ({ label, value, multiline }) => {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    await navigator.clipboard.writeText(value);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };
  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs font-medium text-zinc-500 dark:text-zinc-400">{label}</span>
        <button
          type="button"
          onClick={() => void copy().catch((error: Error) => console.error('[ERROR] copy failed:', error))}
          className="inline-flex items-center gap-1 rounded-md border border-zinc-200 px-2 py-1 text-[11px] font-medium text-zinc-600 transition-colors hover:border-pink-400 hover:text-pink-600 dark:border-white/10 dark:text-zinc-300"
        >
          {copied ? <Check size={12} /> : <Copy size={12} />}
          {copied ? t('agentCopied') : t('agentCopy')}
        </button>
      </div>
      <pre className={`overflow-x-auto rounded-lg border border-zinc-200 bg-zinc-50 p-2 font-mono text-[11px] text-zinc-800 dark:border-white/10 dark:bg-black/30 dark:text-zinc-200 ${multiline ? 'whitespace-pre' : 'whitespace-nowrap'}`}>{value}</pre>
    </div>
  );
};

export const AgentPanel: React.FC = () => {
  const { t } = useI18n();
  const [status, setStatus] = useState<McpStatus | null>(null);
  const [failed, setFailed] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    const load = () =>
      fetch(apiUrl('/mcp/status'))
        .then(async (response) => {
          if (!response.ok) throw new Error(`HTTP ${response.status}`);
          return response.json() as Promise<McpStatus>;
        })
        .then((body) => {
          if (!alive) return;
          setStatus(body);
          setFailed(null);
        })
        .catch((error: Error) => alive && setFailed(error.message));
    void load();
    const timer = window.setInterval(() => void load(), 3000);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, []);

  const url = endpoint();
  const config = JSON.stringify({ mcpServers: { [SERVER_NAME]: { type: 'streamable-http', url } } }, null, 2);
  const dot = (on: boolean) => <span className={`h-2 w-2 shrink-0 rounded-full ${on ? 'bg-emerald-500' : 'bg-zinc-400'}`} />;

  return (
    <div className="max-w-2xl space-y-4">
      <p className="text-sm leading-6 text-zinc-600 dark:text-zinc-300">{t('agentIntro')}</p>

      <div className="space-y-1.5 rounded-xl border border-zinc-200 p-3 text-xs dark:border-white/10">
        <div className="flex items-center gap-2 text-zinc-700 dark:text-zinc-200">
          {dot(Boolean(status?.agent_connected))}
          {status?.agent_last_call
            ? `${t('agentSeen')}: ${status.agent_last_call} · ${status.agent_seconds_ago ?? 0} ${t('agentSecondsAgo')}`
            : t('agentNone')}
        </div>
        <div className="flex items-center gap-2 text-zinc-700 dark:text-zinc-200">
          {dot(Boolean(status?.window_open))}
          {status?.window_open ? t('agentWindowOn') : t('agentWindowOff')}
        </div>
        {Boolean(status?.requests_waiting) && (
          <div className="flex items-center gap-2 text-pink-600 dark:text-pink-300">
            {dot(true)}
            {t('agentRequests')}: {status?.requests_waiting}
          </div>
        )}
        {failed && <div className="text-red-600 dark:text-red-300">{failed}</div>}
      </div>

      <CopyField label={t('agentAddress')} value={url} />
      <CopyField label="Claude Code" value={`claude mcp add --transport http ${SERVER_NAME} ${url}`} />
      <CopyField label={t('agentConfig')} value={config} multiline />
      <p className="text-xs leading-5 text-zinc-500 dark:text-zinc-400">{t('agentAssistantHint')}</p>
    </div>
  );
};

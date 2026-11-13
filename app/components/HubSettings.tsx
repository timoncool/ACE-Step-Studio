import React, { useState } from 'react';
import { useI18n } from '../context/I18nContext';
import { hubStateChanged, useHubState } from '../services/studioQueries';
import { refreshHub, resetInstall, setTelemetry } from '../services/studioHub';
import { HubTelemetryPreview } from './HubTelemetryPreview';

/**
 * When the news were last fetched, the hub's address left out: the window never prints it. Before the first fetch,
 * that the studio shows the news of its release.
 */
export function hubFeedLine(data: { source?: string | null; fetchedAt?: number | null } | null | undefined, language: string, t: (key: string) => string): string {
  if (!data?.source) return t('hubFeedNever');
  return `${t('hubFeedUpdated')} ${new Date((data.fetchedAt ?? 0) * 1000).toLocaleString(language)}`;
}

/** Settings: the telemetry switch, what is sent, a new install id, and when the news were last fetched. */
export const HubSettings: React.FC = () => {
  const { t, language } = useI18n();
  const hub = useHubState(language);
  const [preview, setPreview] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const telemetry = hub.data?.telemetry;
  const run = (action: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    action()
      .catch((failure: Error) => setError(failure.message))
      .finally(() => {
        setBusy(false);
        hubStateChanged();
      });
  };
  return (
    <div className="space-y-5 text-sm">
      <section className="space-y-3">
        <h3 className="font-semibold">{t('hubTelemetryTitle')}</h3>
        <p className="text-zinc-500 dark:text-zinc-400">{t('hubTelemetryWhy')}</p>
        {telemetry?.disabledByEnv ? (
          <p className="text-amber-600 dark:text-amber-400">{t('hubTelemetryByEnv')}</p>
        ) : (
          <label className="flex cursor-pointer items-center gap-3">
            <input type="checkbox" className="h-4 w-4 accent-pink-500" checked={Boolean(telemetry?.enabled)} disabled={!telemetry || busy}
                   onChange={(event) => run(() => setTelemetry(event.target.checked, true))} />
            <span>{t('hubTelemetryCheckbox')}</span>
          </label>
        )}
        <div className="flex flex-wrap gap-2">
          <button type="button" onClick={() => setPreview(true)} className="rounded-lg bg-zinc-100 px-3 py-1.5 font-medium hover:bg-zinc-200 dark:bg-white/10 dark:hover:bg-white/15">{t('hubTelemetryWhat')}</button>
          <button type="button" onClick={() => window.open('https://timoncool.github.io/ACE-Step-Studio/stats.html', '_blank', 'noreferrer')} className="rounded-lg bg-zinc-100 px-3 py-1.5 font-medium hover:bg-zinc-200 dark:bg-white/10 dark:hover:bg-white/15">{t('hubTelemetryStats')}</button>
          {telemetry?.install && (
            <button type="button" disabled={busy} onClick={() => run(resetInstall)} className="rounded-lg bg-zinc-100 px-3 py-1.5 font-medium hover:bg-zinc-200 dark:bg-white/10 dark:hover:bg-white/15">{t('hubTelemetryReset')}</button>
          )}
        </div>
      </section>
      <section className="space-y-2">
        <p className="text-zinc-500 dark:text-zinc-400">
          {hubFeedLine(hub.data, language, t)}
        </p>
        <button type="button" disabled={busy} onClick={() => run(refreshHub)} className="rounded-lg bg-zinc-100 px-3 py-1.5 font-medium hover:bg-zinc-200 dark:bg-white/10 dark:hover:bg-white/15">{t('hubFeedRefresh')}</button>
      </section>
      {error && <p role="alert" className="text-xs text-red-600 dark:text-red-400">{error}</p>}
      {preview && <HubTelemetryPreview onClose={() => setPreview(false)} />}
    </div>
  );
};

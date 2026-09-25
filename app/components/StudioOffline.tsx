import React, { useEffect, useState } from 'react';
import { PlugZap, RefreshCw } from 'lucide-react';
import { useI18n } from '../context/I18nContext';

/**
 * The studio service is not answering.
 *
 * This is a different thing from an engine that is still coming up, and it
 * used to look the same: closing the application left this window saying
 * "starting the music engine" forever, counting seconds at a process that no
 * longer exists. Nothing here is a wait the user should sit through - the
 * service is gone, and the only true statement is that it is gone.
 *
 * The window keeps trying on its own, so if the application is started again
 * the page returns to itself without a reload.
 */
export const StudioOffline: React.FC = () => {
  const { t } = useI18n();
  const [seconds, setSeconds] = useState(0);
  const [checking, setChecking] = useState(false);

  useEffect(() => {
    const started = Date.now();
    const timer = window.setInterval(() => setSeconds(Math.round((Date.now() - started) / 1000)), 1000);
    return () => window.clearInterval(timer);
  }, []);

  return (
    <div className="flex h-full w-full items-center justify-center bg-white px-5 py-10 dark:bg-suno">
      <div className="w-full max-w-md text-center">
        <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-full bg-rose-500/10 text-rose-500">
          <PlugZap size={22} />
        </div>
        <h2 className="mt-4 text-2xl font-extrabold tracking-tight text-zinc-900 dark:text-white">{t('studioOfflineTitle')}</h2>
        <p className="mt-3 text-sm leading-6 text-zinc-500 dark:text-zinc-400">{t('studioOfflineBody')}</p>
        <p className="mt-2 text-xs tabular-nums text-zinc-400">{t('studioOfflineRetrying')} · {seconds} s</p>
        <button
          type="button"
          onClick={() => {
            setChecking(true);
            void fetch('/health')
              .then((response) => { if (response.ok) window.location.reload(); })
              .catch(() => undefined)
              .finally(() => setChecking(false));
          }}
          className="mt-5 inline-flex items-center gap-2 rounded-xl border border-zinc-300 px-4 py-2.5 text-sm font-semibold text-zinc-700 hover:border-pink-400 hover:text-pink-600 dark:border-white/15 dark:text-zinc-200"
        >
          <RefreshCw size={15} className={checking ? 'animate-spin' : undefined} />
          {t('studioOfflineRetry')}
        </button>
      </div>
    </div>
  );
};

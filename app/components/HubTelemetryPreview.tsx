import React, { useEffect, useState } from 'react';
import { X } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { telemetryPreview } from '../services/studioHub';

/** "What is sent": exactly the report the studio would send today, as the hub receives it. */
export const HubTelemetryPreview: React.FC<{ onClose: () => void }> = ({ onClose }) => {
  const { t } = useI18n();
  const [report, setReport] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    telemetryPreview()
      .then((preview) => setReport(preview.report ? JSON.stringify(preview.report, null, 2) : null))
      .catch((failure: Error) => setError(failure.message));
  }, []);
  return (
    <div className="fixed inset-0 z-[95] flex items-center justify-center bg-black/60 p-4" onClick={(event) => { if (event.target === event.currentTarget) onClose(); }}>
      <div role="dialog" aria-modal="true" className="flex max-h-[80vh] w-full max-w-xl flex-col overflow-hidden rounded-2xl border border-zinc-200 bg-white shadow-2xl dark:border-white/10 dark:bg-suno-panel">
        <div className="flex items-center justify-between border-b border-zinc-200 px-5 py-3 dark:border-white/10">
          <h2 className="font-semibold">{t('hubTelemetryWhat')}</h2>
          <button type="button" onClick={onClose} aria-label={t('hubClose')} className="rounded-md p-1 text-zinc-500 hover:bg-zinc-100 dark:hover:bg-white/10"><X size={18} /></button>
        </div>
        <div className="space-y-3 overflow-auto p-5 text-sm text-zinc-600 dark:text-zinc-400">
          <p>{t('hubTelemetryWhy')}</p>
          {error && <p className="text-red-500">{error}</p>}
          {report ? <pre className="overflow-auto rounded-xl bg-zinc-100 p-3 text-xs text-zinc-800 dark:bg-black/40 dark:text-zinc-200">{report}</pre> : !error && <p>{t('hubTelemetryNothing')}</p>}
        </div>
      </div>
    </div>
  );
};

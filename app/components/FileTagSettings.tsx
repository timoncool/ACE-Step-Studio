import React, { useEffect, useState } from 'react';
import { useI18n } from '../context/I18nContext';

/** Settings: whether a track's file carries its note, style and generation settings. */
export const FileTagSettings: React.FC = () => {
  const { t } = useI18n();
  const [settings, setSettings] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetch('/v1/library/file-tags')
      .then(async response => {
        const body = await response.json();
        if (!response.ok) throw new Error(body?.error || String(response.status));
        setSettings(Boolean(body.settings));
      })
      .catch((failure: Error) => setError(failure.message));
  }, []);

  const change = async (next: boolean) => {
    setBusy(true);
    setError(null);
    try {
      const response = await fetch('/v1/library/file-tags', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ settings: next }) });
      const body = await response.json().catch(() => null);
      if (!response.ok) throw new Error(body?.error || String(response.status));
      setSettings(Boolean(body.settings));
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="space-y-3 text-sm">
      <h3 className="font-semibold">{t('fileTagsTitle')}</h3>
      <p className="text-zinc-500 dark:text-zinc-400">{t('fileTagsWhy')}</p>
      <label className="flex cursor-pointer items-center gap-3">
        <input type="checkbox" className="h-4 w-4 accent-pink-500" checked={Boolean(settings)} disabled={settings === null || busy}
               onChange={(event) => { void change(event.target.checked); }} />
        <span>{t('fileTagsCheckbox')}</span>
      </label>
      {error && <p role="alert" className="text-xs text-red-600 dark:text-red-400">{error}</p>}
    </section>
  );
};

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { Check, Plus, Trash2 } from 'lucide-react';
import { useI18n } from '../context/I18nContext';

/**
 * The cover looks, and which one a new cover starts from.
 *
 * Writing the style again for every track is the thing this removes: the look
 * lives in a template with `{title}`, `{style}` and `{excerpt}` in it, the track
 * fills the rest in, and one template is the default so a cover can be made
 * without opening this page at all.
 *
 * The example under the editor is rendered by the server, with the same code
 * that runs when the cover is generated - so what is shown is what will be sent.
 */

interface CoverTemplate {
  id: string;
  name: string;
  template: string;
}

const CONTROL =
  'w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:border-pink-500 dark:border-white/10 dark:bg-black/20 dark:text-white';

const PLACEHOLDERS = ['title', 'style', 'lyrics', 'excerpt', 'duration'];

export const CoverTemplateSettings: React.FC = () => {
  const { t } = useI18n();
  const [templates, setTemplates] = useState<CoverTemplate[]>([]);
  const [defaultId, setDefaultId] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  // Draw a cover as soon as a track is finished, the way karaoke times itself.
  const [auto, setAuto] = useState(true);
  const [preview, setPreview] = useState('');

  useEffect(() => {
    void fetch('/v1/cover-templates')
      .then(response => response.json())
      .then((body: { templates?: CoverTemplate[]; default_id?: string | null; auto?: boolean }) => {
        setTemplates(body.templates ?? []);
        if (typeof body.auto === 'boolean') setAuto(body.auto);
        setDefaultId(body.default_id ?? body.templates?.[0]?.id ?? null);
        setEditingId(current => current ?? body.default_id ?? body.templates?.[0]?.id ?? null);
      })
      .catch(() => setTemplates([]));
  }, []);

  const save = useCallback(async (next: CoverTemplate[], nextDefault: string | null, nextAuto?: boolean) => {
    setTemplates(next);
    setDefaultId(nextDefault);
    if (typeof nextAuto === 'boolean') setAuto(nextAuto);
    await fetch('/v1/cover-templates', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ templates: next, default_id: nextDefault, auto: nextAuto ?? auto }),
    }).catch(() => undefined);
  }, [auto]);

  const editing = useMemo(() => templates.find(entry => entry.id === editingId) ?? null, [templates, editingId]);

  // What this template becomes for a track, without needing a track open.
  useEffect(() => {
    if (!editing?.template.trim()) {
      setPreview('');
      return;
    }
    const timer = window.setTimeout(() => {
      void fetch('/v1/cover-templates/render', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          template: editing.template,
          title: t('coverTemplateSampleTitle'),
          style: t('coverTemplateSampleStyle'),
          lyrics: t('coverTemplateSampleLyrics'),
        }),
      })
        .then(response => response.json())
        .then((body: { prompt?: string }) => setPreview(body.prompt ?? ''))
        .catch(() => setPreview(''));
    }, 250);
    return () => window.clearTimeout(timer);
  }, [editing, t]);

  const update = (patch: Partial<CoverTemplate>) => {
    if (!editing) return;
    void save(templates.map(entry => (entry.id === editing.id ? { ...entry, ...patch } : entry)), defaultId);
  };

  return (
    <div className="max-w-2xl space-y-5">
      <p className="text-sm text-zinc-500 dark:text-zinc-400">{t('coverTemplatesHint')}</p>

      <label className="flex cursor-pointer items-start gap-3 rounded-xl border border-zinc-200 p-3 dark:border-white/10">
        <input
          type="checkbox"
          checked={auto}
          onChange={event => void save(templates, defaultId, event.target.checked)}
          className="mt-0.5 h-4 w-4 accent-pink-500"
        />
        <span className="min-w-0">
          <span className="block text-sm font-medium text-zinc-900 dark:text-white">{t('coverAuto')}</span>
          <span className="mt-0.5 block text-xs leading-5 text-zinc-500 dark:text-zinc-400">{t('coverAutoHint')}</span>
        </span>
      </label>

      <div className="flex flex-wrap gap-2">
        {templates.map(entry => (
          <button
            key={entry.id}
            type="button"
            onClick={() => setEditingId(entry.id)}
            className={`inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-xs font-semibold ${
              entry.id === editingId
                ? 'bg-pink-500/15 text-pink-600 dark:text-pink-300'
                : 'bg-zinc-200/70 text-zinc-600 dark:bg-white/10 dark:text-zinc-300'
            }`}
          >
            {entry.id === defaultId && <Check size={12} />}
            {entry.name}
          </button>
        ))}
        <button
          type="button"
          onClick={() => {
            const id = `custom-${Date.now().toString(36)}`;
            void save([...templates, { id, name: t('coverPromptMyStyle'), template: t('coverPromptTemplatePlaceholder') }], defaultId);
            setEditingId(id);
          }}
          className="inline-flex items-center gap-1.5 rounded-full border border-dashed border-zinc-300 px-3 py-1.5 text-xs font-semibold text-zinc-500 hover:border-pink-400 hover:text-pink-600 dark:border-white/15"
        >
          <Plus size={12} />{t('coverTemplateAdd')}
        </button>
      </div>

      {editing && (
        <div className="space-y-3 rounded-xl border border-zinc-200 p-4 dark:border-white/10">
          <label className="block">
            <span className="mb-1 block text-[11px] font-bold uppercase tracking-wide text-zinc-500">{t('coverTemplateName')}</span>
            <input value={editing.name} onChange={event => update({ name: event.target.value })} className={CONTROL} />
          </label>

          <label className="block">
            <span className="mb-1 block text-[11px] font-bold uppercase tracking-wide text-zinc-500">{t('coverTemplateBody')}</span>
            <textarea
              value={editing.template}
              onChange={event => update({ template: event.target.value })}
              rows={8}
              className={`${CONTROL} min-h-[180px] resize-y font-mono text-[13px] leading-5`}
            />
          </label>

          <div className="flex flex-wrap items-center gap-1">
            <span className="mr-1 text-[10px] font-bold uppercase tracking-wide text-zinc-500">{t('coverPromptPlaceholders')}</span>
            {PLACEHOLDERS.map(name => (
              <button
                key={name}
                type="button"
                onClick={() => update({ template: `${editing.template}{${name}}` })}
                className="rounded-full bg-zinc-200/70 px-2 py-0.5 text-[10px] font-semibold text-zinc-600 hover:bg-pink-500/15 hover:text-pink-600 dark:bg-white/10 dark:text-zinc-300"
              >
                {`{${name}}`}
              </button>
            ))}
          </div>

          {preview && (
            <div className="rounded-lg bg-zinc-100 p-3 text-xs leading-5 text-zinc-600 dark:bg-black/30 dark:text-zinc-400">
              <div className="mb-1 text-[10px] font-bold uppercase tracking-wide text-zinc-500">{t('coverTemplatePreview')}</div>
              {preview}
            </div>
          )}

          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              onClick={() => void save(templates, editing.id)}
              disabled={editing.id === defaultId}
              className="inline-flex items-center gap-1.5 rounded-lg bg-zinc-900 px-3 py-1.5 text-xs font-bold text-white disabled:opacity-40 dark:bg-white dark:text-zinc-900"
            >
              <Check size={13} />{editing.id === defaultId ? t('coverTemplateIsDefault') : t('coverTemplateMakeDefault')}
            </button>
            <button
              type="button"
              onClick={() => {
                const next = templates.filter(entry => entry.id !== editing.id);
                void save(next, defaultId === editing.id ? next[0]?.id ?? null : defaultId);
                setEditingId(next[0]?.id ?? null);
              }}
              disabled={templates.length <= 1}
              className="inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-semibold text-zinc-500 hover:text-rose-600 disabled:opacity-40"
            >
              <Trash2 size={13} />{t('coverPromptDeleteTemplate')}
            </button>
          </div>
        </div>
      )}
    </div>
  );
};

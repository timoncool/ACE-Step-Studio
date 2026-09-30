import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { AlertTriangle, Check, Image as ImageIcon, Loader2, Palette, Plus, Sparkles, Trash2 } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { changeCoverLook, type CoverLook, readJson, useCoverLook, useOpenRouterSettings } from '../services/studioQueries';
import { COVER_PATTERNS, patternArt } from '../services/coverArt';
import type { CommonsPhoto } from './CoverPicker';

/**
 * How a track without a cover of its own looks - a photograph its style calls
 * up, its pattern, or a cover generated for every new track - and the prompt
 * templates a generated cover starts from.
 *
 * Writing the style again for every track is the thing the templates remove:
 * the look lives in a template with `{title}`, `{style}` and `{excerpt}` in it,
 * the track fills the rest in, and one template is the default. The example
 * under the editor is rendered by the server, with the same code that runs
 * when the cover is generated - so what is shown is what will be sent.
 */

interface CoverTemplate {
  id: string;
  name: string;
  template: string;
}

type Mode = 'photo' | 'pattern' | 'ai';

const CONTROL =
  'w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-hidden focus:border-pink-500 dark:border-white/10 dark:bg-black/20 dark:text-white';

const PLACEHOLDERS = ['title', 'style', 'lyrics', 'excerpt', 'duration'];

/** Every style is shown drawn from the same seed, so the tiles differ only in style. */
const EXAMPLE_SEED = 'studio';

/** Styles whose photographs show how a style becomes a picture. */
const PHOTO_EXAMPLES = ['synthwave', 'melodic death metal', 'lo-fi hip hop'];

const reasonOf = (reason: unknown) => (reason instanceof Error ? reason.message : String(reason));

const Problem: React.FC<{ text: string }> = ({ text }) => (
  <p role="alert" className="flex items-start gap-2 rounded-lg bg-rose-500/10 px-3 py-2 text-xs text-rose-700 dark:text-rose-300">
    <AlertTriangle size={13} className="mt-0.5 shrink-0" /> {text}
  </p>
);

interface ExampleRow {
  style: string;
  scene: string;
  photos: CommonsPhoto[];
}

/** What the photographs are: for a few styles, the scene each calls up and the
 *  first photographs Commons gives for it. */
const PhotoExamples: React.FC = () => {
  const { t } = useI18n();
  const [rows, setRows] = useState<ExampleRow[]>([]);
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    Promise.all(PHOTO_EXAMPLES.map(async style => {
      const { scenes } = await readJson<{ scenes: string[] }>(`/v1/media/scenes?style=${encodeURIComponent(style)}`);
      const { photos } = await readJson<{ photos: CommonsPhoto[] }>(`/v1/media/photos?q=${encodeURIComponent(scenes[0])}`);
      return { style, scene: scenes[0], photos: photos.slice(0, 7) };
    }))
      .then(found => { if (current) setRows(found); })
      .catch(reason => { if (current) setProblem(reasonOf(reason)); });
    return () => { current = false; };
  }, []);

  return (
    <div className="space-y-2">
      <span className="block text-[11px] font-bold uppercase tracking-wide text-zinc-500">{t('coverLookPhotoExamples')}</span>
      {problem && <Problem text={t('pickerSearchFailed').replace('{reason}', () => problem)} />}
      {!problem && rows.length === 0 && <Loader2 size={18} className="animate-spin text-pink-500" />}
      {rows.map(row => (
        <div key={row.style}>
          <p className="mb-1 text-[11px] text-zinc-500">
            <span className="font-semibold text-zinc-700 dark:text-zinc-300">{row.style}</span> → {row.scene}
          </p>
          <div className="grid grid-cols-4 gap-2 sm:grid-cols-7">
            {row.photos.map(photo => (
              <img key={photo.page} src={photo.preview} alt={photo.title} title={photo.title} loading="lazy" className="aspect-square w-full rounded-lg object-cover" />
            ))}
          </div>
        </div>
      ))}
    </div>
  );
};

/** The prompt templates a generated cover starts from, and which one is the default. */
const TemplateEditor: React.FC<{
  templates: CoverTemplate[];
  defaultId: string | null;
  onSave: (templates: CoverTemplate[], defaultId: string | null) => void;
}> = ({ templates, defaultId, onSave }) => {
  const { t } = useI18n();
  const [editingId, setEditingId] = useState<string | null>(defaultId ?? templates[0]?.id ?? null);
  const [preview, setPreview] = useState('');
  const [problem, setProblem] = useState<string | null>(null);
  const editing = useMemo(() => templates.find(entry => entry.id === editingId) ?? null, [templates, editingId]);

  // What this template becomes for a track, without needing a track open.
  const sampleTitle = t('coverTemplateSampleTitle');
  const sampleStyle = t('coverTemplateSampleStyle');
  const sampleLyrics = t('coverTemplateSampleLyrics');
  useEffect(() => {
    if (!editing?.template.trim()) {
      setPreview('');
      return;
    }
    const timer = window.setTimeout(() => {
      fetch('/v1/cover-templates/render', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ template: editing.template, title: sampleTitle, style: sampleStyle, lyrics: sampleLyrics }),
      })
        .then(async response => {
          const body = await response.json().catch(() => null);
          if (!response.ok) throw new Error(body?.error || `the template answered ${response.status}`);
          setPreview(typeof body?.prompt === 'string' ? body.prompt : '');
          setProblem(null);
        })
        .catch(reason => setProblem(reasonOf(reason)));
    }, 250);
    return () => window.clearTimeout(timer);
  }, [editing, sampleTitle, sampleStyle, sampleLyrics]);

  const update = (patch: Partial<CoverTemplate>) => {
    if (!editing) return;
    onSave(templates.map(entry => (entry.id === editing.id ? { ...entry, ...patch } : entry)), defaultId);
  };

  return (
    <div className="space-y-3">
      <p className="text-xs leading-5 text-zinc-500 dark:text-zinc-400">{t('coverTemplatesHint')}</p>
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
            onSave([...templates, { id, name: t('coverPromptMyStyle'), template: t('coverPromptTemplatePlaceholder') }], defaultId);
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
          {problem && <Problem text={problem} />}

          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              onClick={() => onSave(templates, editing.id)}
              disabled={editing.id === defaultId}
              className="inline-flex items-center gap-1.5 rounded-lg bg-zinc-900 px-3 py-1.5 text-xs font-bold text-white disabled:opacity-40 dark:bg-white dark:text-zinc-900"
            >
              <Check size={13} />{editing.id === defaultId ? t('coverTemplateIsDefault') : t('coverTemplateMakeDefault')}
            </button>
            <button
              type="button"
              onClick={() => {
                const next = templates.filter(entry => entry.id !== editing.id);
                onSave(next, defaultId === editing.id ? next[0]?.id ?? null : defaultId);
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

export const CoverTemplateSettings: React.FC = () => {
  const { t } = useI18n();
  const { data: look, error: lookError } = useCoverLook(true);
  const { data: openRouter } = useOpenRouterSettings<{ configured?: boolean }>();
  const [templates, setTemplates] = useState<CoverTemplate[]>([]);
  const [defaultId, setDefaultId] = useState<string | null>(null);
  // A cover generated as soon as a track is finished, the way karaoke times itself.
  const [auto, setAuto] = useState<boolean | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    readJson<{ templates?: CoverTemplate[]; default_id?: string | null; auto?: boolean }>('/v1/cover-templates')
      .then(body => {
        setTemplates(body.templates ?? []);
        setAuto(body.auto === true);
        setDefaultId(body.default_id ?? body.templates?.[0]?.id ?? null);
      })
      .catch(reason => setProblem(reasonOf(reason)));
  }, []);

  const save = useCallback(async (next: CoverTemplate[], nextDefault: string | null, nextAuto?: boolean) => {
    setTemplates(next);
    setDefaultId(nextDefault);
    if (typeof nextAuto === 'boolean') setAuto(nextAuto);
    try {
      const response = await fetch('/v1/cover-templates', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ templates: next, default_id: nextDefault, auto: nextAuto ?? auto }),
      });
      if (!response.ok) {
        const body = await response.json().catch(() => null);
        throw new Error(body?.error || `saving the cover settings answered ${response.status}`);
      }
    } catch (reason) {
      setProblem(reasonOf(reason));
    }
  }, [auto]);

  const changeLook = (next: Partial<Pick<CoverLook, 'photo' | 'pattern' | 'keep'>>) => {
    setProblem(null);
    changeCoverLook(next).catch(reason => setProblem(reasonOf(reason)));
  };

  // Generating wears a photograph until its cover is there, and when it fails.
  const choose = (mode: Mode) => {
    if (!look) return;
    const wantsAuto = mode === 'ai';
    if (auto !== wantsAuto) void save(templates, defaultId, wantsAuto);
    const wantsPhoto = mode !== 'pattern';
    if (look.photo !== wantsPhoto) changeLook({ photo: wantsPhoto });
  };

  const mode: Mode | null = !look || auto === null ? null : auto ? 'ai' : look.photo ? 'photo' : 'pattern';
  const failed = problem ?? (lookError ? lookError.message : null);
  const option = (active: boolean) =>
    `inline-flex items-center justify-center gap-1.5 rounded-lg px-3 py-2 text-xs font-bold ${active ? 'bg-pink-500/15 text-pink-600 dark:text-pink-300' : 'bg-zinc-200/70 text-zinc-600 hover:text-zinc-900 dark:bg-white/10 dark:text-zinc-300 dark:hover:text-white'}`;
  const hints: Record<Mode, string> = {
    photo: t('coverLookPhotoHint'),
    pattern: t('coverLookPatternHint'),
    ai: t('coverLookAiHint'),
  };

  return (
    <div className="max-w-2xl space-y-3 rounded-xl border border-zinc-200 p-3 dark:border-white/10">
      <div>
        <span className="block text-sm font-medium text-zinc-900 dark:text-white">{t('coverLookTitle')}</span>
        <span className="mt-0.5 block text-xs leading-5 text-zinc-500 dark:text-zinc-400">{t('coverLookHint')}</span>
      </div>

      {look && mode && (
        <>
          <div className="grid grid-cols-3 gap-2">
            <button type="button" aria-pressed={mode === 'photo'} onClick={() => choose('photo')} className={option(mode === 'photo')}>
              <ImageIcon size={13} />{t('coverLookPhoto')}
            </button>
            <button type="button" aria-pressed={mode === 'pattern'} onClick={() => choose('pattern')} className={option(mode === 'pattern')}>
              <Palette size={13} />{t('coverLookPattern')}
            </button>
            <button type="button" aria-pressed={mode === 'ai'} onClick={() => choose('ai')} className={option(mode === 'ai')}>
              <Sparkles size={13} />{t('coverLookAi')}
            </button>
          </div>
          <p className="text-xs leading-5 text-zinc-500 dark:text-zinc-400">{hints[mode]}</p>

          {mode === 'photo' && <PhotoExamples />}
          {mode === 'pattern' && (
            <div>
              <span className="mb-1.5 block text-[11px] font-bold uppercase tracking-wide text-zinc-500">{t('coverLookPatterns')}</span>
              <div className="grid grid-cols-4 gap-2 sm:grid-cols-7">
                {COVER_PATTERNS.map(pattern => (
                  <button
                    key={pattern}
                    type="button"
                    title={pattern}
                    aria-pressed={look.pattern === pattern}
                    onClick={() => changeLook({ pattern })}
                    className={`relative aspect-square overflow-hidden rounded-lg ${look.pattern === pattern ? 'ring-2 ring-pink-500' : 'hover:ring-2 hover:ring-pink-300'}`}
                  >
                    <img src={patternArt(EXAMPLE_SEED, pattern)} alt={pattern} className="h-full w-full object-cover" />
                    <span className="absolute inset-x-0 bottom-0 truncate bg-black/55 px-1 py-0.5 text-center text-[9px] font-semibold text-white">{pattern}</span>
                  </button>
                ))}
              </div>
            </div>
          )}
          {mode === 'ai' && (
            <>
              {openRouter?.configured === false && (
                <p className="flex items-start gap-2 text-xs leading-5 text-amber-600 dark:text-amber-300">
                  <AlertTriangle size={13} className="mt-1 shrink-0" />{t('coverKeyRequired')}
                </p>
              )}
              <TemplateEditor templates={templates} defaultId={defaultId} onSave={(next, nextDefault) => void save(next, nextDefault)} />
            </>
          )}

          <label className="flex cursor-pointer items-start gap-3">
            <input
              type="checkbox"
              checked={look.keep}
              onChange={event => changeLook({ keep: event.target.checked })}
              className="mt-0.5 h-4 w-4 accent-pink-500"
            />
            <span className="min-w-0">
              <span className="block text-sm font-medium text-zinc-900 dark:text-white">{t('coverKeep')}</span>
              <span className="mt-0.5 block text-xs leading-5 text-zinc-500 dark:text-zinc-400">{t('coverKeepHint')}</span>
            </span>
          </label>

          {look.photo && look.problem && (
            <p className="flex items-start gap-2 text-xs leading-5 text-amber-600 dark:text-amber-300">
              <AlertTriangle size={13} className="mt-1 shrink-0" />
              {t('coverLookProblem').replace('{reason}', () => look.problem ?? '')}
            </p>
          )}
        </>
      )}
      {failed && <Problem text={failed} />}
    </div>
  );
};

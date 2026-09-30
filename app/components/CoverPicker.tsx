import React, { useCallback, useEffect, useRef, useState } from 'react';
import { AlertTriangle, Image as ImageIcon, Loader2, Palette, RefreshCw, Search, Sparkles, Upload, Video } from 'lucide-react';
import type { Song } from '../types';
import { useI18n } from '../context/I18nContext';
import { COVER_PATTERNS, type CoverPattern, patternArt } from '../services/coverArt';
import { coverSeed } from '../services/songStems';
import { readJson } from '../services/studioQueries';

/**
 * Where a picture comes from, wherever one is chosen: a track's cover, and the
 * background and the centre of a video. The same choices everywhere - a
 * photograph from Wikimedia Commons, a clip from it for a background, the
 * track's pattern in any style, a picture drawn through OpenRouter, a file.
 */

export interface CommonsPhoto {
  title: string;
  page: string;
  image: string;
  preview: string;
  large: string;
}

export interface CommonsClip {
  title: string;
  page: string;
  poster: string;
  video: string;
  width: number;
  height: number;
}

export type PickedMedia =
  | { kind: 'image'; url: string; base64: string; mediaType: string }
  | { kind: 'photo'; url: string; photo: CommonsPhoto }
  | { kind: 'pattern'; url: string; pattern: CoverPattern; seed: string }
  | { kind: 'video'; url: string; clip: CommonsClip | null };

type Tab = 'photo' | 'video' | 'pattern' | 'ai';

interface CatalogModel {
  id: string;
  name: string;
  capabilities: string[];
}

/** A saved look, filled in from the track it is used on. */
interface CoverTemplate {
  id: string;
  name: string;
  template: string;
}

interface CoverPickerProps {
  song: Song;
  choice: PickedMedia | null;
  onChoice: (choice: PickedMedia) => void;
  /** A background also takes a clip, and wants its pictures as wide as a frame. */
  forBackground?: boolean;
  startOn?: 'photo' | 'video';
}

const CONTROL =
  'w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-hidden focus:border-pink-500 dark:border-white/10 dark:bg-black/20 dark:text-white';

const TAB = 'inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-bold transition-colors';

const IMAGE_TYPES = ['image/png', 'image/jpeg', 'image/webp'];
const VIDEO_TYPES = ['video/mp4', 'video/webm'];

/** The words a template may stand in for, shown so they can be typed. */
const PLACEHOLDERS = ['title', 'style', 'lyrics', 'excerpt', 'duration'];

const defaultPrompt = (song: Song, forBackground: boolean) =>
  forBackground
    ? `Wide background artwork for a music video of a track titled "${song.title}". Style: ${song.style || 'contemporary'}. No text, no lettering, 16:9 composition.`
    : `Album cover artwork for a track titled "${song.title}". Style: ${song.style || 'contemporary'}. No text, no lettering, square composition.`;

const reasonOf = (reason: unknown) => (reason instanceof Error ? reason.message : String(reason));

const readFile = (file: File) =>
  new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result ?? ''));
    reader.onerror = () => reject(reader.error ?? new Error(`${file.name} could not be read`));
    reader.readAsDataURL(file);
  });

export const CoverPicker: React.FC<CoverPickerProps> = ({ song, choice, onChoice, forBackground = false, startOn = 'photo' }) => {
  const { t } = useI18n();
  const [tab, setTab] = useState<Tab>(forBackground ? startOn : 'photo');
  const [fileError, setFileError] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement | null>(null);

  const pickFile = useCallback(async (file: File) => {
    setFileError(null);
    if (forBackground && VIDEO_TYPES.includes(file.type)) {
      onChoice({ kind: 'video', url: URL.createObjectURL(file), clip: null });
      return;
    }
    if (!IMAGE_TYPES.includes(file.type)) {
      setFileError(forBackground ? t('pickerFileKindsBackground') : t('pickerFileKinds'));
      return;
    }
    try {
      const url = await readFile(file);
      onChoice({ kind: 'image', url, mediaType: file.type, base64: url.slice(url.indexOf(',') + 1) });
    } catch (reason) {
      setFileError(reasonOf(reason));
    }
  }, [forBackground, onChoice, t]);

  const tabs: { id: Tab; label: string; icon: React.ReactNode }[] = [
    { id: 'photo', label: t('pickerTabPhoto'), icon: <ImageIcon size={13} /> },
    ...(forBackground ? [{ id: 'video' as Tab, label: t('pickerTabVideo'), icon: <Video size={13} /> }] : []),
    { id: 'pattern', label: t('pickerTabPattern'), icon: <Palette size={13} /> },
    { id: 'ai', label: t('pickerTabAi'), icon: <Sparkles size={13} /> },
  ];

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-1.5">
        {tabs.map(entry => (
          <button
            key={entry.id}
            type="button"
            onClick={() => setTab(entry.id)}
            aria-pressed={tab === entry.id}
            className={`${TAB} ${tab === entry.id ? 'bg-pink-500/15 text-pink-600 dark:text-pink-300' : 'bg-zinc-200/70 text-zinc-600 hover:text-zinc-900 dark:bg-white/10 dark:text-zinc-300 dark:hover:text-white'}`}
          >
            {entry.icon}{entry.label}
          </button>
        ))}
        <button
          type="button"
          onClick={() => fileInput.current?.click()}
          className={`${TAB} ml-auto border border-zinc-300 text-zinc-700 hover:border-pink-400 hover:text-pink-600 dark:border-white/15 dark:text-zinc-200`}
        >
          <Upload size={13} />{forBackground ? t('pickerUseFile') : t('useImageFile')}
        </button>
        <input
          ref={fileInput}
          type="file"
          accept={(forBackground ? [...IMAGE_TYPES, ...VIDEO_TYPES] : IMAGE_TYPES).join(',')}
          className="hidden"
          onChange={event => {
            const file = event.target.files?.[0];
            if (file) void pickFile(file);
            event.target.value = '';
          }}
        />
      </div>

      {tab === 'photo' && <CommonsSearch kind="photo" song={song} choice={choice} onChoice={onChoice} forBackground={forBackground} />}
      {tab === 'video' && <CommonsSearch kind="video" song={song} choice={choice} onChoice={onChoice} forBackground />}
      {tab === 'pattern' && <PatternChoice song={song} choice={choice} onChoice={onChoice} forBackground={forBackground} />}
      {tab === 'ai' && <AiDrawing song={song} onChoice={onChoice} forBackground={forBackground} />}

      {fileError && <Problem text={fileError} />}
    </div>
  );
};

const Problem: React.FC<{ text: string }> = ({ text }) => (
  <p role="alert" className="flex items-start gap-2 rounded-lg bg-rose-500/10 px-3 py-2 text-xs text-rose-700 dark:text-rose-300">
    <AlertTriangle size={13} className="mt-0.5 shrink-0" /> {text}
  </p>
);

interface Found {
  query: string;
  from: number;
  total: number;
  photos: CommonsPhoto[];
  videos: CommonsClip[];
}

/** Photographs or clips from Commons: the track's style says what to look for
 *  first, and anything else can be typed. */
const CommonsSearch: React.FC<{
  kind: 'photo' | 'video';
  song: Song;
  choice: PickedMedia | null;
  onChoice: (choice: PickedMedia) => void;
  forBackground: boolean;
}> = ({ kind, song, choice, onChoice, forBackground }) => {
  const { t } = useI18n();
  const [scenes, setScenes] = useState<string[]>([]);
  const [query, setQuery] = useState('');
  const [found, setFound] = useState<Found | null>(null);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  const search = useCallback(async (wanted: string, from = 0) => {
    const q = wanted.trim();
    if (!q) return;
    setBusy(true);
    setProblem(null);
    try {
      const response = await fetch(`/v1/media/${kind === 'photo' ? 'photos' : 'videos'}?q=${encodeURIComponent(q)}&from=${from}`);
      const body = await response.json().catch(() => null);
      if (!response.ok) throw new Error(body?.error || `the search answered ${response.status}`);
      setFound({ query: q, from, total: body.total ?? 0, photos: body.photos ?? [], videos: body.videos ?? [] });
    } catch (reason) {
      setProblem(reasonOf(reason));
    } finally {
      setBusy(false);
    }
  }, [kind]);

  const style = song.style;
  useEffect(() => {
    let current = true;
    readJson<{ scenes: string[] }>(`/v1/media/scenes?style=${encodeURIComponent(style ?? '')}`)
      .then(({ scenes: suggested }) => {
        if (!current) return;
        setScenes(suggested);
        if (suggested[0]) {
          setQuery(suggested[0]);
          void search(suggested[0]);
        }
      })
      .catch(reason => { if (current) setProblem(reasonOf(reason)); });
    return () => { current = false; };
  }, [search, style]);

  const items = kind === 'photo' ? found?.photos ?? [] : found?.videos ?? [];
  const chosen = choice?.kind === 'photo' ? choice.photo.page : choice?.kind === 'video' ? choice.clip?.page : undefined;
  const tile = forBackground ? 'aspect-video' : 'aspect-square';

  return (
    <div className="space-y-2">
      <div className="flex gap-2">
        <input
          value={query}
          onChange={event => setQuery(event.target.value)}
          onKeyDown={event => { if (event.key === 'Enter') void search(query); }}
          placeholder={t('pickerSearchPlaceholder')}
          className={CONTROL}
        />
        <button
          type="button"
          onClick={() => void search(query)}
          disabled={busy || !query.trim()}
          className="inline-flex shrink-0 items-center gap-1.5 rounded-lg bg-zinc-900 px-3 py-2 text-xs font-bold text-white disabled:opacity-50 dark:bg-white dark:text-zinc-900"
        >
          {busy ? <Loader2 size={13} className="animate-spin" /> : <Search size={13} />}{t('search')}
        </button>
      </div>
      {scenes.length > 0 && (
        <div className="flex flex-wrap items-center gap-1">
          <span className="mr-1 text-[10px] font-bold uppercase tracking-wide text-zinc-500">{t('pickerSuggested')}</span>
          {scenes.map(scene => (
            <button
              key={scene}
              type="button"
              onClick={() => { setQuery(scene); void search(scene); }}
              className={`rounded-full px-2 py-0.5 text-[11px] font-semibold ${found?.query === scene ? 'bg-pink-500/15 text-pink-600 dark:text-pink-300' : 'bg-zinc-200/70 text-zinc-600 hover:text-pink-600 dark:bg-white/10 dark:text-zinc-300'}`}
            >
              {scene}
            </button>
          ))}
        </div>
      )}

      {problem && <Problem text={t('pickerSearchFailed').replace('{reason}', () => problem)} />}

      {busy && !found ? (
        <div className="flex h-32 items-center justify-center"><Loader2 size={24} className="animate-spin text-pink-500" /></div>
      ) : found && items.length === 0 ? (
        <p className="py-6 text-center text-xs text-zinc-500">{kind === 'photo' ? t('noPhotosFound') : t('noVideosFound')}</p>
      ) : (
        <div className={`grid gap-2 ${forBackground ? 'grid-cols-2 sm:grid-cols-3' : 'grid-cols-3 sm:grid-cols-4'}`}>
          {kind === 'photo'
            ? (found?.photos ?? []).map(photo => (
              <button
                key={photo.page}
                type="button"
                title={photo.title}
                onClick={() => onChoice({ kind: 'photo', url: forBackground ? photo.large : photo.image, photo })}
                className={`relative overflow-hidden rounded-lg bg-zinc-200 dark:bg-zinc-800 ${tile} ${chosen === photo.page ? 'ring-2 ring-pink-500' : 'hover:ring-2 hover:ring-pink-300'}`}
              >
                <img src={photo.preview} alt={photo.title} loading="lazy" className="h-full w-full object-cover" />
              </button>
            ))
            : (found?.videos ?? []).map(clip => (
              <button
                key={clip.page}
                type="button"
                title={clip.title}
                onClick={() => onChoice({ kind: 'video', url: clip.video, clip })}
                className={`relative aspect-video overflow-hidden rounded-lg bg-zinc-200 dark:bg-zinc-800 ${chosen === clip.page ? 'ring-2 ring-pink-500' : 'hover:ring-2 hover:ring-pink-300'}`}
              >
                <img src={clip.poster} alt={clip.title} loading="lazy" className="h-full w-full object-cover" />
                <span className="absolute right-1.5 top-1.5 inline-flex items-center gap-0.5 rounded bg-black/60 px-1.5 py-0.5 text-[9px] font-bold text-white">
                  <Video size={9} />{t('videoBadge')}
                </span>
              </button>
            ))}
        </div>
      )}

      <div className="flex items-center justify-between gap-2">
        <p className="text-[11px] text-zinc-500">{kind === 'photo' ? t('pickerCommonsPhotos') : t('pickerCommonsVideos')}</p>
        {found && found.total > found.from + items.length && (
          <button
            type="button"
            onClick={() => void search(found.query, found.from + items.length)}
            disabled={busy}
            className="inline-flex shrink-0 items-center gap-1 rounded-lg px-2 py-1 text-xs font-semibold text-pink-600 hover:bg-pink-500/10 disabled:opacity-50"
          >
            {busy ? <Loader2 size={12} className="animate-spin" /> : <RefreshCw size={12} />}{t('pickerMore')}
          </button>
        )}
      </div>
    </div>
  );
};

/** The track's pattern in every style on offer; another variant draws them all anew. */
const PatternChoice: React.FC<{
  song: Song;
  choice: PickedMedia | null;
  onChoice: (choice: PickedMedia) => void;
  forBackground: boolean;
}> = ({ song, choice, onChoice, forBackground }) => {
  const { t } = useI18n();
  const base = coverSeed(song);
  const [variant, setVariant] = useState(0);
  const seed = variant === 0 ? base : `${base}~${variant}`;
  const pick = (pattern: CoverPattern, from: string) =>
    onChoice({ kind: 'pattern', url: patternArt(from, pattern, forBackground ? 1920 : 512), pattern, seed: from });

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-2">
        <p className="text-[11px] leading-4 text-zinc-500">{t('pickerPatternHint')}</p>
        <button
          type="button"
          onClick={() => {
            const next = `${base}~${variant + 1}`;
            setVariant(variant + 1);
            // the chosen style stays chosen, drawn anew
            if (choice?.kind === 'pattern') pick(choice.pattern, next);
          }}
          className="inline-flex shrink-0 items-center gap-1.5 rounded-lg border border-zinc-300 px-3 py-1.5 text-xs font-semibold text-zinc-700 hover:border-pink-400 hover:text-pink-600 dark:border-white/15 dark:text-zinc-200"
        >
          <RefreshCw size={13} />{t('pickerPatternReroll')}
        </button>
      </div>
      <div className="grid grid-cols-4 gap-2 sm:grid-cols-7">
        {COVER_PATTERNS.map(pattern => (
          <button
            key={pattern}
            type="button"
            title={pattern}
            onClick={() => pick(pattern, seed)}
            className={`relative aspect-square overflow-hidden rounded-lg ${choice?.kind === 'pattern' && choice.pattern === pattern && choice.seed === seed ? 'ring-2 ring-pink-500' : 'hover:ring-2 hover:ring-pink-300'}`}
          >
            <img src={patternArt(seed, pattern)} alt={pattern} className="h-full w-full object-cover" />
            <span className="absolute inset-x-0 bottom-0 truncate bg-black/55 px-1 py-0.5 text-center text-[9px] font-semibold text-white">{pattern}</span>
          </button>
        ))}
      </div>
    </div>
  );
};

/** A picture drawn through OpenRouter from a free prompt or a saved look. */
const AiDrawing: React.FC<{ song: Song; onChoice: (choice: PickedMedia) => void; forBackground: boolean }> = ({ song, onChoice, forBackground }) => {
  const { t } = useI18n();
  const [models, setModels] = useState<CatalogModel[]>([]);
  const [modelId, setModelId] = useState('');
  const [keyConfigured, setKeyConfigured] = useState<boolean | null>(null);
  const [prompt, setPrompt] = useState(() => defaultPrompt(song, forBackground));
  const [templates, setTemplates] = useState<CoverTemplate[]>([]);
  const [templateId, setTemplateId] = useState('');
  // The template as text, and what it becomes for this track. Editing the
  // first updates the second, so what will be sent is never a guess.
  const [templateText, setTemplateText] = useState('');
  const [editingTemplate, setEditingTemplate] = useState(false);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    // The model is the one chosen for covers on the provider page; picking the
    // first of four hundred was how this window ended up using a different
    // model from the one the settings showed.
    Promise.all([
      readJson<{ configured?: boolean }>('/v1/openrouter/settings'),
      readJson<{ models: CatalogModel[] | null; suggested: { cover_art?: string | null } | null }>('/v1/openrouter/catalog'),
      readJson<{ selections?: { capability: string; cloud_model: string | null }[] }>('/v1/configuration'),
      readJson<{ templates?: CoverTemplate[]; default_id?: string | null }>('/v1/cover-templates'),
    ])
      .then(([settings, catalog, configuration, saved]) => {
        if (!current) return;
        setKeyConfigured(settings.configured === true);
        const covers = (catalog.models ?? []).filter(model => model.capabilities.includes('cover_art'));
        setModels(covers);
        const chosen = (configuration.selections ?? []).find(selection => selection.capability === 'cover_art')?.cloud_model;
        setModelId(chosen || catalog.suggested?.cover_art || covers[0]?.id || '');
        const list = saved.templates ?? [];
        setTemplates(list);
        // Whatever was chosen in Settings is where a new cover starts; a
        // background is not a cover, so it starts from its own prompt.
        const start = list.find(entry => entry.id === saved.default_id);
        if (start && !forBackground) {
          setTemplateId(start.id);
          setTemplateText(start.template);
        }
      })
      .catch(reason => { if (current) setProblem(reasonOf(reason)); });
    return () => { current = false; };
  }, [forBackground]);

  // A template is only useful once it is filled in, and the server does that
  // with the same code that runs when the cover is generated.
  useEffect(() => {
    if (!templateText.trim()) return;
    const timer = window.setTimeout(() => {
      fetch('/v1/cover-templates/render', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ template: templateText, song_id: song.id }),
      })
        .then(async response => {
          const body = await response.json().catch(() => null);
          if (!response.ok) throw new Error(body?.error || `the template answered ${response.status}`);
          if (typeof body?.prompt === 'string') setPrompt(body.prompt);
        })
        .catch(reason => setProblem(reasonOf(reason)));
    }, 250);
    return () => window.clearTimeout(timer);
  }, [templateText, song.id]);

  const saveTemplates = useCallback(async (next: CoverTemplate[]) => {
    setTemplates(next);
    try {
      const response = await fetch('/v1/cover-templates', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ templates: next }),
      });
      if (!response.ok) {
        const body = await response.json().catch(() => null);
        throw new Error(body?.error || `saving the templates answered ${response.status}`);
      }
    } catch (reason) {
      setProblem(reasonOf(reason));
    }
  }, []);

  const canGenerate = keyConfigured === true && Boolean(modelId);

  const generate = useCallback(async () => {
    if (!canGenerate || busy) return;
    setBusy(true);
    setProblem(null);
    try {
      const response = await fetch('/v1/openrouter/covers', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ model_id: modelId, prompt: prompt.trim() }),
      });
      const body = await response.json().catch(() => null);
      if (!response.ok) throw new Error(body?.error || `Cover generation failed (${response.status})`);
      const image = body?.body?.data?.[0];
      if (!image?.b64_json) throw new Error('OpenRouter returned no image data.');
      const mediaType = typeof image.media_type === 'string' ? image.media_type : 'image/png';
      onChoice({ kind: 'image', base64: image.b64_json, mediaType, url: `data:${mediaType};base64,${image.b64_json}` });
    } catch (reason) {
      setProblem(reasonOf(reason));
    } finally {
      setBusy(false);
    }
  }, [busy, canGenerate, modelId, onChoice, prompt]);

  return (
    <div className="rounded-xl border border-zinc-200 p-3 dark:border-white/10">
      <label className="text-[11px] font-bold uppercase tracking-wide text-zinc-500">{t('generateWithOpenRouter')}</label>
      {keyConfigured === false && (
        <p className="mt-2 flex items-start gap-2 text-xs text-amber-600 dark:text-amber-300">
          <AlertTriangle size={13} className="mt-0.5 shrink-0" />
          {t('coverKeyRequired')}
        </p>
      )}
      {keyConfigured === true && models.length === 0 && (
        <p className="mt-2 flex items-start gap-2 text-xs text-amber-600 dark:text-amber-300">
          <AlertTriangle size={13} className="mt-0.5 shrink-0" />
          {t('catalogNoImageModel')}
        </p>
      )}
      <select value={modelId} onChange={event => setModelId(event.target.value)} disabled={models.length === 0} className={`${CONTROL} mt-2`}>
        {models.length === 0 && <option value="">{t('noImageModel')}</option>}
        {models.map(model => <option key={model.id} value={model.id}>{model.name}</option>)}
      </select>
      {/* A look, written once and reused: the style lives in the
          template, the track fills in the rest. */}
      <div className="mt-2 flex items-center gap-2">
        <select
          value={templateId}
          onChange={event => {
            const next = templates.find(entry => entry.id === event.target.value);
            setTemplateId(event.target.value);
            setTemplateText(next?.template ?? '');
            if (!next) setPrompt(defaultPrompt(song, forBackground));
          }}
          className={CONTROL}
        >
          <option value="">{t('coverPromptFree')}</option>
          {templates.map(entry => <option key={entry.id} value={entry.id}>{entry.name}</option>)}
        </select>
        <button
          type="button"
          onClick={() => setEditingTemplate(editing => !editing)}
          className="shrink-0 rounded-lg border border-zinc-300 px-3 py-2 text-xs font-semibold text-zinc-600 hover:border-pink-400 hover:text-pink-600 dark:border-white/15 dark:text-zinc-300"
        >
          {editingTemplate ? t('coverPromptDoneEditing') : t('coverPromptEditTemplate')}
        </button>
      </div>

      {editingTemplate && (
        <div className="mt-2 rounded-lg border border-dashed border-zinc-300 p-2 dark:border-white/15">
          <textarea
            value={templateText}
            onChange={event => setTemplateText(event.target.value)}
            rows={6}
            placeholder={t('coverPromptTemplatePlaceholder')}
            className={CONTROL + ' resize-none'}
          />
          <div className="mt-2 flex flex-wrap items-center gap-1">
            <span className="mr-1 text-[10px] font-bold uppercase tracking-wide text-zinc-500">{t('coverPromptPlaceholders')}</span>
            {PLACEHOLDERS.map(name => (
              <button
                key={name}
                type="button"
                onClick={() => setTemplateText(text => text + '{' + name + '}')}
                className="rounded-full bg-zinc-200/70 px-2 py-0.5 text-[10px] font-semibold text-zinc-600 hover:bg-pink-500/15 hover:text-pink-600 dark:bg-white/10 dark:text-zinc-300"
              >
                {'{' + name + '}'}
              </button>
            ))}
          </div>
          <div className="mt-2 flex gap-2">
            <button
              type="button"
              onClick={() => {
                const id = templateId || 'custom-' + Date.now().toString(36);
                const name = templates.find(entry => entry.id === templateId)?.name || t('coverPromptMyStyle');
                const next = templates.some(entry => entry.id === id)
                  ? templates.map(entry => (entry.id === id ? { ...entry, template: templateText } : entry))
                  : [...templates, { id, name, template: templateText }];
                setTemplateId(id);
                void saveTemplates(next);
              }}
              disabled={!templateText.trim()}
              className="rounded-lg bg-zinc-900 px-3 py-1.5 text-xs font-bold text-white disabled:opacity-50 dark:bg-white dark:text-zinc-900"
            >
              {t('coverPromptSaveTemplate')}
            </button>
            {templateId && (
              <button
                type="button"
                onClick={() => {
                  void saveTemplates(templates.filter(entry => entry.id !== templateId));
                  setTemplateId('');
                  setTemplateText('');
                }}
                className="rounded-lg px-3 py-1.5 text-xs font-semibold text-zinc-500 hover:text-rose-600"
              >
                {t('coverPromptDeleteTemplate')}
              </button>
            )}
          </div>
        </div>
      )}

      {/* The prompt is the work here, so it gets the room: ten lines,
          and the user can drag it taller still. */}
      <textarea
        value={prompt}
        onChange={event => setPrompt(event.target.value)}
        rows={10}
        className={`${CONTROL} mt-2 min-h-[220px] resize-y font-mono text-[13px] leading-5`}
      />
      <button
        type="button"
        onClick={() => void generate()}
        disabled={!canGenerate || busy || !prompt.trim()}
        className="mt-2 inline-flex items-center gap-2 rounded-lg bg-linear-to-r from-orange-500 to-pink-600 px-4 py-2 text-xs font-bold text-white disabled:cursor-not-allowed disabled:opacity-50"
      >
        {busy ? <Loader2 size={14} className="animate-spin" /> : <Sparkles size={14} />} {t('generate')}
      </button>
      {problem && <div className="mt-2"><Problem text={problem} /></div>}
    </div>
  );
};

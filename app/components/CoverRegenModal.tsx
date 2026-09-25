import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { AlertTriangle, Image as ImageIcon, Loader2, Maximize2, Sparkles, Upload, X } from 'lucide-react';
import { Song } from '../types';
import { AlbumCover } from './AlbumCover';
import { ImageLightbox } from './ImageLightbox';
import { apiUrl } from '../services/apiBase';
import { useI18n } from '../context/I18nContext';

/**
 * Cover art for a library track.
 *
 * Two sources, both real: an image file from disk, or a generation through the
 * catalog-verified OpenRouter image models the native server exposes. The
 * result is stored by the studio server next to the track audio, so the cover
 * belongs to the song rather than living in browser state.
 *
 * ACE Studio generated covers through Pollinations. That path is gone with the
 * ACE backend; this replacement is explicit about cost — nothing is generated
 * until the user presses the button, and the button is disabled until a key and
 * a catalog model exist.
 */

interface CoverRegenModalProps {
  song: Song;
  onClose: () => void;
  onCoverSaved: (songId: string, coverUrl: string) => void;
}

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

const CONTROL =
  'w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:border-pink-500 dark:border-white/10 dark:bg-black/20 dark:text-white';

const defaultPrompt = (song: Song) =>
  `Album cover artwork for a track titled "${song.title}". Style: ${song.style || 'contemporary'}. No text, no lettering, square composition.`;

/** The words a template may stand in for, shown so they can be typed. */
const PLACEHOLDERS = ['title', 'style', 'lyrics', 'excerpt', 'duration'];

export const CoverRegenModal: React.FC<CoverRegenModalProps> = ({ song, onClose, onCoverSaved }) => {
  const { t } = useI18n();
  const [models, setModels] = useState<CatalogModel[]>([]);
  const [modelId, setModelId] = useState('');
  const [keyConfigured, setKeyConfigured] = useState<boolean | null>(null);
  const [prompt, setPrompt] = useState(() => defaultPrompt(song));
  const [templates, setTemplates] = useState<CoverTemplate[]>([]);
  const [templateId, setTemplateId] = useState('');
  // The template as text, and what it becomes for this track. Editing the
  // first updates the second, so what will be sent is never a guess.
  const [templateText, setTemplateText] = useState('');
  const [editingTemplate, setEditingTemplate] = useState(false);
  const [preview, setPreview] = useState<{ dataUrl: string; base64: string; mediaType: string } | null>(null);
  const [busy, setBusy] = useState<'generate' | 'save' | null>(null);
  // The cover, as big as the screen allows.
  const [zoomed, setZoomed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    void fetch('/v1/openrouter/settings')
      .then(response => response.json())
      .then((settings: { configured?: boolean }) => setKeyConfigured(settings.configured === true))
      .catch(() => setKeyConfigured(false));

    // The model is the one chosen for covers on the provider page; picking the
    // first of four hundred was how this window ended up using a different
    // model from the one the settings showed.
    void Promise.all([
      fetch('/v1/openrouter/catalog').then(response => response.json()).catch(() => null),
      fetch('/v1/configuration').then(response => response.json()).catch(() => null),
    ]).then(([catalog, configuration]) => {
      const covers = ((catalog?.models ?? []) as CatalogModel[]).filter(model => model.capabilities.includes('cover_art'));
      setModels(covers);
      const chosen = (configuration?.selections ?? []).find(
        (selection: { capability: string; cloud_model: string | null }) => selection.capability === 'cover_art',
      )?.cloud_model;
      const suggested = catalog?.suggested?.cover_art as string | undefined;
      setModelId(current => current || chosen || suggested || covers[0]?.id || '');
    });
  }, []);

  useEffect(() => {
    void fetch('/v1/cover-templates')
      .then(response => response.json())
      .then((body: { templates?: CoverTemplate[]; default_id?: string | null }) => {
        const list = body.templates ?? [];
        setTemplates(list);
        // Whatever was chosen in Settings is where a new cover starts.
        const chosen = list.find(entry => entry.id === body.default_id);
        if (chosen) {
          setTemplateId(chosen.id);
          setTemplateText(chosen.template);
        }
      })
      .catch(() => setTemplates([]));
  }, []);

  // A template is only useful once it is filled in, and the server does that
  // with the same code that runs when the cover is generated.
  useEffect(() => {
    if (!templateText.trim()) return;
    const timer = window.setTimeout(() => {
      void fetch('/v1/cover-templates/render', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ template: templateText, song_id: song.id }),
      })
        .then(response => response.json())
        .then((body: { prompt?: string }) => { if (body.prompt) setPrompt(body.prompt); })
        .catch(() => undefined);
    }, 250);
    return () => window.clearTimeout(timer);
  }, [templateText, song.id]);

  const saveTemplates = useCallback(async (next: CoverTemplate[]) => {
    setTemplates(next);
    await fetch('/v1/cover-templates', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ templates: next }),
    }).catch(() => undefined);
  }, []);

  const canGenerate = useMemo(() => keyConfigured === true && Boolean(modelId), [keyConfigured, modelId]);

  const generate = useCallback(async () => {
    if (!canGenerate || busy) return;
    setBusy('generate');
    setError(null);
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
      setPreview({ base64: image.b64_json, mediaType, dataUrl: `data:${mediaType};base64,${image.b64_json}` });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Cover generation failed.');
    } finally {
      setBusy(null);
    }
  }, [busy, canGenerate, modelId, prompt]);

  const pickFile = useCallback(async (file: File) => {
    setError(null);
    if (!['image/png', 'image/jpeg', 'image/webp'].includes(file.type)) {
      setError('Use a PNG, JPEG or WebP image.');
      return;
    }
    const buffer = new Uint8Array(await file.arrayBuffer());
    let binary = '';
    for (let offset = 0; offset < buffer.length; offset += 0x8000) {
      binary += String.fromCharCode(...buffer.subarray(offset, offset + 0x8000));
    }
    const base64 = btoa(binary);
    setPreview({ base64, mediaType: file.type, dataUrl: `data:${file.type};base64,${base64}` });
  }, []);

  const save = useCallback(async () => {
    if (!preview || busy) return;
    setBusy('save');
    setError(null);
    try {
      const response = await fetch(`/v1/library/songs/${encodeURIComponent(song.id)}/cover`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ image_base64: preview.base64, media_type: preview.mediaType }),
      });
      const body = await response.json().catch(() => null);
      if (!response.ok) throw new Error(body?.error || `Storing the cover failed (${response.status})`);
      onCoverSaved(song.id, apiUrl(`/v1/library/songs/${encodeURIComponent(song.id)}/cover`));
      onClose();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Storing the cover failed.');
    } finally {
      setBusy(null);
    }
  }, [busy, onClose, onCoverSaved, preview, song.id]);

  return (
    <div className="fixed inset-0 z-[70] flex items-center justify-center bg-black/60 p-4" onClick={onClose}>
      <div className="flex max-h-[92vh] w-full max-w-2xl flex-col overflow-hidden rounded-2xl bg-white shadow-2xl dark:bg-zinc-900" onClick={event => event.stopPropagation()}>
        <div className="flex items-center justify-between border-b border-zinc-200 px-5 py-4 dark:border-white/10">
          <h3 className="flex items-center gap-2 text-base font-bold text-zinc-900 dark:text-white">
            <ImageIcon size={18} className="text-pink-500" /> {t('coverArt')}
          </h3>
          <button type="button" onClick={onClose} className="text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200">
            <X size={18} />
          </button>
        </div>

        <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-5">
          {/* The result is the point of this window, so it gets the middle of
              it: a large square, and a click to see it at full size. */}
          <div className="flex flex-col items-center">
            <button
              type="button"
              onClick={() => { if (preview?.dataUrl || song.coverUrl) setZoomed(true); }}
              className="group relative aspect-square w-full max-w-[360px] overflow-hidden rounded-2xl border border-zinc-200 dark:border-white/10"
              title={t('coverOpenLarge')}
            >
              <AlbumCover seed={song.id} size="full" coverUrl={preview?.dataUrl || song.coverUrl} />
              <span className="pointer-events-none absolute bottom-2 right-2 inline-flex items-center gap-1 rounded-lg bg-black/60 px-2 py-1 text-[11px] text-white opacity-0 transition-opacity group-hover:opacity-100">
                <Maximize2 size={12} /> {t('coverOpenLarge')}
              </span>
            </button>

            <p className="mt-3 w-full truncate text-center text-sm font-semibold text-zinc-900 dark:text-white">{song.title}</p>
            <button
              type="button"
              onClick={() => fileInput.current?.click()}
              className="mt-2 inline-flex items-center gap-2 rounded-lg border border-zinc-300 px-3 py-2 text-xs font-semibold text-zinc-700 hover:border-pink-400 hover:text-pink-600 dark:border-white/15 dark:text-zinc-200"
            >
              <Upload size={13} /> {t('useImageFile')}
            </button>
            <input
              ref={fileInput}
              type="file"
              accept="image/png,image/jpeg,image/webp"
              className="hidden"
              onChange={event => {
                const file = event.target.files?.[0];
                if (file) void pickFile(file);
                event.target.value = '';
              }}
            />
          </div>

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
                  if (!next) setPrompt(defaultPrompt(song));
                }}
                className={CONTROL}
              >
                <option value="">{t('coverPromptFree')}</option>
                {templates.map(entry => <option key={entry.id} value={entry.id}>{entry.name}</option>)}
              </select>
              <button
                type="button"
                onClick={() => setEditingTemplate(current => !current)}
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
                      onClick={() => setTemplateText(current => current + '{' + name + '}')}
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
              disabled={!canGenerate || busy !== null || !prompt.trim()}
              className="mt-2 inline-flex items-center gap-2 rounded-lg bg-gradient-to-r from-orange-500 to-pink-600 px-4 py-2 text-xs font-bold text-white disabled:cursor-not-allowed disabled:opacity-50"
            >
              {busy === 'generate' ? <Loader2 size={14} className="animate-spin" /> : <Sparkles size={14} />} {t('generate')}
            </button>
          </div>

          {error && (
            <p role="alert" className="flex items-center gap-2 rounded-lg bg-rose-500/10 px-3 py-2 text-xs text-rose-700 dark:text-rose-300">
              <AlertTriangle size={14} /> {error}
            </p>
          )}
        </div>

        <div className="flex justify-end gap-2 border-t border-zinc-200 px-5 py-4 dark:border-white/10">
          <button type="button" onClick={onClose} className="rounded-lg px-4 py-2 text-sm font-medium text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200">
            {t('cancel')}
          </button>
          <button
            type="button"
            onClick={() => void save()}
            disabled={!preview || busy !== null}
            className="inline-flex items-center gap-2 rounded-lg bg-zinc-900 px-4 py-2 text-sm font-bold text-white disabled:opacity-50 dark:bg-white dark:text-zinc-900"
          >
            {busy === 'save' ? <Loader2 size={14} className="animate-spin" /> : null} {t('saveCover')}
          </button>
        </div>
      </div>
      {zoomed && (preview?.dataUrl || song.coverUrl) && (
        <ImageLightbox src={(preview?.dataUrl || song.coverUrl) as string} alt={song.title} onClose={() => setZoomed(false)} />
      )}
    </div>
  );
};

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { AlertTriangle, BookOpen, Check, CheckSquare, ChevronDown, Clock, ChevronLeft, ChevronRight, Cpu, Download, FolderInput, FolderOpen, FolderOutput, Headphones, Library, Loader2, Mic2, MoreHorizontal, Music, Play, Plus, RotateCcw, Search, Square, Trash2, UploadCloud, Wand2, X } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { ConfirmDialog } from './ConfirmDialog';
import { StemPlayer } from './StemPlayer';
import { TrainingGuide } from './TrainingGuide';
import {
  Dataset,
  DatasetItem,
  FieldCondition,
  PickedFile,
  PrepareRequest,
  PrepareStatus,
  Recipe,
  RecipeField,
  TrainingRun,
  TrainingState,
  addLibrarySongs,
  addPicked,
  cancelPrepare,
  cancelRun,
  continueRun,
  cancelTrainingPack,
  clock,
  createDataset,
  deleteDataset,
  deleteItem,
  deleteRun,
  describeItem,
  fetchTraining,
  downloadTrainingBase,
  gigabytes,
  importDataset,
  installCheckpoint,
  installListenPack,
  installTrainingPack,
  nameFromPicked,
  pickedFromDrop,
  pickedFromInput,
  prepareDataset,
  revealDataset,
  setTrainAfter,
  startRun,
  updateDataset,
  updateItem,
  usable,
} from '../services/training';

/**
 * Training a LoRA on the user's own songs: the optional pack, a dataset of
 * songs with their style and lyrics, a recipe, and runs whose checkpoints go
 * straight to the LoRA library.
 */

const CARD = 'rounded-2xl border border-zinc-200 bg-zinc-50 p-4 dark:border-white/10 dark:bg-white/[0.03]';
const CONTROL =
  'w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:border-pink-500 disabled:opacity-50 dark:border-white/10 dark:bg-black/30 dark:text-white';
const OUTLINE =
  'inline-flex items-center gap-1.5 rounded-lg border border-zinc-200 px-3 py-1.5 text-xs font-semibold text-zinc-700 hover:border-pink-400 hover:text-pink-600 disabled:cursor-not-allowed disabled:opacity-50 dark:border-white/10 dark:text-zinc-200';
const PRIMARY =
  'inline-flex items-center justify-center gap-2 rounded-lg bg-gradient-to-r from-orange-500 to-pink-600 px-4 py-2 text-sm font-bold text-white disabled:cursor-not-allowed disabled:opacity-50';
const LABEL = 'text-[11px] font-bold uppercase tracking-wide text-zinc-500';

const HINT = 'mt-1 text-[11px] leading-4 text-zinc-500';

/** A number typed freely: the text is kept as typed, a value inside the bounds
 * is taken at once, and leaving the field brings it inside them. */
const NumberField: React.FC<{ label: string; value: number; onChange: (value: number) => void; min?: number; max?: number; step?: number; integer?: boolean; disabled?: boolean; hint?: string }> = ({ label, value, onChange, min, max, step, integer, disabled, hint }) => {
  const [text, setText] = useState(String(value));
  useEffect(() => {
    setText(current => (Number(current) === value && current.trim() !== '' ? current : String(value)));
  }, [value]);
  const inside = (next: number) => (min === undefined || next >= min) && (max === undefined || next <= max);
  const settle = () => {
    const parsed = Number(text);
    if (text.trim() === '' || !Number.isFinite(parsed)) {
      setText(String(value));
      return;
    }
    const bounded = Math.min(max ?? parsed, Math.max(min ?? parsed, integer ? Math.round(parsed) : parsed));
    setText(String(bounded));
    if (bounded !== value) onChange(bounded);
  };
  return (
    <label className="block">
      <span className={LABEL}>{label}</span>
      <input
        type="number"
        value={text}
        min={min}
        max={max}
        step={step}
        disabled={disabled}
        onChange={event => {
          setText(event.target.value);
          const next = Number(event.target.value);
          if (event.target.value.trim() !== '' && Number.isFinite(next) && inside(next) && (!integer || Number.isInteger(next))) onChange(next);
        }}
        onBlur={settle}
        className={`${CONTROL} mt-1`}
      />
      {hint && <span className={`block ${HINT}`}>{hint}</span>}
    </label>
  );
};

const Choice = <T extends string>({ label, value, options, onChange }: { label: string; value: T; options: { value: T; label: string }[]; onChange: (value: T) => void }) => (
  <label className="block">
    <span className={LABEL}>{label}</span>
    <select value={value} onChange={event => onChange(event.target.value as T)} className={`${CONTROL} mt-1`}>
      {options.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
    </select>
  </label>
);

/** Every setting of a run, as the engine lists them, starting from its recipe. */
const RecipeForm: React.FC<{ recipe: Recipe; defaults: Recipe; fields: RecipeField[]; onChange: (recipe: Recipe) => void; only?: string[]; except?: string[] }> = ({ recipe, defaults, fields, onChange, only, except }) => {
  const { tt } = useStrings();
  const holds = (condition?: FieldCondition) => condition !== undefined && condition.values.includes(String(recipe[condition.field]));
  const groups: string[] = [];
  for (const field of fields) {
    const wanted = (!only || only.includes(field.group)) && !except?.includes(field.group);
    if (wanted && !groups.includes(field.group)) groups.push(field.group);
  }
  const changed = JSON.stringify(recipe) !== JSON.stringify(defaults);
  const control = (field: RecipeField) => {
    const label = tt(`trainingField_${field.key}`);
    const off = holds(field.off_when);
    const hint = off ? tt(`trainingHint_${field.key}_off`) : undefined;
    const value = recipe[field.key];
    if (field.kind === 'toggle') {
      return (
        <label key={field.key} className="flex items-center gap-2 pb-2 text-sm text-zinc-700 dark:text-zinc-200">
          <input type="checkbox" checked={value === true} onChange={event => onChange({ ...recipe, [field.key]: event.target.checked })} className="accent-pink-500" />
          {label}
        </label>
      );
    }
    if (field.kind === 'choice') {
      return (
        <React.Fragment key={field.key}>
          <Choice
            label={label}
            value={String(value)}
            options={(field.choices ?? []).map(choice => ({ value: choice, label: tt(`trainingChoice_${choice}`) }))}
            onChange={next => onChange({ ...recipe, [field.key]: next })}
          />
        </React.Fragment>
      );
    }
    return (
      <NumberField
        key={field.key}
        label={label}
        value={Number(value)}
        min={field.min}
        max={field.max}
        step={field.step}
        integer={field.kind === 'integer'}
        disabled={off}
        hint={hint}
        onChange={next => onChange({ ...recipe, [field.key]: next })}
      />
    );
  };
  return (
    <div className="mt-3 space-y-4">
      {groups.map(group => {
        const shown = fields.filter(field => field.group === group && (field.shown_when === undefined || holds(field.shown_when)));
        const hints = shown.map(field => ({ key: field.key, text: tt(`trainingHint_${field.key}`) })).filter(entry => entry.text !== `trainingHint_${entry.key}`);
        return (
          <div key={group}>
            <p className={LABEL}>{tt(`trainingGroup_${group}`)}</p>
            <div className="mt-1.5 grid items-end gap-2 sm:grid-cols-4">{shown.map(control)}</div>
            {hints.map(entry => <p key={entry.key} className={HINT}>{entry.text}</p>)}
          </div>
        );
      })}
      {changed && (
        <button type="button" onClick={() => onChange(defaults)} className={OUTLINE}>{tt('trainingResetRecipe')}</button>
      )}
    </div>
  );
};

function useStrings() {
  const { t, language } = useI18n();
  const tt = t as unknown as (key: string) => string;
  // "1 song", "3 песни", "61 песня": the form each language asks for
  const songs = (count: number) => tt(`trainingSongs_${new Intl.PluralRules(language).select(count)}`).replace('{count}', String(count));
  return { t, tt, songs };
}

const errorText = (problem: unknown) => (problem instanceof Error ? problem.message : String(problem));

/** The pack: what it holds, the requirements, and its download. */
const PackCard: React.FC<{ state: TrainingState; onError: (message: string) => void; onChanged: () => void }> = ({ state, onError, onChanged }) => {
  const { t } = useStrings();
  const total = state.pack.reduce((sum, file) => sum + file.bytes, 0);
  const missing = state.pack.filter(file => !file.installed).reduce((sum, file) => sum + file.bytes, 0);
  const download = state.download && !state.download.done ? state.download : null;
  const percent = download ? Math.min(100, (100 * download.downloaded_bytes) / Math.max(1, download.total_bytes)) : 0;
  return (
    <section className={CARD}>
      <p className="flex items-center gap-2 text-sm font-semibold text-zinc-900 dark:text-white"><Download size={16} className="text-pink-500" />{t('trainingSetupTitle')}</p>
      <p className="mt-2 text-sm text-zinc-600 dark:text-zinc-300">{t('trainingSetupNeeds').replace('{size}', gigabytes(total)).replace('{vram}', String(state.min_vram_gb))}</p>
      <ul className="mt-3 space-y-1.5">
        {state.pack.map(file => (
          <li key={file.id} className="flex items-center justify-between gap-3 text-xs text-zinc-700 dark:text-zinc-200">
            <span className="flex items-center gap-2">{file.installed ? <Check size={13} className="text-emerald-500" /> : <Square size={13} className="text-zinc-400" />}{file.label}</span>
            <span className="tabular-nums text-zinc-500">{gigabytes(file.bytes)}</span>
          </li>
        ))}
      </ul>
      {download ? (
        <div className="mt-3">
          <div className="flex items-center justify-between text-xs text-zinc-500">
            <span className="inline-flex items-center gap-1.5"><Loader2 size={13} className="animate-spin text-pink-500" />{percent.toFixed(1)}%</span>
            <span className="tabular-nums">{gigabytes(download.downloaded_bytes)} / {gigabytes(download.total_bytes)}</span>
          </div>
          <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-zinc-200 dark:bg-white/10">
            <div className="h-full bg-gradient-to-r from-orange-500 to-pink-600 transition-[width]" style={{ width: `${percent}%` }} />
          </div>
          <button type="button" onClick={() => void cancelTrainingPack().then(onChanged)} className={`${OUTLINE} mt-3`}><X size={13} />{t('adaptersCancel')}</button>
        </div>
      ) : (
        !state.pack_ready && (
          <button type="button" onClick={() => void installTrainingPack().then(onChanged).catch(problem => onError(errorText(problem)))} className={`${PRIMARY} mt-3`}>
            <Download size={15} />{t('trainingDownload')} · {gigabytes(missing)}
          </button>
        )
      )}
      {state.download?.done && state.download.error && state.download.error !== 'cancelled' && (
        <p role="alert" className="mt-3 text-xs text-rose-600 dark:text-rose-300">{state.download.error}</p>
      )}
    </section>
  );
};

/** The optional listening pack: songs described by ear instead of by hand. */
const ListenCard: React.FC<{ state: TrainingState; onError: (message: string) => void; onChanged: () => void }> = ({ state, onError, onChanged }) => {
  const { t } = useStrings();
  const listen = state.listen;
  if (!listen || listen.ready) return null;
  const total = listen.pack.reduce((sum, file) => sum + file.bytes, 0);
  const missing = listen.pack.filter(file => !file.installed).reduce((sum, file) => sum + file.bytes, 0);
  const download = listen.download && !listen.download.done ? listen.download : null;
  const percent = download ? Math.min(100, (100 * download.downloaded_bytes) / Math.max(1, download.total_bytes)) : 0;
  return (
    <section className={CARD}>
      <p className="flex items-center gap-2 text-sm font-semibold text-zinc-900 dark:text-white"><Headphones size={16} className="text-pink-500" />{t('trainingListenTitle')}</p>
      <p className="mt-2 text-sm text-zinc-600 dark:text-zinc-300">{t('trainingListenIntro').replace('{size}', gigabytes(total))}</p>
      <ul className="mt-3 space-y-1.5">
        {listen.pack.map(file => (
          <li key={file.id} className="flex items-center justify-between gap-3 text-xs text-zinc-700 dark:text-zinc-200">
            <span className="flex items-center gap-2">{file.installed ? <Check size={13} className="text-emerald-500" /> : <Square size={13} className="text-zinc-400" />}{file.label}</span>
            <span className="tabular-nums text-zinc-500">{gigabytes(file.bytes)}</span>
          </li>
        ))}
      </ul>
      {download ? (
        <div className="mt-3">
          <div className="flex items-center justify-between text-xs text-zinc-500">
            <span className="inline-flex items-center gap-1.5"><Loader2 size={13} className="animate-spin text-pink-500" />{percent.toFixed(1)}%</span>
            <span className="tabular-nums">{gigabytes(download.downloaded_bytes)} / {gigabytes(download.total_bytes)}</span>
          </div>
          <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-zinc-200 dark:bg-white/10">
            <div className="h-full bg-gradient-to-r from-orange-500 to-pink-600 transition-[width]" style={{ width: `${percent}%` }} />
          </div>
          <button type="button" onClick={() => void cancelTrainingPack().then(onChanged)} className={`${OUTLINE} mt-3`}><X size={13} />{t('adaptersCancel')}</button>
        </div>
      ) : (
        <button type="button" onClick={() => void installListenPack().then(onChanged).catch(problem => onError(errorText(problem)))} className={`${OUTLINE} mt-3`}>
          <Download size={14} />{t('trainingDownload')} · {gigabytes(missing)}
        </button>
      )}
      {listen.download?.done && listen.download.error && listen.download.error !== 'cancelled' && (
        <p role="alert" className="mt-3 text-xs text-rose-600 dark:text-rose-300">{listen.download.error}</p>
      )}
    </section>
  );
};

/** Library songs with audio, searchable, several picked at once. */
const LibraryPicker: React.FC<{ exclude: string[]; onAdd: (ids: string[]) => void; onClose: () => void }> = ({ exclude, onAdd, onClose }) => {
  const { t } = useStrings();
  const [library, setLibrary] = useState<{ id: string; title: string; caption: string }[]>([]);
  const [failed, setFailed] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [picked, setPicked] = useState<string[]>([]);
  useEffect(() => {
    void fetch('/v1/library/songs')
      .then(async response => {
        if (!response.ok) throw new Error(`${response.status} ${await response.text()}`);
        return response.json() as Promise<{ id: string; title: string; caption: string; audio_path?: string | null }[]>;
      })
      .then(list => setLibrary(list.filter(song => song.audio_path)))
      .catch((error: unknown) => setFailed(error instanceof Error ? error.message : String(error)));
  }, []);
  const needle = query.trim().toLowerCase();
  const songs = library.filter(song => !exclude.includes(song.id));
  const visible = needle ? songs.filter(song => song.title.toLowerCase().includes(needle) || song.caption.toLowerCase().includes(needle)) : songs;
  return (
    <div className="mt-3 rounded-xl border border-zinc-200 p-3 dark:border-white/10">
      <div className="relative">
        <Search size={14} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-zinc-400" />
        <input value={query} onChange={event => setQuery(event.target.value)} placeholder={t('trainingSearchLibrary')} aria-label={t('trainingSearchLibrary')} className={`${CONTROL} pl-9`} />
      </div>
      {failed && <p className="mt-2 text-xs text-red-500">{t('trainingLibraryFailed')} {failed}</p>}
      <div className="mt-2 max-h-56 overflow-y-auto rounded-lg border border-zinc-200 dark:border-white/10">
        {visible.map(song => {
          const on = picked.includes(song.id);
          return (
            <button
              key={song.id}
              type="button"
              role="checkbox"
              aria-checked={on}
              onClick={() => setPicked(current => (on ? current.filter(id => id !== song.id) : [...current, song.id]))}
              className={`flex w-full items-center gap-2 border-b border-zinc-100 px-3 py-1.5 text-left text-sm last:border-b-0 dark:border-white/5 ${on ? 'bg-pink-500/10' : 'hover:bg-zinc-100 dark:hover:bg-white/5'}`}
            >
              {on ? <CheckSquare size={14} className="shrink-0 text-pink-500" /> : <Square size={14} className="shrink-0 text-zinc-400" />}
              <span className="min-w-0 flex-1 truncate text-zinc-800 dark:text-zinc-200">{song.title}</span>
            </button>
          );
        })}
      </div>
      <div className="mt-2 flex justify-end gap-2">
        <button type="button" onClick={onClose} className={OUTLINE}>{t('adaptersCancel')}</button>
        <button type="button" onClick={() => onAdd(picked)} disabled={picked.length === 0} className={OUTLINE}><Plus size={13} />{t('trainingAddSelected')}{picked.length ? ` · ${picked.length}` : ''}</button>
      </div>
    </div>
  );
};

/** Where one song stands: in the job, failed, done, or missing something. */
type SongState = { kind: 'queued' | 'working' | 'failed' | 'ready' | 'missing'; text: string };

/** Moves vocal separation to the graphics card, keeping the rest of its settings. */
const separateOnCard = async () => {
  const current = await fetch('/v1/separation/settings').then(response => {
    if (!response.ok) throw new Error(`Separation: HTTP ${response.status}`);
    return response.json();
  });
  const response = await fetch('/v1/separation/settings', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ ...current, runtime: 'cuda' }) });
  if (!response.ok) throw new Error(`Separation: HTTP ${response.status}`);
};

/** Turns on the recogniser dataset lyrics need: Whisper large-v3, fetched on its first use. */
const enableRecogniser = () =>
  fetch('/v1/karaoke/status', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ enabled: true, provider: 'whisper', whisper_model: 'whisper-large-v3' }) }).then(response => {
    if (!response.ok) throw new Error(`Karaoke: HTTP ${response.status}`);
  });

function songState(item: DatasetItem, job: PrepareStatus | null, datasetId: string, t: (key: string) => string): SongState {
  const mine = job && job.dataset === datasetId ? job : null;
  const failure = mine?.failures.find(entry => entry.item === item.id);
  const at = mine && !mine.finished ? mine.stages.find(stage => stage.current === item.id) : undefined;
  if (at) return { kind: 'working', text: t(`trainingJob_${at.name}`) };
  if (mine && !mine.finished && mine.pending.includes(item.id)) {
    return { kind: 'queued', text: t('trainingQueued') };
  }
  if (failure) return { kind: 'failed', text: failure.error };
  const lacks = [
    item.lyrics_state === 'wanted' ? t('trainingLacksLyrics') : '',
    item.lyrics_state === 'found' ? t('trainingLacksSections') : '',
    item.style_state !== 'done' ? t('trainingLacksStyle') : '',
  ].filter(Boolean);
  if (lacks.length) return { kind: 'missing', text: `${t('trainingLacks')} ${lacks.join(', ')}` };
  return { kind: 'ready', text: t('trainingSongReady') };
}

const StateIcon: React.FC<{ state: SongState }> = ({ state }) => {
  if (state.kind === 'working') return <Loader2 size={14} className="shrink-0 animate-spin text-pink-500" />;
  if (state.kind === 'queued') return <Clock size={14} className="shrink-0 text-zinc-400" />;
  if (state.kind === 'ready') return <Check size={14} className="shrink-0 text-emerald-500" />;
  return <AlertTriangle size={14} className="shrink-0 text-amber-500" />;
};

/** A place to drop a folder of songs or single files, or click to choose them. */
const DropZone: React.FC<{ onPicked: (files: PickedFile[]) => void; disabled?: boolean; large?: boolean; title: string; hint?: string }> = ({ onPicked, disabled, large, title, hint }) => {
  const { t } = useStrings();
  const [over, setOver] = useState(false);
  const folderInput = useRef<HTMLInputElement | null>(null);
  const fileInput = useRef<HTMLInputElement | null>(null);
  useEffect(() => {
    folderInput.current?.setAttribute('webkitdirectory', '');
  }, []);
  const take = (files: PickedFile[]) => {
    const kept = usable(files);
    if (kept.length > 0) onPicked(kept);
  };
  return (
    <div
      onDragOver={event => { event.preventDefault(); if (!disabled) setOver(true); }}
      onDragLeave={() => setOver(false)}
      onDrop={event => {
        event.preventDefault();
        setOver(false);
        if (!disabled) void pickedFromDrop(event.dataTransfer.items).then(take);
      }}
      className={`rounded-2xl border-2 border-dashed text-center transition-colors ${large ? 'px-6 py-14' : 'px-4 py-4'} ${over ? 'border-pink-500 bg-pink-500/5' : 'border-zinc-300 dark:border-white/15'} ${disabled ? 'pointer-events-none opacity-50' : ''}`}
    >
      {large && <UploadCloud size={36} className="mx-auto mb-3 text-pink-500" />}
      <p className={`${large ? 'text-base font-semibold text-zinc-900 dark:text-white' : 'flex items-center justify-center gap-2 text-sm text-zinc-600 dark:text-zinc-300'}`}>
        {!large && <UploadCloud size={15} className="text-pink-500" />}{title}
      </p>
      {hint && <p className="mx-auto mt-1 max-w-xl text-xs text-zinc-500">{hint}</p>}
      <div className={`${large ? 'mt-4' : 'mt-2'} flex flex-wrap justify-center gap-2`}>
        <button type="button" onClick={() => folderInput.current?.click()} className={OUTLINE}><FolderOpen size={13} />{t('trainingDropFolder')}</button>
        <button type="button" onClick={() => fileInput.current?.click()} className={OUTLINE}><Music size={13} />{t('trainingDropFiles')}</button>
      </div>
      <input ref={folderInput} type="file" multiple className="hidden" onChange={event => { take(pickedFromInput(event.target.files)); event.target.value = ''; }} />
      <input ref={fileInput} type="file" multiple accept="audio/*,.wav,.mp3,.flac,.ogg,.m4a,.txt,.lrc,.cue" className="hidden" onChange={event => { take(pickedFromInput(event.target.files)); event.target.value = ''; }} />
    </div>
  );
};

/** A small menu behind a "more" button. */
const MoreMenu: React.FC<{ items: { label: string; icon: React.ReactNode; onClick: () => void; danger?: boolean; disabled?: boolean }[] }> = ({ items }) => {
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => { if (!box.current?.contains(event.target as Node)) setOpen(false); };
    const escape = (event: KeyboardEvent) => { if (event.key === 'Escape') setOpen(false); };
    window.addEventListener('mousedown', close);
    window.addEventListener('keydown', escape);
    return () => {
      window.removeEventListener('mousedown', close);
      window.removeEventListener('keydown', escape);
    };
  }, [open]);
  return (
    <div ref={box} className="relative">
      <button type="button" onClick={() => setOpen(value => !value)} className={`${OUTLINE} px-2`} aria-expanded={open}><MoreHorizontal size={15} /></button>
      {open && (
        <div className="absolute right-0 z-30 mt-1 w-72 overflow-hidden rounded-xl border border-zinc-200 bg-white py-1 shadow-xl dark:border-white/10 dark:bg-zinc-900">
          {items.map(item => (
            <button
              key={item.label}
              type="button"
              disabled={item.disabled}
              onClick={() => { setOpen(false); item.onClick(); }}
              className={`flex w-full items-center gap-2 whitespace-nowrap px-3 py-2 text-left text-sm disabled:opacity-40 ${item.danger ? 'text-rose-600 hover:bg-rose-500/10 dark:text-rose-300' : 'text-zinc-700 hover:bg-zinc-100 dark:text-zinc-200 dark:hover:bg-white/5'}`}
            >
              {item.icon}{item.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
};

/** One song opened in the list: its recording, style and lyrics to check and
 * fix, and its own "do again" buttons. */
const SongEditor: React.FC<{
  datasetId: string;
  item: DatasetItem;
  styleKind: 'style' | 'caption';
  state: SongState;
  listenReady: boolean;
  jobBusy: boolean;
  describing: boolean;
  onPrepare: (request: PrepareRequest) => void;
  onDescribe: () => void;
  onChanged: (dataset: Dataset) => void;
  onError: (message: string) => void;
}> = ({ datasetId, item, styleKind, state, listenReady, jobBusy, describing, onPrepare, onDescribe, onChanged, onError }) => {
  const { t } = useStrings();
  const [title, setTitle] = useState(item.title);
  const [artist, setArtist] = useState(item.artist);
  const [style, setStyle] = useState(item.style);
  const [lyrics, setLyrics] = useState(item.lyrics);
  useEffect(() => { setTitle(item.title); setArtist(item.artist); setStyle(item.style); setLyrics(item.lyrics); }, [item.id, item.title, item.artist, item.style, item.lyrics]);
  const save = (patch: Parameters<typeof updateItem>[2]) => void updateItem(datasetId, item.id, patch).then(onChanged).catch(problem => onError(errorText(problem)));
  const working = state.kind === 'working' || state.kind === 'queued';
  const redo = 'inline-flex items-center gap-1 text-[11px] font-semibold text-zinc-500 hover:text-pink-500 disabled:opacity-50';
  return (
    <div className="space-y-3 pb-4 pl-7 pr-1 pt-1">
      {state.kind !== 'ready' && (
        <div className="flex flex-wrap items-start gap-2">
          <p className={`min-w-0 flex-1 text-xs ${state.kind === 'working' ? 'text-pink-600 dark:text-pink-300' : state.kind === 'queued' ? 'text-zinc-500' : 'text-amber-600 dark:text-amber-300'}`}>{state.text}</p>
          {(state.kind === 'failed' || state.kind === 'missing') && (
            <button type="button" onClick={() => onPrepare({ items: [item.id], lyrics: 'missing', style: 'missing' })} disabled={jobBusy} title={jobBusy ? t('trainingRedoBusy') : undefined} className={OUTLINE}>
              <RotateCcw size={13} />{t(state.kind === 'failed' ? 'trainingRetrySong' : 'trainingFinishSong')}
            </button>
          )}
        </div>
      )}
      <StemPlayer src={`/v1/training/datasets/${datasetId}/items/${item.id}/audio`} label={t('trainingRecording')} />
      <div className="grid gap-2 sm:grid-cols-2">
        <label className="block">
          <span className={LABEL}>{t('trainingSongTitle')}</span>
          <input value={title} onChange={event => setTitle(event.target.value)} onBlur={() => title.trim() && title !== item.title && save({ title })} className={`${CONTROL} mt-1`} />
        </label>
        <label className="block">
          <span className={LABEL}>{t('trainingSongArtist')}</span>
          <input value={artist} onChange={event => setArtist(event.target.value)} onBlur={() => artist !== item.artist && save({ artist })} className={`${CONTROL} mt-1`} />
        </label>
      </div>
      <label className="block">
        <span className="flex items-center justify-between gap-2">
          <span className={LABEL}>{styleKind === 'caption' ? t('trainingCaption') : t('trainingStyle')}</span>
          {listenReady ? (
            <button type="button" onClick={() => onPrepare({ items: [item.id], lyrics: 'none', style: 'all' })} disabled={jobBusy} title={jobBusy ? t('trainingRedoBusy') : undefined} className={redo}><Headphones size={12} />{t('trainingListenAgain')}</button>
          ) : styleKind === 'caption' ? (
            <button type="button" onClick={onDescribe} disabled={describing} className={redo}>{describing ? <Loader2 size={12} className="animate-spin" /> : <Wand2 size={12} />}{t('trainingDescribe')}</button>
          ) : null}
        </span>
        <textarea value={style} disabled={working} onChange={event => setStyle(event.target.value)} onBlur={() => style !== item.style && save({ style })} rows={styleKind === 'caption' ? 10 : 3} className={`${CONTROL} mt-1 text-xs ${styleKind === 'caption' ? 'font-mono' : ''}`} />
        {styleKind === 'caption' && <span className={`block ${HINT}`}>{t('trainingCaptionHint')}</span>}
      </label>
      <label className="flex items-center gap-2 text-sm text-zinc-700 dark:text-zinc-200">
        <input type="checkbox" checked={item.instrumental} disabled={working} onChange={event => save({ instrumental: event.target.checked })} className="accent-pink-500" />
        {t('trainingInstrumental')}
      </label>
      {!item.instrumental && (
        <label className="block">
          <span className="flex items-center justify-between gap-2">
            <span className={LABEL}>
              {t('trainingLyrics')}
              {item.lyrics_source && <span className="ml-2 normal-case tracking-normal text-zinc-400">{item.lyrics_source === 'recognised' ? t('trainingLyricsRecognised') : t('trainingLyricsFrom').replace('{source}', item.lyrics_source)}</span>}
            </span>
            <button type="button" onClick={() => onPrepare({ items: [item.id], lyrics: 'all', style: 'none' })} disabled={jobBusy} title={jobBusy ? t('trainingRedoBusy') : undefined} className={redo}><Mic2 size={12} />{t('trainingRecogniseAgain')}</button>
          </span>
          <textarea value={lyrics} disabled={working} onChange={event => setLyrics(event.target.value)} onBlur={() => lyrics !== item.lyrics && save({ lyrics })} rows={14} className={`${CONTROL} mt-1 font-mono text-xs`} />
        </label>
      )}
    </div>
  );
};

/** Step one: the songs, prepared by themselves as soon as they arrive. */
const SongsStep: React.FC<{
  state: TrainingState;
  dataset: Dataset;
  job: PrepareStatus | null;
  adding: { done: number; total: number } | null;
  onAdd: (files: PickedFile[]) => void;
  onAddLibrary: (ids: string[]) => void;
  onPrepare: (request: PrepareRequest) => void;
  onNext: () => void;
  onTrainAfter: (on: boolean) => void;
  onChanged: (dataset: Dataset) => void;
  onRefresh: () => void;
  onError: (message: string) => void;
}> = ({ state, dataset, job, adding, onAdd, onAddLibrary, onPrepare, onNext, onTrainAfter, onChanged, onRefresh, onError }) => {
  const { t, tt, songs } = useStrings();
  const [open, setOpen] = useState<string | null>(null);
  const [picking, setPicking] = useState(false);
  const [describing, setDescribing] = useState<string[]>([]);
  const mine = job && job.dataset === dataset.id ? job : null;
  const jobBusy = Boolean(job && !job.finished) || Boolean(state.active);
  const listenReady = Boolean(state.listen?.ready);
  const states = dataset.items.map(item => songState(item, job, dataset.id, tt));
  const ready = states.filter(entry => entry.kind === 'ready').length;
  const total = dataset.items.reduce((sum, item) => sum + item.seconds, 0);
  const exclude = dataset.items.map(item => item.source.replace(/^song:/, ''));
  const working = Boolean(mine && !mine.finished);
  const stage = working ? mine!.stages[mine!.stages.length - 1] : undefined;
  const describe = async (ids: string[]) => {
    setDescribing(ids);
    for (const id of ids) {
      try {
        onChanged(await describeItem(dataset.id, id));
      } catch (problem) {
        onError(errorText(problem));
        break;
      } finally {
        setDescribing(current => current.filter(value => value !== id));
      }
    }
  };

  if (dataset.items.length === 0 && !adding) {
    return (
      <div className="space-y-3">
        <DropZone large onPicked={onAdd} title={t('trainingDropTitle')} hint={t('trainingDropHint')} />
        <div className="flex justify-center"><button type="button" onClick={() => setPicking(value => !value)} className={OUTLINE}><Library size={13} />{t('trainingAddLibrary')}</button></div>
        {picking && <LibraryPicker exclude={exclude} onAdd={ids => { setPicking(false); onAddLibrary(ids); }} onClose={() => setPicking(false)} />}
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {!listenReady && <ListenCard state={state} onError={onError} onChanged={onRefresh} />}
      {mine?.notices.filter(notice => notice !== 'listen_missing').map(notice => (
        <div key={notice} className="flex flex-wrap items-center gap-3 rounded-xl border border-amber-500/30 bg-amber-500/10 px-3 py-2">
          <p className="min-w-0 flex-1 text-xs text-amber-700 dark:text-amber-300">{tt(`trainingNotice_${notice}`)}</p>
          {notice === 'separator_on_cpu' && (
            <button type="button" onClick={() => void separateOnCard().catch(problem => onError(errorText(problem)))} className={OUTLINE}>
              <Cpu size={13} />{t('trainingSeparateOnCard')}
            </button>
          )}
          {notice === 'recogniser_missing' && (
            <button type="button" onClick={() => void enableRecogniser().then(() => onPrepare({ lyrics: 'missing', style: 'none' })).catch(problem => onError(errorText(problem)))} disabled={jobBusy} className={OUTLINE}>
              <Mic2 size={13} />{t('trainingEnableRecogniser')}
            </button>
          )}
        </div>
      ))}

      <section className={CARD}>
        <div className="flex flex-wrap items-center gap-3">
          <div className="min-w-0 flex-1">
            <p className="text-sm font-semibold text-zinc-900 dark:text-white">
              {adding
                ? t('trainingUploading').replace('{done}', String(adding.done)).replace('{total}', String(adding.total))
                : working
                  ? stage
                    ? `${tt(`trainingJob_${stage.name}`)}${stage.total ? ` · ${stage.done}/${stage.total}` : ''}`
                    : tt('trainingJob_job')
                  : t('trainingReadyCount').replace('{ready}', String(ready)).replace('{count}', String(dataset.items.length))}
            </p>
            <p className="text-[11px] text-zinc-500">{t('trainingSongsTotal').replace('{songs}', songs(dataset.items.length)).replace('{time}', clock(total))}</p>
          </div>
          {working && <button type="button" onClick={() => void cancelPrepare().then(onRefresh)} className={OUTLINE}><X size={13} />{t('trainingStop')}</button>}
          {!working && !adding && ready < dataset.items.length && (
            <button type="button" onClick={() => onPrepare({ lyrics: 'missing', style: 'missing' })} disabled={jobBusy} className={OUTLINE}><Wand2 size={13} />{t('trainingFillMissing')}</button>
          )}
          <MoreMenu
            items={[
              { label: t('trainingAddLibrary'), icon: <Library size={14} />, onClick: () => setPicking(true), disabled: Boolean(adding) },
              { label: t('trainingAutofillAll'), icon: <Mic2 size={14} />, onClick: () => onPrepare({ lyrics: 'all', style: 'missing' }), disabled: jobBusy },
              ...(listenReady ? [{ label: t('trainingListenAll'), icon: <Headphones size={14} />, onClick: () => onPrepare({ lyrics: 'missing', style: 'all' }), disabled: jobBusy }] : []),
              ...(state.item_style === 'caption' && !listenReady ? [{ label: t('trainingDescribeAll'), icon: <Wand2 size={14} />, onClick: () => void describe(dataset.items.map(item => item.id)), disabled: describing.length > 0 }] : []),
            ]}
          />
        </div>
        {(working || adding) && (
          <div className="mt-3 h-1.5 overflow-hidden rounded-full bg-zinc-200 dark:bg-white/10">
            <div
              className="h-full bg-gradient-to-r from-orange-500 to-pink-600 transition-[width]"
              style={{ width: `${adding ? (100 * adding.done) / Math.max(1, adding.total) : stage ? (100 * stage.done) / Math.max(1, stage.total) : 0}%` }}
            />
          </div>
        )}
        {picking && <LibraryPicker exclude={exclude} onAdd={ids => { setPicking(false); onAddLibrary(ids); }} onClose={() => setPicking(false)} />}

        <div className="mt-3 divide-y divide-zinc-200 dark:divide-white/10">
          {dataset.items.map((item, index) => {
            const expanded = open === item.id;
            return (
              <div key={item.id}>
                <div className="flex items-center gap-3 py-2">
                  <button type="button" onClick={() => setOpen(expanded ? null : item.id)} className="flex min-w-0 flex-1 items-center gap-3 text-left" aria-expanded={expanded}>
                    <ChevronDown size={14} className={`shrink-0 text-zinc-400 transition-transform ${expanded ? 'rotate-180' : ''}`} />
                    <StateIcon state={states[index]} />
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-sm text-zinc-900 dark:text-zinc-100">{item.title}</span>
                      {!expanded && (
                        <span className={`block truncate text-[11px] ${states[index].kind === 'ready' ? 'text-zinc-500' : states[index].kind === 'working' ? 'text-pink-600 dark:text-pink-300' : states[index].kind === 'queued' ? 'text-zinc-500' : 'text-amber-600 dark:text-amber-300'}`}>
                          {states[index].kind === 'ready' ? `${item.instrumental ? `${t('trainingInstrumental')} · ` : ''}${item.style}` : states[index].text}
                        </span>
                      )}
                    </span>
                  </button>
                  <span className="shrink-0 text-[11px] tabular-nums text-zinc-500">{clock(item.seconds)}</span>
                  <button
                    type="button"
                    disabled={states[index].kind === 'working'}
                    onClick={() => void deleteItem(dataset.id, item.id).then(onChanged).catch(problem => onError(errorText(problem)))}
                    className="shrink-0 p-1 text-zinc-400 hover:text-rose-500 disabled:opacity-40"
                    title={t('trainingDelete')}
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
                {expanded && (
                  <SongEditor
                    datasetId={dataset.id}
                    item={item}
                    styleKind={state.item_style}
                    state={states[index]}
                    listenReady={listenReady}
                    jobBusy={jobBusy}
                    describing={describing.includes(item.id)}
                    onPrepare={onPrepare}
                    onDescribe={() => void describe([item.id])}
                    onChanged={onChanged}
                    onError={onError}
                  />
                )}
              </div>
            );
          })}
        </div>
        <div className="mt-3">
          <DropZone onPicked={onAdd} disabled={Boolean(adding)} title={t('trainingDropMore')} />
        </div>
      </section>

      <div className="sticky bottom-0 z-10 flex flex-wrap items-center justify-end gap-3 rounded-2xl border border-zinc-200 bg-white/90 p-3 backdrop-blur dark:border-white/10 dark:bg-zinc-950/90">
        {working && (
          <label className="mr-auto flex items-center gap-2 text-xs text-zinc-700 dark:text-zinc-200">
            <input type="checkbox" checked={Boolean(mine?.train_after)} disabled={!state.pack_ready} onChange={event => onTrainAfter(event.target.checked)} className="accent-pink-500" />
            {state.pack_ready ? t('trainingTrainWhenReady') : t('trainingTrainWhenReadyNeedsPack')}
          </label>
        )}
        <button type="button" onClick={onNext} className={PRIMARY}>{t('trainingNextTrain')}<ChevronRight size={15} /></button>
      </div>

    </div>
  );
};

/** Step two: what will be trained, checked, and the button that starts it. */
const TrainStep: React.FC<{ state: TrainingState; dataset: Dataset; job: PrepareStatus | null; onBack: () => void; onStarted: () => void; onChanged: (dataset: Dataset) => void; onRefresh: () => void; onError: (message: string) => void }> = ({ state, dataset, job, onBack, onStarted, onChanged, onRefresh, onError }) => {
  const { t, songs } = useStrings();
  const [advanced, setAdvanced] = useState(false);
  const [recipe, setRecipe] = useState<Recipe | null>(null);
  const [starting, setStarting] = useState(false);
  const unsung = dataset.items.filter(item => item.lyrics_state !== 'done').length;
  const unstyled = dataset.items.filter(item => item.style_state !== 'done').length;
  const total = dataset.items.reduce((sum, item) => sum + item.seconds, 0);
  const preparing = Boolean(job && !job.finished);
  const baseMissing = Boolean(state.base && !state.base.installed);
  const blocker = !dataset.items.length ? t('trainingNeedSongs') : unsung ? t('trainingNeedLyrics').replace('{count}', String(unsung)) : preparing ? t('trainingWaitPrepare') : state.active ? t('trainingBusy') : baseMissing ? t('trainingBaseMissing') : null;
  // the BF16 base downloads through the model manager; its progress is read from there
  const [baseDownload, setBaseDownload] = useState<{ downloaded_bytes: number; total_bytes: number } | null>(null);
  const fetchingBase = baseDownload !== null;
  useEffect(() => {
    if (!fetchingBase) return;
    const timer = window.setInterval(() => {
      void fetch('/setup/status')
        .then(response => response.json())
        .then((status: { active?: { status: string; downloaded_bytes: number; total_bytes: number; error?: string | null } | null }) => {
          const active = status.active;
          if (active?.status === 'downloading') {
            setBaseDownload(active);
            return;
          }
          setBaseDownload(null);
          if (active?.status === 'failed' || active?.status === 'cancelled') onError(active.error || active.status);
          onRefresh();
        })
        .catch((problem: unknown) => onError(errorText(problem)));
    }, 1000);
    return () => window.clearInterval(timer);
  }, [fetchingBase, onRefresh, onError]);
  const start = async () => {
    setStarting(true);
    try {
      await startRun(dataset.id, dataset.name, recipe ?? state.recipe_defaults);
      onStarted();
    } catch (problem) {
      onError(errorText(problem));
    } finally {
      setStarting(false);
    }
  };
  const check = (ok: boolean, text: string) => (
    <li className="flex items-center gap-2 text-sm text-zinc-700 dark:text-zinc-200">{ok ? <Check size={14} className="text-emerald-500" /> : <AlertTriangle size={14} className="text-amber-500" />}{text}</li>
  );
  return (
    <div className="space-y-3">
      {!state.pack_ready && <PackCard state={state} onError={onError} onChanged={onRefresh} />}
      <section className={CARD}>
        <div className="grid gap-3 sm:grid-cols-2">
          <label>
            <span className={LABEL}>{t('trainingLoraName')}</span>
            <input key={`${dataset.id}-name`} defaultValue={dataset.name} onBlur={event => event.target.value.trim() && event.target.value !== dataset.name && void updateDataset(dataset.id, { name: event.target.value }).then(onChanged).catch(problem => onError(errorText(problem)))} className={`${CONTROL} mt-1`} />
          </label>
          <label>
            <span className={LABEL}>{t('trainingTrigger')}</span>
            <input key={`${dataset.id}-trigger`} defaultValue={dataset.trigger} onBlur={event => event.target.value !== dataset.trigger && void updateDataset(dataset.id, { trigger: event.target.value }).then(onChanged).catch(problem => onError(errorText(problem)))} className={`${CONTROL} mt-1`} />
            <span className={`block ${HINT}`}>{t('trainingTriggerHint')}</span>
          </label>
        </div>
        <ul className="mt-4 space-y-1.5">
          {check(dataset.items.length > 0, t('trainingSongsTotal').replace('{songs}', songs(dataset.items.length)).replace('{time}', clock(total)))}
          {check(unsung === 0, unsung ? t('trainingNeedLyrics').replace('{count}', String(unsung)) : t('trainingCheckLyrics'))}
          {check(unstyled === 0, unstyled ? t('trainingNeedStyle').replace('{count}', String(unstyled)) : t('trainingCheckStyle'))}
        </ul>
        {state.base !== undefined && (
          <div className="mt-3 text-xs text-zinc-600 dark:text-zinc-300">
            {t('trainingBase')}: <b className="font-mono text-[11px]">{state.base ? state.base.dit.replace(/\.gguf$/, '') : '—'}</b>
            <span className={`block ${HINT}`}>{t('trainingBaseHint')}</span>
            {state.base && !state.base.installed && (
              baseDownload ? (
                <div className="mt-2 max-w-sm">
                  <div className="flex justify-between text-[11px] tabular-nums text-zinc-500"><span>{t('trainingBaseDownloading')}</span><span>{gigabytes(baseDownload.downloaded_bytes)} / {gigabytes(baseDownload.total_bytes)}</span></div>
                  <div className="mt-1 h-1 overflow-hidden rounded-full bg-zinc-200 dark:bg-white/10"><div className="h-full bg-gradient-to-r from-orange-500 to-pink-600" style={{ width: `${baseDownload.total_bytes ? (100 * baseDownload.downloaded_bytes) / baseDownload.total_bytes : 0}%` }} /></div>
                </div>
              ) : (
                <button
                  type="button"
                  onClick={() => void downloadTrainingBase(state.base!.download).then(() => setBaseDownload({ downloaded_bytes: 0, total_bytes: state.base!.bytes })).catch(problem => onError(errorText(problem)))}
                  className={`${OUTLINE} mt-2`}
                >
                  <Download size={13} />{t('trainingBaseDownload').replace('{size}', gigabytes(state.base.bytes))}
                </button>
              )
            )}
          </div>
        )}
        <RecipeForm recipe={recipe ?? state.recipe_defaults} defaults={state.recipe_defaults} fields={state.recipe_fields} onChange={setRecipe} only={['stop']} />
        {(recipe ?? state.recipe_defaults).stop === 'epochs' && (
          <p className={HINT}>
            {t('trainingEpochSteps')
              .replace('{epochs}', String((recipe ?? state.recipe_defaults).epochs))
              .replace('{songs}', String(dataset.items.length))
              .replace('{steps}', String(Number((recipe ?? state.recipe_defaults).epochs) * dataset.items.length))}
          </p>
        )}
        <button type="button" onClick={() => setAdvanced(value => !value)} className="mt-4 inline-flex items-center gap-1 text-xs font-semibold text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200" aria-expanded={advanced}>
          <ChevronDown size={13} className={`transition-transform ${advanced ? 'rotate-180' : ''}`} />{t('trainingAdvanced')}
        </button>
        {advanced && <RecipeForm recipe={recipe ?? state.recipe_defaults} defaults={state.recipe_defaults} fields={state.recipe_fields} onChange={setRecipe} except={['stop']} />}
      </section>
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-zinc-200 bg-white/90 p-3 dark:border-white/10 dark:bg-zinc-950/90">
        <button type="button" onClick={onBack} className={OUTLINE}><ChevronLeft size={14} />{t('trainingBackSongs')}</button>
        <div className="flex items-center gap-3">
          {blocker && <span className="text-xs text-zinc-500">{blocker}</span>}
          <button type="button" onClick={() => void start()} disabled={Boolean(blocker) || !state.pack_ready || starting} className={PRIMARY}>
            {starting ? <Loader2 size={15} className="animate-spin" /> : <Play size={15} />}{t('trainingStart')}
          </button>
        </div>
      </div>
    </div>
  );
};

/** The datasets as cards, and the drop zone that makes a new one. */
const DatasetList: React.FC<{ state: TrainingState; busy: boolean; onOpen: (id: string) => void; onNew: (files: PickedFile[]) => void; onImport: (files: File[]) => void; importing: boolean }> = ({ state, busy, onOpen, onNew, onImport, importing }) => {
  const { t, songs } = useStrings();
  const importPicker = useRef<HTMLInputElement | null>(null);
  useEffect(() => {
    importPicker.current?.setAttribute('webkitdirectory', '');
  }, []);
  const job = state.prepare;
  return (
    <div className="space-y-4">
      <DropZone large onPicked={onNew} disabled={busy} title={t('trainingDropTitle')} hint={t('trainingDropHint')} />
      {state.datasets.length > 0 && (
        <div className="flex items-center justify-between">
          <p className={LABEL}>{t('trainingDatasets')}</p>
          <button type="button" onClick={() => importPicker.current?.click()} disabled={importing} className={OUTLINE} title={t('trainingImportHint')}>
            {importing ? <Loader2 size={13} className="animate-spin" /> : <FolderInput size={13} />}{t('trainingImport')}
          </button>
        </div>
      )}
      <input ref={importPicker} type="file" multiple className="hidden" onChange={event => { onImport(Array.from(event.target.files ?? [])); event.target.value = ''; }} />
      <div className="grid gap-3 sm:grid-cols-2">
        {state.datasets.map(dataset => {
          const ready = dataset.items.filter(item => item.lyrics_state === 'done' && item.style_state === 'done').length;
          const preparing = job && !job.finished && job.dataset === dataset.id;
          const training = state.runs.some(run => run.dataset_id === dataset.id && run.status === 'running');
          const minutes = Math.round(dataset.items.reduce((sum, item) => sum + item.seconds, 0) / 60);
          return (
            <button key={dataset.id} type="button" onClick={() => onOpen(dataset.id)} className={`${CARD} text-left transition-colors hover:border-pink-400/60`}>
              <p className="truncate text-sm font-semibold text-zinc-900 dark:text-white">{dataset.name}</p>
              <p className="mt-1 text-xs text-zinc-500">{t('trainingCardSongs').replace('{songs}', songs(dataset.items.length)).replace('{minutes}', String(minutes))}</p>
              <p className="mt-2 flex items-center gap-1.5 text-xs">
                {preparing ? <><Loader2 size={12} className="animate-spin text-pink-500" /><span className="text-pink-600 dark:text-pink-300">{t('trainingCardPreparing')}</span></>
                  : training ? <><Loader2 size={12} className="animate-spin text-pink-500" /><span className="text-pink-600 dark:text-pink-300">{t('trainingCardTraining')}</span></>
                  : ready === dataset.items.length && ready > 0 ? <><Check size={12} className="text-emerald-500" /><span className="text-emerald-600 dark:text-emerald-400">{t('trainingCardReady')}</span></>
                  : <><AlertTriangle size={12} className="text-amber-500" /><span className="text-amber-600 dark:text-amber-300">{t('trainingReadyCount').replace('{ready}', String(ready)).replace('{count}', String(dataset.items.length))}</span></>}
              </p>
            </button>
          );
        })}
      </div>
    </div>
  );
};

/** The loss over the steps so far, as a line. */
const LossLine: React.FC<{ steps: TrainingRun['steps'] }> = ({ steps }) => {
  if (steps.length < 2) return null;
  const losses = steps.map(step => step.loss);
  const low = Math.min(...losses);
  const high = Math.max(...losses);
  const span = Math.max(1e-9, high - low);
  const last = steps[steps.length - 1].step;
  const points = steps.map(step => `${(100 * step.step) / last},${36 - (34 * (step.loss - low)) / span}`).join(' ');
  return (
    <svg viewBox="0 0 100 38" preserveAspectRatio="none" className="mt-2 h-16 w-full" aria-hidden>
      <polyline points={points} fill="none" stroke="currentColor" strokeWidth="1.2" vectorEffect="non-scaling-stroke" className="text-pink-500" />
    </svg>
  );
};

/** Trains a stopped or finished run further: the steps it is to reach in
 *  all, from the checkpoint the server says it can go on from. */
const ContinueRun: React.FC<{ run: TrainingRun; epochs: boolean; onChanged: () => void; onError: (message: string) => void }> = ({ run, epochs, onChanged, onError }) => {
  const { t, tt } = useStrings();
  const from = run.resume_step;
  const [steps, setSteps] = useState('');
  const [starting, setStarting] = useState(false);
  if (from === undefined) {
    return run.resume_refused && run.checkpoints.length > 0 ? <p className="mt-3 text-[11px] leading-4 text-zinc-500">{tt(`trainingResume_${run.resume_refused}`)}</p> : null;
  }
  const suggested = epochs ? from + 100 : from + Math.max(Number(run.recipe.save_every) || 1, 1) * 4;
  const target = Number(steps || suggested);
  const valid = Number.isInteger(target) && target > from;
  const start = () => {
    if (!valid || starting) return;
    setStarting(true);
    void continueRun(run.id, target)
      .then(onChanged)
      .catch(problem => onError(errorText(problem)))
      .finally(() => setStarting(false));
  };
  return (
    <div className="mt-3 rounded-lg border border-zinc-200 p-2.5 dark:border-white/10">
      <div className="flex flex-wrap items-center gap-2">
        <button type="button" onClick={start} disabled={!valid || starting} className={OUTLINE}>
          {starting ? <Loader2 size={13} className="animate-spin" /> : <Play size={13} />}
          {t('trainingContinue')}
        </button>
        <label className="flex items-center gap-1.5 text-[11px] text-zinc-500">
          {t('trainingContinueTo')}
          <input
            value={steps}
            onChange={event => setSteps(event.target.value.replace(/[^0-9]/g, ''))}
            onKeyDown={event => { if (event.key === 'Enter') start(); }}
            placeholder={String(suggested)}
            inputMode="numeric"
            aria-label={t('trainingContinueTo')}
            className="w-24 rounded-md border border-zinc-200 bg-white px-2 py-1 text-xs tabular-nums text-zinc-900 outline-none focus:border-pink-500 dark:border-white/10 dark:bg-black/30 dark:text-white"
          />
        </label>
      </div>
      <p className="mt-1.5 text-[11px] leading-4 text-zinc-500">{tt(epochs ? 'trainingContinueHintEpochs' : 'trainingContinueHint').replace('{step}', String(from))}</p>
    </div>
  );
};

const RunCard: React.FC<{ run: TrainingRun; epochs: boolean; onChanged: () => void; onError: (message: string) => void; onDelete: () => void }> = ({ run, epochs, onChanged, onError, onDelete }) => {
  const { t, tt } = useStrings();
  const running = run.status === 'running';
  const last = run.steps[run.steps.length - 1];
  // the trainer counts the run's steps itself where it can
  const cap = Number(last?.total) || Number(run.recipe.steps) || 1;
  // an engine that stops on drift reports it per step; the stop is on the mean of the last 20
  const target = Number(run.recipe.target_kl ?? 0);
  const window = run.steps.slice(-20).map(step => step.ar_kl).filter((kl): kl is number => typeof kl === 'number');
  const kl = window.length ? window.reduce((a, b) => a + b, 0) / window.length : null;
  const recent = run.steps.slice(-10).map(step => step.step_ms).filter((ms): ms is number => typeof ms === 'number');
  const perStep = recent.length ? recent.reduce((a, b) => a + b, 0) / recent.length / 1000 : 0;
  const left = last && perStep && target <= 0 ? (cap - last.step) * perStep : 0;
  const percent = last ? Math.min(100, 100 * Math.max(last.step / cap, target > 0 && kl !== null ? kl / target : 0)) : 0;
  const stageIndex = run.stage ? run.stages.indexOf(run.stage) : run.status === 'done' ? run.stages.length : -1;
  const tone = { running: 'text-pink-600 dark:text-pink-300', done: 'text-emerald-600 dark:text-emerald-400', failed: 'text-rose-600 dark:text-rose-300', cancelled: 'text-zinc-500', interrupted: 'text-amber-600 dark:text-amber-300' }[run.status];
  return (
    <section className={CARD}>
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <p className="truncate text-sm font-semibold text-zinc-900 dark:text-white">{run.name}</p>
          <p className="mt-0.5 text-[11px] text-zinc-500">{run.dataset_name}{run.trigger ? ` · ${run.trigger}` : ''} · {target > 0 ? `KL ${target} · ` : ''}{target > 0 ? '≤ ' : ''}{epochs ? `${run.recipe.epochs} ${t('trainingEpochsUnit')}` : `${cap} ${t('trainingSteps')}`}{run.base ? ` · ${run.base.replace(/\.gguf$/, '')}` : ''}</p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <span className={`inline-flex items-center gap-1 text-xs font-semibold ${tone}`}>{running && <Loader2 size={12} className="animate-spin" />}{tt(`trainingStatus_${run.status}`)}</span>
          {running ? (
            <button type="button" onClick={() => void cancelRun(run.id).then(onChanged).catch(problem => onError(errorText(problem)))} className={OUTLINE}><X size={13} />{t('trainingStop')}</button>
          ) : (
            <button type="button" onClick={onDelete} className={`${OUTLINE} hover:border-rose-400 hover:text-rose-600`} title={t('trainingDelete')}><Trash2 size={13} /></button>
          )}
        </div>
      </div>

      <div className="mt-3 flex flex-wrap gap-1">
        {run.stages.map((stage, index) => (
          <span
            key={stage}
            className={`rounded-md px-1.5 py-0.5 text-[10px] font-semibold ${
              index < stageIndex ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400' : index === stageIndex ? 'bg-pink-500/15 text-pink-600 dark:text-pink-300' : 'bg-zinc-200/60 text-zinc-500 dark:bg-white/5'
            }`}
          >
            {tt(`trainingStage_${stage}`)}
          </span>
        ))}
      </div>

      {last && (
        <>
          <div className="mt-3 flex items-baseline justify-between text-[11px] tabular-nums text-zinc-600 dark:text-zinc-300">
            <span>{t('trainingStep')} {last.step} · {t('trainingLoss')} {last.loss.toFixed(3)}{kl !== null ? ` · KL ${kl.toFixed(2)}${target > 0 ? ` / ${target}` : ''}` : ''}</span>
            {running && left > 0 && <span>{clock(left)} {t('trainingLeft')}</span>}
          </div>
          <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-zinc-200 dark:bg-white/10">
            <div className="h-full bg-gradient-to-r from-orange-500 to-pink-600 transition-[width]" style={{ width: `${percent}%` }} />
          </div>
          <LossLine steps={run.steps} />
        </>
      )}

      {run.checkpoints.length > 0 && (
        <div className="mt-3">
          <p className={LABEL}>{t('trainingCheckpoints')}</p>
          <div className="mt-1.5 flex flex-wrap gap-2">
            {[...run.checkpoints].sort((a, b) => a - b).map(step => {
              const installed = run.installed.includes(step);
              return (
                <button
                  key={step}
                  type="button"
                  disabled={installed}
                  onClick={() =>
                    void installCheckpoint(run.id, step)
                      .then(() => {
                        window.dispatchEvent(new CustomEvent('studio:adapters-changed'));
                        window.dispatchEvent(new CustomEvent('studio:toast', { detail: { message: t('trainingAdded'), type: 'success' } }));
                        onChanged();
                      })
                      .catch(problem => onError(errorText(problem)))
                  }
                  className={`${OUTLINE} ${installed ? 'text-emerald-600 dark:text-emerald-400' : ''}`}
                >
                  {installed ? <Check size={13} /> : <Plus size={13} />}
                  {t('trainingStep')} {step} · {installed ? t('trainingInLora') : t('trainingToLora')}
                </button>
              );
            })}
          </div>
        </div>
      )}

      {(run.continuations ?? []).length > 0 && (
        <p className="mt-2 text-[11px] text-zinc-500">
          {(run.continuations ?? []).map(({ from, to }) => tt('trainingContinued').replace('{from}', String(from)).replace('{to}', String(to))).join(' · ')}
        </p>
      )}

      {!running && <ContinueRun run={run} epochs={epochs} onChanged={onChanged} onError={onError} />}

      {run.error && <p role="alert" className="mt-3 text-xs text-rose-600 dark:text-rose-300">{run.error}</p>}
      {run.log && run.log.length > 0 && (
        <details className="mt-2">
          <summary className="cursor-pointer text-[11px] text-zinc-500">{t('trainingLog')}</summary>
          <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap rounded-lg bg-zinc-100 p-2 text-[10px] leading-4 text-zinc-600 dark:bg-black/30 dark:text-zinc-400">{run.log.join('\n')}</pre>
        </details>
      )}
    </section>
  );
};

type Step = 'songs' | 'train' | 'result';

export const TrainingPanel: React.FC = () => {
  const { t } = useStrings();
  const [state, setState] = useState<TrainingState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [openId, setOpenId] = useState<string | null>(null);
  const [step, setStep] = useState<Step>('songs');
  const [adding, setAdding] = useState<{ done: number; total: number } | null>(null);
  const [importing, setImporting] = useState(false);
  const [deleting, setDeleting] = useState<{ kind: 'dataset' | 'run'; id: string } | null>(null);
  const [guideOpen, setGuideOpen] = useState(false);

  // the service being away is its own message, gone as soon as it answers again
  const [offline, setOffline] = useState<string | null>(null);
  const refresh = useCallback(async () => {
    try {
      setState(await fetchTraining());
      setOffline(null);
    } catch (problem) {
      setOffline(errorText(problem));
    }
  }, []);

  const job = state?.prepare ?? null;
  const busy = Boolean(state?.active || (job && !job.finished) || adding || (state?.download && !state.download.done) || (state?.listen?.download && !state.listen.download.done));
  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), busy ? 1500 : 5000);
    return () => window.clearInterval(timer);
  }, [refresh, busy]);

  // a run that starts by itself after preparation is shown where it runs
  useEffect(() => {
    if (job?.run && job.dataset === openId) setStep('result');
  }, [job?.run, job?.dataset, openId]);

  const dataset = state?.datasets.find(entry => entry.id === openId) ?? null;
  const replace = (next: Dataset) => setState(current => (current ? { ...current, datasets: current.datasets.map(entry => (entry.id === next.id ? next : entry)) } : current));

  const addAndPrepare = async (id: string, files: PickedFile[]) => {
    setError(null);
    try {
      const next = await addPicked(id, files, (done, total) => setAdding({ done, total }));
      if (next) replace(next);
      setAdding(null);
      await prepareDataset(id, { lyrics: 'missing', style: 'missing' });
      await refresh();
    } catch (problem) {
      setAdding(null);
      setError(errorText(problem));
    }
  };

  const createFrom = async (files: PickedFile[]) => {
    try {
      const made = await createDataset(nameFromPicked(files) || t('trainingNewDataset'), '');
      await refresh();
      setOpenId(made.id);
      setStep('songs');
      await addAndPrepare(made.id, files);
    } catch (problem) {
      setError(errorText(problem));
    }
  };

  const importFolder = async (files: File[]) => {
    if (files.length === 0) return;
    setImporting(true);
    try {
      const made = await importDataset(files);
      await refresh();
      setOpenId(made.id);
      setStep('songs');
    } catch (problem) {
      setError(errorText(problem));
    } finally {
      setImporting(false);
    }
  };

  const prepare = async (request: PrepareRequest) => {
    if (!dataset) return;
    setError(null);
    try {
      await prepareDataset(dataset.id, request);
      await refresh();
    } catch (problem) {
      setError(errorText(problem));
    }
  };

  const trainAfter = async (on: boolean) => {
    if (!dataset || !state) return;
    try {
      await setTrainAfter(on ? { name: dataset.name, recipe: state.recipe_defaults } : null);
      await refresh();
    } catch (problem) {
      setError(errorText(problem));
    }
  };

  const confirmDelete = async () => {
    const target = deleting;
    setDeleting(null);
    if (!target) return;
    try {
      if (target.kind === 'dataset') {
        await deleteDataset(target.id);
        setOpenId(null);
      } else await deleteRun(target.id);
      await refresh();
    } catch (problem) {
      setError(errorText(problem));
    }
  };

  if (!state) return <p className="flex items-center gap-2 text-sm text-zinc-500"><Loader2 size={14} className="animate-spin" />{offline}</p>;
  const runs = dataset ? state.runs.filter(run => run.dataset_id === dataset.id) : [];
  // one step after another: training once there are songs, the result once there was a run
  const steps: { id: Step; label: string; open: boolean }[] = [
    { id: 'songs', label: t('trainingStepSongs'), open: true },
    { id: 'train', label: t('trainingStepTrain'), open: Boolean(dataset?.items.length) },
    { id: 'result', label: t('trainingStepResult'), open: runs.length > 0 },
  ];

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        {dataset ? (
          <>
            <button type="button" onClick={() => setOpenId(null)} className={`${OUTLINE} px-2`} title={t('trainingDatasets')}><ChevronLeft size={15} /></button>
            <p className="min-w-0 truncate text-base font-semibold text-zinc-900 dark:text-white">{dataset.name}</p>
            <div className="flex items-center gap-1">
              {steps.map((entry, index) => (
                <React.Fragment key={entry.id}>
                  {index > 0 && <span className="h-px w-4 bg-zinc-300 dark:bg-white/15" />}
                  <button
                    type="button"
                    disabled={!entry.open}
                    onClick={() => setStep(entry.id)}
                    className={`rounded-full px-3 py-1 text-xs font-semibold disabled:cursor-not-allowed disabled:opacity-40 ${step === entry.id ? 'bg-pink-500/15 text-pink-600 dark:text-pink-300' : 'text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200'}`}
                  >
                    {index + 1} · {entry.label}
                  </button>
                </React.Fragment>
              ))}
            </div>
            <div className="ml-auto flex items-center gap-2">
              <button type="button" onClick={() => setGuideOpen(value => !value)} className={OUTLINE} aria-pressed={guideOpen}><BookOpen size={13} />{t('trainingGuideOpen')}</button>
              <MoreMenu
                items={[
                  { label: t('trainingExport'), icon: <FolderOutput size={14} />, onClick: () => void revealDataset(dataset.id).catch(problem => setError(errorText(problem))) },
                  { label: t('trainingDeleteDataset'), icon: <Trash2 size={14} />, onClick: () => setDeleting({ kind: 'dataset', id: dataset.id }), danger: true, disabled: Boolean(job && !job.finished && job.dataset === dataset.id) },
                ]}
              />
            </div>
          </>
        ) : (
          <>
            <p className="min-w-0 flex-1 text-sm text-zinc-600 dark:text-zinc-400">{t('trainingIntro')}</p>
            <button type="button" onClick={() => setGuideOpen(value => !value)} className={OUTLINE} aria-pressed={guideOpen}><BookOpen size={13} />{t('trainingGuideOpen')}</button>
          </>
        )}
      </div>
      {guideOpen && <TrainingGuide onClose={() => setGuideOpen(false)} />}

      {!dataset && <DatasetList state={state} busy={Boolean(adding)} onOpen={id => { setOpenId(id); setStep(state.runs.some(run => run.dataset_id === id && run.status === 'running') ? 'result' : 'songs'); }} onNew={files => void createFrom(files)} onImport={files => void importFolder(files)} importing={importing} />}

      {dataset && step === 'songs' && (
        <SongsStep
          state={state}
          dataset={dataset}
          job={job}
          adding={adding}
          onAdd={files => void addAndPrepare(dataset.id, files)}
          onAddLibrary={ids => void addLibrarySongs(dataset.id, ids).then(next => { replace(next); return prepareDataset(dataset.id, { lyrics: 'missing', style: 'missing' }); }).then(refresh).catch(problem => setError(errorText(problem)))}
          onPrepare={request => void prepare(request)}
          onNext={() => setStep('train')}
          onTrainAfter={on => void trainAfter(on)}
          onChanged={replace}
          onRefresh={() => void refresh()}
          onError={setError}
        />
      )}
      {dataset && step === 'train' && <TrainStep state={state} dataset={dataset} job={job} onBack={() => setStep('songs')} onStarted={() => { setStep('result'); void refresh(); }} onChanged={replace} onRefresh={() => void refresh()} onError={setError} />}
      {dataset && step === 'result' && (
        <div className="space-y-3">
          {runs.length === 0 && <p className="text-sm text-zinc-500">{t('trainingNoRuns')}</p>}
          {runs.map(run => <RunCard key={run.id} run={run} epochs={state?.progress_unit === 'epoch'} onChanged={() => void refresh()} onError={setError} onDelete={() => setDeleting({ kind: 'run', id: run.id })} />)}
        </div>
      )}

      {offline && <p role="alert" className="rounded-lg bg-rose-500/10 px-3 py-2 text-sm text-rose-700 dark:text-rose-300">{offline}</p>}
      {error && (
        <div role="alert" className="flex items-start gap-3 rounded-lg bg-rose-500/10 px-3 py-2 text-sm text-rose-700 dark:text-rose-300">
          <p className="min-w-0 flex-1">{error}</p>
          <button type="button" onClick={() => setError(null)} className="shrink-0" aria-label={t('adaptersCancel')}><X size={14} /></button>
        </div>
      )}

      <ConfirmDialog
        isOpen={deleting !== null}
        title={t('trainingDelete')}
        message={deleting?.kind === 'dataset' ? t('trainingDeleteDatasetMessage') : t('trainingDeleteRunMessage')}
        confirmLabel={t('trainingDelete')}
        onConfirm={() => void confirmDelete()}
        onCancel={() => setDeleting(null)}
      />
    </div>
  );
};

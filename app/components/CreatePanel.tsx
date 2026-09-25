import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { karaokeReason } from '../services/karaoke';
import { AlertTriangle, ChevronDown, CircleAlert, Dices, Ear, FolderOpen, Loader2, Pause, Play, RotateCcw, Save, Sparkles, Square, Tags, Undo2, Upload, Wand2, Settings2, Lightbulb, ListChecks } from 'lucide-react';
import type { AceCreateRequest, Song } from '../types';
import { useI18n } from '../context/I18nContext';
import { useBridgeCommand } from '../services/mcpBridge';
import { AdapterPicker } from './AdapterPicker';
import type { AdapterUse } from '../services/adapters';
import { randomExample, randomIdea, someGenres } from '../services/examples';
import { apiUrl } from '../services/apiBase';
import { SCHEDULERS, SOLVERS } from '../services/aceEngine';

/**
 * The ACE-Step 1.5 request form.
 *
 * The fields are acestep.cpp's own request fields, grouped the way the model
 * works: a task (make a song, cover one, repaint a part, add or take out an
 * instrument), the song (caption, lyrics, metadata), the language model that
 * plans it, the DiT that renders it, the output. An empty field means the
 * engine's default, shown as the placeholder; the studio fills in what it
 * knows better for the chosen model (steps, guidance, shift, sampler).
 */

interface CreatePanelProps {
  onGenerate: (request: AceCreateRequest & { _tempId?: string }) => void;
  isGenerating: boolean;
  activeJobCount?: number;
  initialData?: { song: Song; timestamp: number } | null;
}

type ProfileFiles = { synth_model: string; lm_model: string; text_encoder: string; vae: string };

type SetupStatus = {
  ready?: boolean;
  profile_files?: ProfileFiles | null;
  engine_ready?: boolean;
  selected_profile_id?: string | null;
  selected_component_ids?: string[] | null;
  hardware?: { reason?: string };
};

type EngineCatalog = {
  default?: Record<string, unknown>;
  models?: { lm?: string[]; dit?: string[]; vae?: string[]; embedding?: string[] };
};

type LibrarySong = { id: string; title: string };

type Task = 'text2music' | 'cover' | 'cover-nofsq' | 'repaint' | 'lego' | 'extract' | 'complete';
const TASKS: Task[] = ['text2music', 'cover', 'cover-nofsq', 'repaint', 'lego', 'extract', 'complete'];
const needsSource = (task: Task) => task !== 'text2music';
const baseOnly = (task: Task) => task === 'lego' || task === 'extract' || task === 'complete';

const TRACKS = ['vocals', 'backing_vocals', 'drums', 'bass', 'guitar', 'keyboard', 'percussion', 'strings', 'synth', 'fx', 'brass', 'woodwinds'];
const GUIDANCE = ['apg', 'adg', 'cfg_pp', 'dynamic_cfg', 'rescaled_cfg', 'cfg_zero_star', 'smc_cfg', 'cfg_mp'];
const LANGUAGES = ['', 'en', 'ru', 'zh', 'ja', 'ko', 'es', 'fr', 'de', 'it', 'pt', 'ar', 'hi', 'tr', 'pl', 'uk', 'nl', 'sv', 'vi', 'th', 'id', 'unknown'];
const KEYS = ['', ...['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'].flatMap(note => [`${note} major`, `${note} minor`])];
const TIME_SIGNATURES = ['', '2', '3', '4', '6'];
const GROUPS = ['self_attn', 'cross_attn', 'mlp', 'cond_embed', 'time_embed', 'proj_in'] as const;
const MAX_DURATION_SECONDS = 600;
const MAX_TAKES = 9;

/** What the chosen DiT is, which decides its defaults. */
type DitKind = 'turbo' | 'sft' | 'base';
const ditKind = (file: string | undefined): DitKind => {
  const name = (file ?? '').toLowerCase();
  if (name.includes('turbo') && !name.includes('sftturbo') && !name.includes('base-turbo')) return 'turbo';
  if (name.includes('base')) return 'base';
  if (name.includes('turbo')) return 'turbo';
  return 'sft';
};
/**
 * The studio's defaults per model. Turbo keeps the authors' 8 steps and shift
 * 3 with the Heun solver and the linear-quadratic schedule, which the ACE-Step
 * community reports as the cleaner turbo render (ace-step/ACE-Step-1.5#956);
 * SFT and base are guided models, rendered with 50 steps at CFG 7 as the
 * official interface does.
 */
const DIT_DEFAULTS: Record<DitKind, { steps: number; guidance: number; shift: number; solver: string; scheduler: string }> = {
  turbo: { steps: 8, guidance: 1, shift: 3, solver: 'heun', scheduler: 'linear_quadratic' },
  sft: { steps: 50, guidance: 7, shift: 1, solver: 'euler', scheduler: 'linear' },
  base: { steps: 50, guidance: 7, shift: 1, solver: 'euler', scheduler: 'linear' },
};

const CONTROL =
  'w-full rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2 text-sm text-zinc-900 outline-none transition-colors focus:border-pink-500 disabled:opacity-50 dark:border-white/10 dark:bg-black/25 dark:text-white';
const LABEL = 'mb-1.5 block text-[11px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400';
const ICON =
  'rounded-md p-1.5 text-zinc-400 transition-colors hover:bg-zinc-200 hover:text-black dark:hover:bg-white/10 dark:hover:text-white disabled:opacity-40';

const numberOrUndefined = (value: string): number | undefined => {
  const trimmed = value.trim();
  if (trimmed === '') return undefined;
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) ? parsed : undefined;
};

const Field: React.FC<{ label: string; hint?: string; children: React.ReactNode }> = ({ label, hint, children }) => (
  <label className="block">
    <span className={LABEL}>{label}</span>
    {children}
    {hint && <span className="mt-1 block text-[11px] leading-4 text-zinc-500">{hint}</span>}
  </label>
);

const Switch: React.FC<{ checked: boolean; onChange: (value: boolean) => void; label: string; hint?: string }> = ({ checked, onChange, label, hint }) => (
  <div className="flex items-center justify-between gap-3">
    <div className="min-w-0">
      <span className="text-[11px] font-bold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{label}</span>
      {hint && <p className="mt-0.5 text-[11px] leading-4 text-zinc-500">{hint}</p>}
    </div>
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className={`relative h-5 w-10 shrink-0 rounded-full transition-colors ${checked ? 'bg-pink-500' : 'bg-zinc-300 dark:bg-zinc-600'}`}
    >
      <span className={`absolute top-[2px] h-4 w-4 rounded-full bg-white shadow-sm transition-all ${checked ? 'left-[22px]' : 'left-[2px]'}`} />
    </button>
  </div>
);

/** A number you drag. Empty means the default, which the slider shows until touched. */
const SliderRow: React.FC<{
  label: string;
  value: string;
  fallback: number;
  min: number;
  max: number;
  step: number;
  suffix?: string;
  onChange: (value: string) => void;
  disabled?: boolean;
}> = ({ label, value, fallback, min, max, step, suffix, onChange, disabled }) => {
  const current = value.trim() === '' ? fallback : Number(value);
  const shown = Number.isFinite(current) ? current : fallback;
  const decimals = step < 1 ? String(step).split('.')[1]?.length ?? 1 : 0;
  return (
    <div className={disabled ? 'opacity-50' : undefined}>
      <div className="flex items-baseline justify-between gap-2">
        <span className="text-[11px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{label}</span>
        <span className="text-[11px] tabular-nums text-zinc-600 dark:text-zinc-300">
          {shown.toFixed(decimals)}{suffix ?? ''}
          {value.trim() === '' && <span className="ml-1 text-zinc-400">·</span>}
        </span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={shown}
        disabled={disabled}
        onChange={event => onChange(event.target.value)}
        className="mt-1.5 h-1 w-full cursor-pointer accent-pink-500"
      />
    </div>
  );
};

const Stage: React.FC<{ title: string; hint: string; children: React.ReactNode }> = ({ title, hint, children }) => (
  <section>
    <h4 className="text-[11px] font-bold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{title}</h4>
    <p className="mb-3 mt-0.5 text-[11px] leading-4 text-zinc-500">{hint}</p>
    {children}
  </section>
);

const Card: React.FC<{ title: string; actions?: React.ReactNode; children: React.ReactNode }> = ({ title, actions, children }) => (
  <div className="overflow-hidden rounded-xl border border-zinc-200 bg-white dark:border-white/5 dark:bg-suno-card">
    <div className="flex items-center justify-between gap-2 border-b border-zinc-100 bg-zinc-50 px-3 py-2 dark:border-white/5 dark:bg-white/5">
      <span className="text-[11px] font-bold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{title}</span>
      {actions && <div className="flex items-center gap-1">{actions}</div>}
    </div>
    <div className="p-3">{children}</div>
  </div>
);

const AutoTextarea: React.FC<React.TextareaHTMLAttributes<HTMLTextAreaElement> & { minRows?: number }> = ({ minRows = 3, value, ...rest }) => {
  const node = useRef<HTMLTextAreaElement | null>(null);
  useEffect(() => {
    const element = node.current;
    if (!element) return;
    element.style.height = 'auto';
    element.style.height = `${Math.max(element.scrollHeight, minRows * 20)}px`;
  }, [value, minRows]);
  return <textarea ref={node} value={value} rows={minRows} {...rest} />;
};

/** The adapters a stored request used, as the picker holds them. */
const usesFromRequest = (settings: Record<string, unknown>): AdapterUse[] => {
  const uses: AdapterUse[] = [];
  if (Array.isArray(settings.adapters)) {
    for (const item of settings.adapters) {
      const entry = item as Record<string, unknown>;
      const id = typeof entry?.name === 'string' ? entry.name : typeof entry?.id === 'string' ? entry.id : '';
      if (!id) continue;
      const scale = typeof entry.scale === 'number' ? entry.scale : 1;
      uses.push({ id, scales: entry.scales && typeof entry.scales === 'object' ? (entry.scales as Record<string, number>) : { dit: scale } });
    }
  }
  if (typeof settings.lm_adapter === 'string' && settings.lm_adapter) {
    uses.push({ id: settings.lm_adapter, scales: { lm: typeof settings.lm_adapter_scale === 'number' ? settings.lm_adapter_scale : 1 } });
  }
  return uses;
};

export const CreatePanel: React.FC<CreatePanelProps> = ({ onGenerate, isGenerating, activeJobCount = 0, initialData }) => {
  const { t } = useI18n();
  const tt = t as unknown as (key: string) => string;

  const [name, setName] = useState('');
  const [caption, setCaption] = useState('');
  const [lyrics, setLyrics] = useState('');
  const [instrumental, setInstrumental] = useState(false);
  const [language, setLanguage] = useState('');
  const [gender, setGender] = useState<'' | 'male' | 'female'>('');
  const [genres, setGenres] = useState<string[]>(() => someGenres());
  // What the language model or the assistant replaced, for the undo button.
  const [undo, setUndo] = useState<{ caption: string; lyrics: string } | null>(null);
  const [bpm, setBpm] = useState('');
  const [keyscale, setKeyscale] = useState('');
  const [timesignature, setTimesignature] = useState('');
  const [duration, setDuration] = useState('');

  const [task, setTask] = useState<Task>('text2music');
  const [sourceSong, setSourceSong] = useState('');
  const [referenceSong, setReferenceSong] = useState('');
  const [tracks, setTracks] = useState<string[]>([]);
  const [coverStrength, setCoverStrength] = useState('');
  const [coverNoise, setCoverNoise] = useState('');
  const [repaintStart, setRepaintStart] = useState('');
  const [repaintEnd, setRepaintEnd] = useState('');

  const [think, setThink] = useState(true);
  const [cotCaption, setCotCaption] = useState(true);
  const [lmTemperature, setLmTemperature] = useState('');
  const [lmCfg, setLmCfg] = useState('');
  const [lmTopP, setLmTopP] = useState('');
  const [lmTopK, setLmTopK] = useState('');
  const [lmNegative, setLmNegative] = useState('');
  const [lmSeed, setLmSeed] = useState('');
  const [songs, setSongs] = useState('');
  const [audioCodes, setAudioCodes] = useState('');

  const [steps, setSteps] = useState('');
  const [guidance, setGuidance] = useState('');
  const [shift, setShift] = useState('');
  const [solver, setSolver] = useState('');
  const [scheduler, setScheduler] = useState('');
  const [guidanceMode, setGuidanceMode] = useState('');
  const [apgMomentum, setApgMomentum] = useState('');
  const [apgNorm, setApgNorm] = useState('');
  const [dcwMode, setDcwMode] = useState('');
  const [dcwScaler, setDcwScaler] = useState('');
  const [dcwHigh, setDcwHigh] = useState('');
  const [customTimesteps, setCustomTimesteps] = useState('');
  const [latentShift, setLatentShift] = useState('');
  const [latentRescale, setLatentRescale] = useState('');
  const [cfgStart, setCfgStart] = useState('');
  const [cfgEnd, setCfgEnd] = useState('');
  const [retakeVariance, setRetakeVariance] = useState('');
  const [retakeSeed, setRetakeSeed] = useState('');
  const [fadeIn, setFadeIn] = useState('');
  const [fadeOut, setFadeOut] = useState('');
  const [bulk, setBulk] = useState('');
  const [uploading, setUploading] = useState<'source' | 'reference' | null>(null);
  const [listening, setListening] = useState<'codes' | 'describe' | null>(null);
  const [playing, setPlaying] = useState<string | null>(null);
  const preview = useRef<HTMLAudioElement | null>(null);

  const [takes, setTakes] = useState('');
  const [randomizeSeed, setRandomizeSeed] = useState(true);
  const [seed, setSeed] = useState('');
  const [peakClip, setPeakClip] = useState('');
  const [mp3Bitrate, setMp3Bitrate] = useState('320');
  const [format, setFormat] = useState<AceCreateRequest['output_format']>('mp3');
  const [models, setModels] = useState<Partial<Record<'synth_model' | 'lm_model' | 'vae', string>>>({});
  const [adapters, setAdapters] = useState<AdapterUse[]>([]);
  const [groups, setGroups] = useState<Record<string, string>>({});

  const applyTrigger = useCallback((word: string, present: boolean) => {
    setCaption(current => {
      const parts = current.split(',').map(part => part.trim());
      const has = parts.some(part => part.toLowerCase() === word.toLowerCase());
      if (present) return has ? current : current.trim() ? `${word}, ${current.trim()}` : `${word}, `;
      return has ? parts.filter(part => part.toLowerCase() !== word.toLowerCase()).join(', ') : current;
    });
  }, []);

  const [setup, setSetup] = useState<SetupStatus | null>(null);
  const [serviceDown, setServiceDown] = useState(false);
  const [catalog, setCatalog] = useState<EngineCatalog | null>(null);
  const [library, setLibrary] = useState<LibrarySong[]>([]);
  const [assistantReady, setAssistantReady] = useState(false);
  const [assisting, setAssisting] = useState<'all' | 'lyrics' | 'prompt' | 'sections' | null>(null);
  const [planning, setPlanning] = useState<'inspire' | 'format' | null>(null);
  const [assistStage, setAssistStage] = useState<string | null>(null);
  const [assistModel, setAssistModel] = useState<string | null>(null);
  const [assistDraft, setAssistDraft] = useState('');
  const [coverPrompt, setCoverPrompt] = useState('');
  const [activity, setActivity] = useState<Array<{ song_id: string; title: string; kind: string; state: string; detail?: string }>>([]);
  const [assistSeconds, setAssistSeconds] = useState(0);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [mode, setMode] = useState<'simple' | 'studio'>('studio');
  const [idea, setIdea] = useState('');
  const [error, setError] = useState<string | null>(null);
  const promptFile = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    let finished = '';
    const read = () => void fetch('/v1/activity')
      .then(response => response.json())
      .then((body: { activity?: typeof activity }) => {
        const entries = body.activity ?? [];
        const done = entries.filter(entry => entry.state === 'done').map(entry => `${entry.song_id}:${entry.kind}`).join(',');
        if (done !== finished) {
          finished = done;
          window.dispatchEvent(new CustomEvent('studio:library-changed'));
        }
        setActivity(entries);
      })
      .catch(() => undefined);
    read();
    const timer = window.setInterval(read, 2000);
    return () => window.clearInterval(timer);
  }, []);

  const ready = setup?.ready === true && setup?.engine_ready === true;
  const synthModel = models.synth_model || setup?.profile_files?.synth_model;
  const kind = ditKind(synthModel);
  const dit = DIT_DEFAULTS[kind];
  const turbo = kind === 'turbo';

  useEffect(() => {
    if (!assisting && !planning) return;
    setAssistSeconds(0);
    const started = Date.now();
    const timer = window.setInterval(() => setAssistSeconds(Math.round((Date.now() - started) / 1000)), 1000);
    return () => window.clearInterval(timer);
  }, [assisting, planning]);

  const refreshSetup = useCallback(async () => {
    const response = await fetch('/setup/status');
    if (!response.ok) throw new Error(String(response.status));
    setSetup(await response.json());
    setServiceDown(false);
  }, []);

  // The ready-made sets by name, so the profile line reads as the model manager does.
  const [profileNames, setProfileNames] = useState<Record<string, string>>({});
  useEffect(() => {
    void fetch('/setup/catalog')
      .then(response => (response.ok ? response.json() : Promise.reject(new Error(String(response.status)))))
      .then((body: { profiles?: Array<{ id: string; label: string }> }) => setProfileNames(Object.fromEntries((body.profiles ?? []).map(profile => [profile.id, profile.label]))))
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    const poll = () => void refreshSetup().catch(() => { setSetup(null); setServiceDown(true); });
    poll();
    const timer = window.setInterval(poll, 5000);
    return () => window.clearInterval(timer);
  }, [refreshSetup]);

  useEffect(() => {
    void fetch('/v1/local-models/music')
      .then(response => (response.ok ? response.json() : Promise.reject(new Error())))
      .then((body: { catalog?: EngineCatalog }) => setCatalog(body.catalog ?? null))
      .catch(() => setCatalog(null));
  }, [setup?.engine_ready]);

  useEffect(() => {
    const read = () => void fetch('/v1/library/songs')
      .then(response => (response.ok ? response.json() : Promise.reject(new Error())))
      .then((body: LibrarySong[]) => setLibrary(Array.isArray(body) ? body.map(song => ({ id: song.id, title: song.title })) : []))
      .catch(() => undefined);
    read();
    window.addEventListener('studio:library-changed', read);
    return () => window.removeEventListener('studio:library-changed', read);
  }, []);

  useEffect(() => {
    const read = () => void fetch('/v1/assistant/status')
      .then(response => (response.ok ? response.json() : Promise.reject(new Error())))
      .then((body: { available?: boolean }) => setAssistantReady(body.available === true))
      .catch(() => setAssistantReady(false));
    read();
    const timer = window.setInterval(read, 5000);
    window.addEventListener('studio:settings-changed', read);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener('studio:settings-changed', read);
    };
  }, []);

  const remember = () => setUndo({ caption, lyrics });

  /** Fills the form from an engine request: a stored track's, a file's, the model's plan. */
  const applyRequest = useCallback((settings: Record<string, unknown>, options: { keepSource?: boolean } = {}) => {
    const text = (key: string) => (settings[key] === undefined || settings[key] === null ? '' : String(settings[key]));
    if (typeof settings.title === 'string') setName(settings.title);
    if (typeof settings.caption === 'string') setCaption(settings.caption);
    if (typeof settings.lyrics === 'string') {
      const words = settings.lyrics.trim();
      setInstrumental(words === '[Instrumental]');
      setLyrics(words === '[Instrumental]' ? '' : settings.lyrics);
    }
    const numberText = (key: string) => (typeof settings[key] === 'number' && (settings[key] as number) > 0 ? String(settings[key]) : '');
    if ('bpm' in settings) setBpm(numberText('bpm'));
    if ('duration' in settings) setDuration(numberText('duration'));
    if ('keyscale' in settings) setKeyscale(text('keyscale'));
    if ('timesignature' in settings) setTimesignature(text('timesignature'));
    if ('vocal_language' in settings) setLanguage(text('vocal_language'));
    if (!options.keepSource) {
      if (typeof settings.task_type === 'string' && TASKS.includes(settings.task_type as Task)) setTask(settings.task_type as Task);
      if (typeof settings.track === 'string') setTracks(settings.track ? settings.track.split('|').map(part => part.trim().toLowerCase()).filter(Boolean) : []);
      setSourceSong(typeof settings.source_song_id === 'string' ? settings.source_song_id : '');
      setReferenceSong(typeof settings.reference_song_id === 'string' ? settings.reference_song_id : '');
      for (const [key, set] of [
        ['inference_steps', setSteps], ['guidance_scale', setGuidance], ['shift', setShift], ['solver', setSolver], ['scheduler', setScheduler],
        ['guidance', setGuidanceMode], ['apg_momentum', setApgMomentum], ['apg_norm_threshold', setApgNorm], ['dcw_mode', setDcwMode],
        ['dcw_scaler', setDcwScaler], ['dcw_high_scaler', setDcwHigh], ['custom_timesteps', setCustomTimesteps], ['latent_shift', setLatentShift],
        ['latent_rescale', setLatentRescale], ['lm_temperature', setLmTemperature], ['lm_cfg_scale', setLmCfg], ['lm_top_p', setLmTopP],
        ['lm_top_k', setLmTopK], ['lm_negative_prompt', setLmNegative], ['audio_cover_strength', setCoverStrength],
        ['cover_noise_strength', setCoverNoise], ['repainting_start', setRepaintStart], ['repainting_end', setRepaintEnd],
        ['peak_clip', setPeakClip], ['mp3_bitrate', setMp3Bitrate], ['cfg_interval_start', setCfgStart], ['cfg_interval_end', setCfgEnd],
        ['retake_variance', setRetakeVariance], ['retake_seed', setRetakeSeed], ['fade_in', setFadeIn], ['fade_out', setFadeOut],
      ] as const) {
        if (key in settings) (set as (value: string) => void)(text(key));
      }
      if (typeof settings.output_format === 'string') setFormat(settings.output_format as AceCreateRequest['output_format']);
      if (typeof settings.use_cot_caption === 'boolean') setCotCaption(settings.use_cot_caption);
      setAdapters(usesFromRequest(settings));
      const groupScales = settings.adapter_group_scales as Record<string, unknown> | undefined;
      setGroups(groupScales && typeof groupScales === 'object' ? Object.fromEntries(Object.entries(groupScales).map(([key, value]) => [key, String(value)])) : {});
    }
    setError(null);
  }, []);

  useEffect(() => {
    if (!initialData?.song) return;
    const song = initialData.song;
    const settings = { ...(song.generationParams ?? {}) } as Record<string, unknown>;
    settings.title = song.title || '';
    if (typeof settings.caption !== 'string') settings.caption = song.style || '';
    if (typeof settings.lyrics !== 'string') settings.lyrics = song.lyrics || '';
    applyRequest(settings);
  }, [initialData, applyRequest]);

  const reset = () => {
    setName(''); setCaption(''); setLyrics(''); setInstrumental(false); setLanguage('');
    setBpm(''); setKeyscale(''); setTimesignature(''); setDuration(''); setAudioCodes('');
    setTask('text2music'); setSourceSong(''); setReferenceSong(''); setTracks([]);
    setAdapters([]); setGroups({}); setCoverPrompt(''); setError(null);
  };

  const resetParameters = () => {
    setSteps(''); setGuidance(''); setShift(''); setSolver(''); setScheduler(''); setGuidanceMode('');
    setApgMomentum(''); setApgNorm(''); setDcwMode(''); setDcwScaler(''); setDcwHigh(''); setCustomTimesteps('');
    setLatentShift(''); setLatentRescale(''); setLmTemperature(''); setLmCfg(''); setLmTopP(''); setLmTopK('');
    setLmNegative(''); setLmSeed(''); setSongs(''); setTakes(''); setSeed(''); setRandomizeSeed(true);
    setCoverStrength(''); setCoverNoise(''); setRepaintStart(''); setRepaintEnd('');
    setPeakClip(''); setMp3Bitrate('320'); setFormat('mp3'); setModels({});
    setCfgStart(''); setCfgEnd(''); setRetakeVariance(''); setRetakeSeed(''); setFadeIn(''); setFadeOut(''); setBulk('');
  };

  const buildRequest = (): AceCreateRequest => {
    const genderLine = gender === 'male' ? 'Male vocals' : gender === 'female' ? 'Female vocals' : '';
    const request: AceCreateRequest = {
      caption: genderLine && !instrumental && !caption.includes(genderLine) ? `${caption.trim()}${caption.trim() ? '\n' : ''}${genderLine}` : caption.trim(),
      lyrics: instrumental ? '[Instrumental]' : lyrics.replace(/\r\n?/g, '\n').trim(),
      task_type: task,
      think,
      use_cot_caption: cotCaption,
      inference_steps: numberOrUndefined(steps) ?? dit.steps,
      shift: numberOrUndefined(shift) ?? dit.shift,
      solver: solver || dit.solver,
      scheduler: scheduler || dit.scheduler,
      output_format: format,
      mp3_bitrate: numberOrUndefined(mp3Bitrate) ?? 320,
    };
    if (!turbo) request.guidance_scale = numberOrUndefined(guidance) ?? dit.guidance;
    const put = (key: keyof AceCreateRequest, value: unknown) => {
      if (value !== undefined && value !== '') (request as unknown as Record<string, unknown>)[key] = value;
    };
    put('vocal_language', language);
    put('bpm', numberOrUndefined(bpm));
    put('keyscale', keyscale);
    put('timesignature', timesignature);
    const seconds = numberOrUndefined(duration);
    put('duration', seconds === undefined ? undefined : Math.min(seconds, MAX_DURATION_SECONDS));
    put('seed', randomizeSeed ? undefined : numberOrUndefined(seed));
    put('lm_seed', numberOrUndefined(lmSeed));
    put('lm_batch_size', numberOrUndefined(songs));
    put('synth_batch_size', numberOrUndefined(takes));
    put('lm_temperature', numberOrUndefined(lmTemperature));
    put('lm_cfg_scale', numberOrUndefined(lmCfg));
    put('lm_top_p', numberOrUndefined(lmTopP));
    put('lm_top_k', numberOrUndefined(lmTopK));
    put('lm_negative_prompt', lmNegative.trim());
    put('audio_codes', audioCodes.trim());
    put('guidance', guidanceMode);
    put('apg_momentum', numberOrUndefined(apgMomentum));
    put('apg_norm_threshold', numberOrUndefined(apgNorm));
    put('dcw_mode', dcwMode);
    put('dcw_scaler', numberOrUndefined(dcwScaler));
    put('dcw_high_scaler', numberOrUndefined(dcwHigh));
    put('custom_timesteps', customTimesteps.trim());
    put('latent_shift', numberOrUndefined(latentShift));
    put('latent_rescale', numberOrUndefined(latentRescale));
    put('peak_clip', numberOrUndefined(peakClip));
    put('cfg_interval_start', numberOrUndefined(cfgStart));
    put('cfg_interval_end', numberOrUndefined(cfgEnd));
    const variance = numberOrUndefined(retakeVariance);
    if (variance && variance > 0) {
      put('retake_variance', variance);
      put('retake_seed', numberOrUndefined(retakeSeed));
    }
    put('fade_in', numberOrUndefined(fadeIn));
    put('fade_out', numberOrUndefined(fadeOut));
    if (task === 'cover' || task === 'cover-nofsq') {
      put('audio_cover_strength', numberOrUndefined(coverStrength));
      put('cover_noise_strength', numberOrUndefined(coverNoise));
    }
    if (task === 'repaint' || task === 'lego') {
      put('repainting_start', numberOrUndefined(repaintStart));
      put('repainting_end', numberOrUndefined(repaintEnd));
    }
    if (baseOnly(task)) put('track', tracks.map(track => (task === 'complete' ? track.toUpperCase() : track)).join(' | '));
    if (needsSource(task) && sourceSong) request.source_song_id = sourceSong;
    if (referenceSong) request.reference_song_id = referenceSong;
    for (const key of ['synth_model', 'lm_model', 'vae'] as const) put(key, models[key]);
    if (adapters.length) request.adapters = adapters;
    const scales = Object.fromEntries(Object.entries(groups).flatMap(([key, value]: [string, string]) => {
      const number = numberOrUndefined(value);
      return number === undefined || number === 1 ? [] : [[key, number]];
    }));
    if (Object.keys(scales).length) request.adapter_group_scales = scales;
    if (name.trim()) request.title = name.trim();
    if (coverPrompt.trim()) request.cover_prompt = coverPrompt.trim();
    return request;
  };

  const savePrompt = () => {
    const blob = new Blob([JSON.stringify(buildRequest(), null, 2)], { type: 'application/json' });
    const link = document.createElement('a');
    link.href = URL.createObjectURL(blob);
    link.download = `${(name.trim() || 'request').replace(/[\\/:*?"<>|]/g, '')}.json`;
    link.click();
    URL.revokeObjectURL(link.href);
  };

  const openPrompt = async (file: File) => {
    try {
      applyRequest(JSON.parse(await file.text()) as Record<string, unknown>);
    } catch {
      setError(t('promptFileInvalid'));
    }
  };

  /**
   * The engine's own language model: inspire turns a one-line idea into a
   * caption, lyrics and metadata; format completes the metadata of what is
   * written and tidies it. Both run on the card, nothing leaves the machine.
   */
  const plan = async (planMode: 'inspire' | 'format') => {
    if (planning || !ready) return;
    remember();
    setPlanning(planMode);
    setError(null);
    try {
      const request: Record<string, unknown> = planMode === 'inspire'
        ? { caption: idea.trim(), lyrics: instrumental ? '[Instrumental]' : '', vocal_language: language }
        : { caption: caption.trim(), lyrics: instrumental ? '[Instrumental]' : lyrics.trim(), vocal_language: language, bpm: numberOrUndefined(bpm) ?? 0, keyscale, timesignature, duration: numberOrUndefined(duration) ?? 0 };
      if (models.lm_model) request.lm_model = models.lm_model;
      const response = await fetch('/v1/ace/plan', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ mode: planMode, request }),
      });
      const body = await response.json().catch(() => null);
      if (!response.ok || !body?.plan) throw new Error(body?.error || String(response.status));
      applyRequest(body.plan as Record<string, unknown>, { keepSource: true });
      if (planMode === 'inspire' && !name.trim() && idea.trim()) setName(idea.trim().slice(0, 60));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setPlanning(null);
    }
  };

  const assistRun = useRef<AbortController | null>(null);
  const stopAssistant = () => {
    assistRun.current?.abort();
    assistRun.current = null;
    setAssisting(null);
    setAssistStage(null);
    setAssistDraft('');
  };
  /** The optional writing assistant (a local model or OpenRouter) writes caption and lyrics. */
  const askAssistant = async (target: 'all' | 'lyrics' | 'prompt') => {
    if (!assistantReady || assisting) return;
    remember();
    const run = new AbortController();
    assistRun.current = run;
    setAssisting(target);
    setError(null);
    try {
      setAssistStage('preparing');
      setAssistDraft('');
      let streamed = '';
      const live = await fetch('/v1/assistant/write/stream', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          target,
          description: name.trim(),
          instruction: idea.trim(),
          lyrics: lyrics.trim(),
          caption: caption.trim(),
          duration_seconds: numberOrUndefined(duration) ?? 120,
          instrumental,
        }),
        signal: run.signal,
      });
      if (!live.ok || !live.body) {
        const refused = await live.json().catch(() => null);
        throw new Error(refused?.error || String(live.status));
      }
      let body: Record<string, unknown> | null = null;
      const reader = live.body.getReader();
      const decoder = new TextDecoder();
      let carry = '';
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        carry += decoder.decode(value, { stream: true });
        let split = carry.indexOf('\n\n');
        while (split !== -1) {
          const frame = carry.slice(0, split).trim();
          carry = carry.slice(split + 2);
          split = carry.indexOf('\n\n');
          if (!frame.startsWith('data:')) continue;
          let event: { stage?: string; delta?: string; error?: string; model?: string; draft?: Record<string, unknown> };
          try {
            event = JSON.parse(frame.slice(5).trim());
          } catch {
            continue;
          }
          if (event.error) throw new Error(event.error);
          if (event.stage) setAssistStage(event.stage);
          if (event.draft) body = event.draft;
          if (event.model) setAssistModel(event.model);
          if (event.delta) {
            streamed += event.delta;
            setAssistDraft(streamed);
          }
        }
      }
      if (!body) throw new Error(t('assistantNoAnswer'));
      if (typeof body.lyrics === 'string') setLyrics(body.lyrics);
      if (typeof body.caption === 'string') setCaption(body.caption);
      if (typeof body.title === 'string' && body.title.trim()) setName(body.title.trim());
      if (typeof body.cover_prompt === 'string' && body.cover_prompt.trim()) setCoverPrompt(body.cover_prompt.trim());
      if (typeof body.duration_seconds === 'number' && body.duration_seconds >= 10) setDuration(String(Math.min(MAX_DURATION_SECONDS, Math.round(body.duration_seconds))));
    } catch (reason) {
      const cancelled = reason instanceof DOMException && reason.name === 'AbortError';
      if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      assistRun.current = null;
      setAssisting(null);
      setAssistStage(null);
      setAssistDraft('');
    }
  };

  const layOutLyrics = async () => {
    if (!assistantReady || assisting || !lyrics.trim()) return;
    const run = new AbortController();
    assistRun.current = run;
    setAssisting('sections');
    setError(null);
    try {
      const response = await fetch('/v1/assistant/sections', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ lyrics }),
        signal: run.signal,
      });
      const body = await response.json().catch(() => null);
      if (!response.ok || typeof body?.lyrics !== 'string') throw new Error(body?.error || String(response.status));
      setLyrics(body.lyrics);
    } catch (reason) {
      const cancelled = reason instanceof DOMException && reason.name === 'AbortError';
      if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      assistRun.current = null;
      setAssisting(null);
    }
  };

  /** Puts a file of the user's into the library and picks it as the source or the reference. */
  const upload = async (target: 'source' | 'reference', file: File) => {
    setUploading(target);
    setError(null);
    try {
      const form = new FormData();
      form.append('audio', file);
      const response = await fetch('/v1/library/import', { method: 'POST', body: form });
      const body = await response.json().catch(() => null);
      if (!response.ok || !body?.id) throw new Error(body?.error || String(response.status));
      setLibrary(current => [{ id: body.id, title: body.title }, ...current.filter(song => song.id !== body.id)]);
      if (target === 'source') setSourceSong(body.id);
      else setReferenceSong(body.id);
      window.dispatchEvent(new CustomEvent('studio:library-changed'));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setUploading(null);
    }
  };

  /** The engine listens to the source track: its codes, or its caption, lyrics and metadata. */
  const listen = async (what: 'codes' | 'describe') => {
    if (!sourceSong || listening || !ready) return;
    setListening(what);
    setError(null);
    try {
      const response = await fetch('/v1/ace/understand', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ song_id: sourceSong }),
      });
      const body = await response.json().catch(() => null);
      if (!response.ok || !body?.request) throw new Error(body?.error || String(response.status));
      const heard = body.request as Record<string, unknown>;
      if (what === 'codes') {
        if (typeof heard.audio_codes === 'string') setAudioCodes(heard.audio_codes);
      } else {
        remember();
        applyRequest(heard, { keepSource: true });
      }
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setListening(null);
    }
  };

  const togglePreview = (songId: string) => {
    const player = preview.current;
    if (!player) return;
    if (playing === songId) {
      player.pause();
      setPlaying(null);
      return;
    }
    player.src = apiUrl(`/v1/library/media/${encodeURIComponent(songId)}`);
    void player.play().then(() => setPlaying(songId)).catch(() => setPlaying(null));
  };

  const loadIdea = () => {
    const next = randomIdea();
    setIdea(next.description);
    if (typeof next.instrumental === 'boolean') setInstrumental(next.instrumental);
    if (next.vocal_language) setLanguage(next.vocal_language);
  };

  const loadExample = () => {
    remember();
    applyRequest({ ...randomExample() } as Record<string, unknown>, { keepSource: true });
  };

  const totalTracks = (numberOrUndefined(songs) ?? 1) * (numberOrUndefined(takes) ?? 1);

  const submit = () => {
    if (!ready) { setError(t('downloadProfileFirst')); return; }
    if (!caption.trim() && !baseOnly(task)) { setError(t('captionRequired')); return; }
    if (needsSource(task) && !sourceSong) { setError(tt('aceSourceRequired')); return; }
    if (baseOnly(task) && tracks.length === 0) { setError(tt('aceTrackRequired')); return; }
    if (totalTracks > MAX_TAKES) { setError(tt('aceTooManyTakes')); return; }
    setError(null);
    const count = Math.min(Math.max(numberOrUndefined(bulk) ?? 1, 1), 10);
    for (let index = 0; index < count; index++) {
      const request = buildRequest();
      if (index > 0) {
        // every queued job is its own song: its own seeds, its own title
        delete request.seed;
        delete request.lm_seed;
        if (request.title) request.title = `${request.title} (${index + 1})`;
      }
      onGenerate(request);
    }
  };

  const formFields: Record<string, [unknown, (value: string) => void]> = {
    title: [name, setName],
    caption: [caption, setCaption],
    lyrics: [lyrics, setLyrics],
    vocal_language: [language, setLanguage],
    bpm: [bpm, setBpm],
    keyscale: [keyscale, setKeyscale],
    timesignature: [timesignature, setTimesignature],
    duration: [duration, setDuration],
    task_type: [task, value => setTask(value as Task)],
    source_song_id: [sourceSong, setSourceSong],
    reference_song_id: [referenceSong, setReferenceSong],
    audio_cover_strength: [coverStrength, setCoverStrength],
    cover_noise_strength: [coverNoise, setCoverNoise],
    repainting_start: [repaintStart, setRepaintStart],
    repainting_end: [repaintEnd, setRepaintEnd],
    inference_steps: [steps, setSteps],
    guidance_scale: [guidance, setGuidance],
    shift: [shift, setShift],
    solver: [solver, setSolver],
    scheduler: [scheduler, setScheduler],
    guidance: [guidanceMode, setGuidanceMode],
    lm_temperature: [lmTemperature, setLmTemperature],
    lm_cfg_scale: [lmCfg, setLmCfg],
    lm_top_p: [lmTopP, setLmTopP],
    lm_top_k: [lmTopK, setLmTopK],
    lm_seed: [lmSeed, setLmSeed],
    lm_batch_size: [songs, setSongs],
    synth_batch_size: [takes, setTakes],
    seed: [seed, setSeed],
    audio_codes: [audioCodes, setAudioCodes],
    cover_prompt: [coverPrompt, setCoverPrompt],
    output_format: [format, value => setFormat(value as AceCreateRequest['output_format'])],
    mp3_bitrate: [mp3Bitrate, setMp3Bitrate],
    peak_clip: [peakClip, setPeakClip],
  };
  useBridgeCommand('create_get', () => ({
    mode,
    fields: { ...Object.fromEntries(Object.entries(formFields).map(([key, [value]]) => [key, value])), instrumental, think, tracks, randomize_seed: randomizeSeed, adapters },
    request: buildRequest(),
    ready,
    error,
    assistant_writing: assisting,
  }));
  useBridgeCommand('create_set', (args) => {
    const fields = (args.fields && typeof args.fields === 'object' ? args.fields : args) as Record<string, unknown>;
    const extra = ['mode', 'instrumental', 'think', 'tracks', 'randomize_seed', 'adapters'];
    const choices: Record<string, string[]> = { mode: ['studio', 'simple'], task_type: TASKS, output_format: ['mp3', 'wav16', 'wav24', 'wav32', 'flac'] };
    const unknown = Object.keys(fields).filter(key => !formFields[key] && !extra.includes(key));
    if (unknown.length) throw new Error(`Unknown fields: ${unknown.join(', ')}. The form has: ${[...Object.keys(formFields), ...extra].join(', ')}.`);
    for (const [key, allowed] of Object.entries(choices)) {
      if (key in fields && !allowed.includes(String(fields[key] ?? ''))) throw new Error(`${key} is one of: ${allowed.join(', ')}.`);
    }
    if ('tracks' in fields && !Array.isArray(fields.tracks)) throw new Error(`tracks is a list of: ${TRACKS.join(', ')}.`);
    if ('adapters' in fields && !Array.isArray(fields.adapters)) throw new Error('adapters is a list of {id, scales}.');
    for (const [key, value] of Object.entries(fields)) {
      if (key === 'mode') setMode(value as 'simple' | 'studio');
      else if (key === 'instrumental') setInstrumental(Boolean(value));
      else if (key === 'think') setThink(Boolean(value));
      else if (key === 'tracks') setTracks((value as unknown[]).map(String));
      else if (key === 'randomize_seed') setRandomizeSeed(Boolean(value));
      else if (key === 'adapters') setAdapters(value as AdapterUse[]);
      else formFields[key][1](value == null ? '' : String(value));
    }
    return { text: 'Filled in; create_form_get shows the form, ui_screenshot shows it on screen.' };
  });
  useBridgeCommand('create_submit', () => {
    submit();
    return { text: 'Pressed Create. studio_status shows the new job; if the form refused, create_form_get says why under error.' };
  });

  const profileLabel = useMemo(() => {
    if (setup?.selected_component_ids?.length) return t('customSet');
    const id = setup?.selected_profile_id;
    return id ? profileNames[id] ?? id : '—';
  }, [setup, t, profileNames]);

  const roles: Array<{ key: 'synth_model' | 'lm_model' | 'vae'; label: string; options: string[] }> = [
    { key: 'synth_model', label: 'DiT', options: catalog?.models?.dit ?? [] },
    { key: 'lm_model', label: 'LM', options: catalog?.models?.lm ?? [] },
    { key: 'vae', label: 'VAE', options: catalog?.models?.vae ?? [] },
  ];
  const baseModels = (catalog?.models?.dit ?? []).filter(file => ditKind(file) === 'base');
  const songOptions = (value: string, onChange: (value: string) => void, empty: string, target: 'source' | 'reference') => (
    <div className="flex items-center gap-1.5">
      <select value={value} onChange={event => onChange(event.target.value)} className={CONTROL}>
        <option value="">{empty}</option>
        {library.map(song => <option key={song.id} value={song.id}>{song.title}</option>)}
      </select>
      {value && (
        <button type="button" onClick={() => togglePreview(value)} className={ICON} title={tt('acePreview')}>
          {playing === value ? <Pause size={14} /> : <Play size={14} />}
        </button>
      )}
      <label className={`${ICON} cursor-pointer`} title={tt('aceUpload')}>
        {uploading === target ? <Loader2 size={14} className="animate-spin" /> : <Upload size={14} />}
        <input type="file" accept="audio/*,.mp3,.wav,.flac,.ogg,.m4a" className="hidden" onChange={event => { const file = event.target.files?.[0]; if (file) void upload(target, file); event.target.value = ''; }} />
      </label>
    </div>
  );
  const busy = assisting !== null || planning !== null;

  return (
    <section className="flex h-full min-h-0 w-full flex-col overflow-hidden bg-zinc-50 text-zinc-900 dark:bg-suno-panel dark:text-white">
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain custom-scrollbar">
        <div className="space-y-3 p-4 pb-6">
          <div className="flex items-center justify-between gap-3">
            <div className="min-w-0">
              <h1 className="truncate text-base font-bold">{t('createMusic')}</h1>
              <p className="mt-0.5 truncate text-[11px] text-zinc-500 dark:text-zinc-400">{t('localInference')}</p>
            </div>
            <span className={`shrink-0 rounded-full px-2.5 py-1 text-[10px] font-semibold ${ready ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-300' : 'bg-amber-500/10 text-amber-700 dark:text-amber-300'}`}>
              <span className={`mr-1 inline-block h-1.5 w-1.5 rounded-full ${ready ? 'bg-emerald-500' : 'bg-amber-500'}`} />
              {serviceDown ? t('serviceUnavailable') : ready ? t('engineReady') : t('profileRequired')}
            </span>
          </div>

          {serviceDown ? (
            <div className="flex gap-2 rounded-xl border border-rose-500/30 bg-rose-500/10 p-3 text-xs leading-5 text-rose-700 dark:text-rose-200">
              <CircleAlert className="mt-0.5 shrink-0" size={15} />
              <div><b>{t('serviceUnavailable')}</b><br />{t('serviceUnavailableHint')}</div>
            </div>
          ) : !ready && (
            <div className="flex gap-2 rounded-xl border border-amber-500/25 bg-amber-500/10 p-3 text-xs leading-5 text-amber-800 dark:text-amber-200">
              <CircleAlert className="mt-0.5 shrink-0" size={15} />
              <div><b>{t('localGenerationUnavailable')}</b><br />{t('downloadProfileFirst')}</div>
            </div>
          )}

          <div className="flex items-center rounded-lg border border-zinc-300 bg-zinc-200 p-1 dark:border-white/5 dark:bg-black/40">
            {(['studio', 'simple'] as const).map(value => (
              <button
                key={value}
                type="button"
                onClick={() => setMode(value)}
                className={`flex-1 rounded-md py-1.5 text-xs font-semibold transition-all ${mode === value ? 'bg-white text-black shadow-sm dark:bg-zinc-800 dark:text-white' : 'text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-300'}`}
              >
                {value === 'studio' ? t('studioMode') : t('simpleMode')}
              </button>
            ))}
          </div>

          <audio ref={preview} onEnded={() => setPlaying(null)} className="hidden" />
          {mode === 'simple' && (
            <Card title={t('songIdea')} actions={<button type="button" onClick={loadIdea} className={ICON} title={tt('aceRandomIdea')}><Dices size={14} /></button>}>
              <AutoTextarea
                value={idea}
                minRows={3}
                onChange={event => setIdea(event.target.value)}
                placeholder={tt('aceIdeaPlaceholder')}
                className={`${CONTROL} resize-none`}
              />
              <div className="mt-3 grid grid-cols-2 gap-2">
                <Field label={tt('aceLanguage')}>
                  <select value={language} onChange={event => setLanguage(event.target.value)} className={CONTROL}>
                    {LANGUAGES.map(code => <option key={code} value={code}>{code ? tt(`aceLang_${code}`) : tt('aceAuto')}</option>)}
                  </select>
                </Field>
                <div className="flex items-end pb-2"><Switch checked={instrumental} onChange={setInstrumental} label={t('instrumental')} /></div>
              </div>
              <p className="mt-2 text-[11px] leading-4 text-zinc-500">{tt('aceIdeaHint')}</p>
              <button
                type="button"
                onClick={() => void plan('inspire')}
                disabled={busy || !idea.trim() || !ready}
                className="mt-3 inline-flex w-full items-center justify-center gap-2 rounded-lg bg-gradient-to-r from-orange-500 to-pink-600 py-2.5 text-xs font-bold text-white transition hover:brightness-110 disabled:opacity-50"
              >
                {planning === 'inspire' ? <Loader2 size={14} className="animate-spin" /> : <Lightbulb size={14} />}
                {planning === 'inspire' ? `${tt('acePlanning')} · ${assistSeconds} ${t('secondsShort')}` : tt('aceInspire')}
              </button>
              {assistantReady && (
                <button
                  type="button"
                  onClick={() => void askAssistant('all')}
                  disabled={busy || !idea.trim()}
                  className="mt-2 inline-flex w-full items-center justify-center gap-2 rounded-lg border border-zinc-300 py-2 text-xs font-semibold text-zinc-600 transition-colors hover:border-pink-400 hover:text-pink-600 disabled:opacity-50 dark:border-white/15 dark:text-zinc-300"
                >
                  {assisting === 'all' ? <Loader2 size={13} className="animate-spin" /> : <Wand2 size={13} />}
                  {assisting === 'all' ? `${t('assistantWriting')} · ${assistSeconds} ${t('secondsShort')}` : t('writeEverything')}
                </button>
              )}
              {assisting === 'all' && (
                <button type="button" onClick={stopAssistant} className="mt-2 inline-flex w-full items-center justify-center gap-2 rounded-lg border border-zinc-300 py-2 text-xs font-semibold text-zinc-600 transition-colors hover:border-rose-400 hover:text-rose-600 dark:border-white/15 dark:text-zinc-300">
                  <Square size={13} />
                  {t('cancelDownload')}
                </button>
              )}
              {!assistantReady && (
                <button
                  type="button"
                  onClick={() => window.dispatchEvent(new CustomEvent('studio:open-settings', { detail: 'models' }))}
                  className="mt-2 inline-flex items-center gap-1 text-[11px] text-zinc-500 hover:text-pink-500"
                >
                  <Settings2 size={12} />
                  {t('setUpAssistant')}
                </button>
              )}
            </Card>
          )}

          {activity.filter(entry => entry.state !== 'done').slice(-3).map(entry => (
            <div key={`${entry.song_id}-${entry.kind}`} className="rounded-xl border border-zinc-200 bg-white px-3 py-2 text-[11px] dark:border-white/10 dark:bg-suno-card">
              <div className="flex items-center gap-2">
                {entry.state === 'running' ? <Loader2 size={12} className="animate-spin text-pink-500" /> : <AlertTriangle size={12} className="text-amber-500" />}
                <span className="font-semibold text-zinc-700 dark:text-zinc-200">{entry.kind === 'cover' ? t('activityCover') : t('activityKaraoke')}</span>
                <span className="min-w-0 flex-1 truncate text-zinc-500">{entry.title}</span>
              </div>
              {entry.detail && <p className="mt-1 break-words text-[11px] leading-4 text-amber-600 dark:text-amber-300">{karaokeReason(t, entry.detail)}</p>}
            </div>
          ))}

          {assisting !== null && (
            <div className="rounded-xl border border-zinc-200 bg-white p-3 dark:border-white/10 dark:bg-suno-card">
              <div className="flex items-center justify-between gap-2 text-[11px] font-semibold uppercase tracking-wide">
                <span className="flex items-center gap-1.5 text-pink-600 dark:text-pink-300">
                  <Loader2 size={12} className="animate-spin" />
                  {assistStage === 'sent' ? t('assistStageSent') : assistStage === 'writing' ? t('assistStageWriting') : assistStage === 'done' ? t('assistStageDone') : t('assistStagePreparing')}
                </span>
                <span className="tabular-nums text-zinc-400">{assistSeconds} {t('secondsShort')}</span>
              </div>
              {assistModel && <p className="mt-1 truncate text-[11px] text-zinc-500">{assistModel}</p>}
              {assistDraft && (
                <pre className="mt-2 max-h-40 overflow-y-auto whitespace-pre-wrap break-words rounded-lg bg-zinc-50 p-2 font-mono text-[11px] leading-4 text-zinc-600 dark:bg-black/30 dark:text-zinc-300">{assistDraft.slice(-1200)}</pre>
              )}
            </div>
          )}

          <Card title={tt('aceTask')}>
            <div className="grid grid-cols-2 gap-1.5 sm:grid-cols-4">
              {TASKS.map(value => (
                <button
                  key={value}
                  type="button"
                  onClick={() => setTask(value)}
                  className={`rounded-lg border px-2 py-1.5 text-[11px] font-semibold transition-colors ${task === value ? 'border-pink-500 bg-pink-500/10 text-pink-600 dark:text-pink-300' : 'border-zinc-200 text-zinc-600 hover:border-pink-300 dark:border-white/10 dark:text-zinc-300'}`}
                  title={tt(`aceTaskHint_${value}`)}
                >
                  {tt(`aceTask_${value}`)}
                </button>
              ))}
            </div>
            <p className="mt-2 text-[11px] leading-4 text-zinc-500">{tt(`aceTaskHint_${task}`)}</p>
            {baseOnly(task) && kind !== 'base' && (
              <p className="mt-2 rounded-lg bg-amber-500/10 p-2 text-[11px] leading-4 text-amber-700 dark:text-amber-300">
                {baseModels.length ? tt('aceBaseOnlyPick') : tt('aceBaseOnlyInstall')}
                {baseModels.length > 0 && (
                  <select value="" onChange={event => event.target.value && setModels(current => ({ ...current, synth_model: event.target.value }))} className={`${CONTROL} mt-2`}>
                    <option value="">{tt('aceChooseBase')}</option>
                    {baseModels.map(file => <option key={file} value={file}>{file.replace(/\.gguf$/, '')}</option>)}
                  </select>
                )}
              </p>
            )}
            {needsSource(task) && (
              <div className="mt-3 space-y-3">
                <Field label={tt('aceSource')} hint={tt('aceSourceHint')}>{songOptions(sourceSong, setSourceSong, tt('aceChooseTrack'), 'source')}</Field>
                {sourceSong && (
                  <div className="flex flex-wrap gap-1.5">
                    <button type="button" onClick={() => void listen('describe')} disabled={listening !== null || !ready} className="inline-flex items-center gap-1.5 rounded-lg border border-zinc-200 px-2.5 py-1.5 text-[11px] font-semibold text-zinc-600 hover:border-pink-400 hover:text-pink-600 disabled:opacity-50 dark:border-white/10 dark:text-zinc-300">
                      {listening === 'describe' ? <Loader2 size={12} className="animate-spin" /> : <Ear size={12} />}{tt('aceDescribeSource')}
                    </button>
                    <button type="button" onClick={() => void listen('codes')} disabled={listening !== null || !ready} className="inline-flex items-center gap-1.5 rounded-lg border border-zinc-200 px-2.5 py-1.5 text-[11px] font-semibold text-zinc-600 hover:border-pink-400 hover:text-pink-600 disabled:opacity-50 dark:border-white/10 dark:text-zinc-300">
                      {listening === 'codes' ? <Loader2 size={12} className="animate-spin" /> : <ListChecks size={12} />}{tt('aceSourceCodes')}
                    </button>
                  </div>
                )}
                {(task === 'cover' || task === 'cover-nofsq') && (
                  <>
                    <SliderRow label={tt('aceCoverStrength')} value={coverStrength} fallback={1} min={0} max={1} step={0.05} onChange={setCoverStrength} />
                    <SliderRow label={tt('aceCoverNoise')} value={coverNoise} fallback={0} min={0} max={1} step={0.05} onChange={setCoverNoise} />
                    <p className="text-[11px] leading-4 text-zinc-500">{tt('aceCoverHint')}</p>
                  </>
                )}
                {(task === 'repaint' || task === 'lego') && (
                  <div className="grid grid-cols-2 gap-2">
                    <Field label={tt('aceRepaintStart')}><input value={repaintStart} onChange={event => setRepaintStart(event.target.value)} placeholder="0" inputMode="decimal" className={CONTROL} /></Field>
                    <Field label={tt('aceRepaintEnd')}><input value={repaintEnd} onChange={event => setRepaintEnd(event.target.value)} placeholder={tt('aceToTheEnd')} inputMode="decimal" className={CONTROL} /></Field>
                  </div>
                )}
                {(task === 'repaint' || task === 'lego') && <p className="text-[11px] leading-4 text-zinc-500">{tt('aceRepaintHint')}</p>}
                {baseOnly(task) && (
                  <div>
                    <span className={LABEL}>{task === 'extract' ? tt('aceTrackExtract') : task === 'lego' ? tt('aceTrackAdd') : tt('aceTrackComplete')}</span>
                    <div className="flex flex-wrap gap-1.5">
                      {TRACKS.map(track => {
                        const on = tracks.includes(track);
                        return (
                          <button
                            key={track}
                            type="button"
                            onClick={() => setTracks(current => (task === 'complete' ? (on ? current.filter(item => item !== track) : [...current, track]) : on ? [] : [track]))}
                            className={`rounded-full border px-2.5 py-1 text-[11px] ${on ? 'border-pink-500 bg-pink-500/10 text-pink-600 dark:text-pink-300' : 'border-zinc-200 text-zinc-600 dark:border-white/10 dark:text-zinc-300'}`}
                          >
                            {tt(`aceTrack_${track}`)}
                          </button>
                        );
                      })}
                    </div>
                  </div>
                )}
              </div>
            )}
            <div className="mt-3">
              <Field label={tt('aceReference')} hint={tt('aceReferenceHint')}>{songOptions(referenceSong, setReferenceSong, tt('aceNoReference'), 'reference')}</Field>
            </div>
          </Card>

          <Card
            title={tt('aceSong')}
            actions={
              <>
                {undo && (
                  <button type="button" onClick={() => { setCaption(undo.caption); setLyrics(undo.lyrics); setUndo(null); }} className={ICON} title={tt('aceUndo')}><Undo2 size={14} /></button>
                )}
                <button type="button" onClick={loadExample} className={ICON} title={t('examplePrompt')}><Dices size={14} /></button>
                <button type="button" onClick={() => void plan('format')} disabled={busy || !ready || (!caption.trim() && !lyrics.trim())} className={ICON} title={tt('aceFormat')}>
                  {planning === 'format' ? <Loader2 size={14} className="animate-spin" /> : <ListChecks size={14} className="text-pink-500" />}
                </button>
                {assistantReady && (
                  <button type="button" onClick={() => void askAssistant('prompt')} disabled={busy} className={ICON} title={t('writeCaption')}>
                    {assisting === 'prompt' ? <Loader2 size={14} className="animate-spin" /> : <Wand2 size={14} className="text-pink-500" />}
                  </button>
                )}
                <button type="button" onClick={() => promptFile.current?.click()} className={ICON} title={t('openPrompt')}><FolderOpen size={14} /></button>
                <button type="button" onClick={savePrompt} className={ICON} title={t('savePrompt')}><Save size={14} /></button>
                <button type="button" onClick={reset} className={ICON} title={t('resetPrompt')}><RotateCcw size={14} /></button>
                <input
                  ref={promptFile}
                  type="file"
                  accept="application/json,.json"
                  className="hidden"
                  onChange={event => { const file = event.target.files?.[0]; if (file) void openPrompt(file); event.target.value = ''; }}
                />
              </>
            }
          >
            <input
              value={name}
              onChange={event => setName(event.target.value)}
              placeholder={t('untitled')}
              className="w-full border-0 bg-transparent p-0 text-lg font-bold text-zinc-900 outline-none placeholder:text-zinc-300 dark:text-white dark:placeholder:text-zinc-600"
            />
            <div className="mt-3">
              <Field label={tt('aceCaption')} hint={tt('aceCaptionHint')}>
                <AutoTextarea value={caption} minRows={3} onChange={event => setCaption(event.target.value)} placeholder={tt('aceCaptionPlaceholder')} className={`${CONTROL} resize-none`} />
              </Field>
            </div>
            <div className="mt-2 flex flex-wrap items-center gap-1.5">
              {genres.map(genre => (
                <button key={genre} type="button" onClick={() => setCaption(current => (current.trim() ? `${current.trim().replace(/,$/, '')}, ${genre}` : genre))} className="rounded-full border border-zinc-200 bg-zinc-100 px-2.5 py-1 text-[10px] font-medium text-zinc-600 hover:border-pink-300 hover:text-pink-600 dark:border-white/5 dark:bg-white/5 dark:text-zinc-400">
                  {genre}
                </button>
              ))}
              <button type="button" onClick={() => setGenres(someGenres())} className={ICON} title={tt('aceMoreGenres')}><Dices size={13} /></button>
            </div>
            <div className="mt-3 grid grid-cols-2 gap-2 sm:grid-cols-5">
              <Field label="BPM"><input value={bpm} onChange={event => setBpm(event.target.value)} placeholder={tt('aceAuto')} inputMode="numeric" className={CONTROL} /></Field>
              <Field label={tt('aceKey')}>
                <select value={keyscale} onChange={event => setKeyscale(event.target.value)} className={CONTROL}>
                  {KEYS.map(key => <option key={key} value={key}>{key || tt('aceAuto')}</option>)}
                </select>
              </Field>
              <Field label={tt('aceTimeSignature')}>
                <select value={timesignature} onChange={event => setTimesignature(event.target.value)} className={CONTROL}>
                  {TIME_SIGNATURES.map(value => <option key={value} value={value}>{value ? (value === '6' ? '6/8' : `${value}/4`) : tt('aceAuto')}</option>)}
                </select>
              </Field>
              <Field label={tt('aceDuration')}><input value={duration} onChange={event => setDuration(event.target.value)} placeholder={tt('aceAuto')} inputMode="numeric" className={CONTROL} /></Field>
              <Field label={tt('aceLanguage')}>
                <select value={language} onChange={event => setLanguage(event.target.value)} className={CONTROL}>
                  {LANGUAGES.map(code => <option key={code} value={code}>{code ? tt(`aceLang_${code}`) : tt('aceAuto')}</option>)}
                </select>
              </Field>
            </div>
            {!instrumental && (
              <div className="mt-3">
                <span className={LABEL}>{tt('aceGender')}</span>
                <div className="flex gap-1.5">
                  {(['', 'male', 'female'] as const).map(value => (
                    <button key={value || 'any'} type="button" onClick={() => setGender(value)} className={`flex-1 rounded-lg border px-2 py-1.5 text-[11px] font-semibold ${gender === value ? 'border-pink-500 bg-pink-500/10 text-pink-600 dark:text-pink-300' : 'border-zinc-200 text-zinc-600 dark:border-white/10 dark:text-zinc-300'}`}>
                      {tt(`aceGender_${value || 'any'}`)}
                    </button>
                  ))}
                </div>
              </div>
            )}
            <p className="mt-2 text-[11px] leading-4 text-zinc-500">{tt('aceMetadataHint')}</p>
          </Card>

          <Card
            title={t('lyrics')}
            actions={
              <>
                {assistantReady && (
                  <button type="button" onClick={() => void layOutLyrics()} disabled={busy || !lyrics.trim()} className={ICON} title={t('formatLyrics')}>
                    {assisting === 'sections' ? <Loader2 size={14} className="animate-spin" /> : <Tags size={14} />}
                  </button>
                )}
                {assistantReady && (
                  <button type="button" onClick={() => void askAssistant('lyrics')} disabled={busy} className={ICON} title={t('writeLyrics')}>
                    {assisting === 'lyrics' ? <Loader2 size={14} className="animate-spin" /> : <Wand2 size={14} className="text-pink-500" />}
                  </button>
                )}
                <button type="button" onClick={() => setLyrics('')} className={ICON} title={t('resetPrompt')}><RotateCcw size={14} /></button>
              </>
            }
          >
            <div className="mb-3"><Switch checked={instrumental} onChange={setInstrumental} label={t('instrumental')} hint={tt('aceInstrumentalHint')} /></div>
            {!instrumental && (
              <AutoTextarea
                value={lyrics}
                minRows={10}
                onChange={event => setLyrics(event.target.value)}
                placeholder={'[Verse 1]\n…\n\n[Chorus]\n…'}
                className={`${CONTROL} resize-none font-mono text-xs leading-5`}
              />
            )}
            <p className="mt-2 text-[11px] leading-4 text-zinc-500">{instrumental ? tt('aceInstrumentalHint') : tt('aceLyricsHint')}</p>
          </Card>

          <AdapterPicker
            value={adapters}
            onChange={setAdapters}
            onTrigger={applyTrigger}
            iconClass={ICON}
            frame={(title, _icon, actions, body) => <Card title={title} actions={actions}>{body}</Card>}
          />

          <Card
            title={t('quality')}
            actions={
              <button type="button" onClick={resetParameters} className="rounded-md px-2 py-1 text-[10px] font-semibold text-zinc-500 transition-colors hover:bg-zinc-200 hover:text-black dark:hover:bg-white/10 dark:hover:text-white">
                {t('resetToDefaults')}
              </button>
            }
          >
            <div className="space-y-3">
              <Switch checked={think} onChange={setThink} label={tt('aceThink')} hint={tt('aceThinkHint')} />
              <SliderRow label={t('ditSteps')} value={steps} fallback={dit.steps} min={1} max={turbo ? 20 : 150} step={1} onChange={setSteps} />
              {!turbo && <SliderRow label={t('cfgScale')} value={guidance} fallback={dit.guidance} min={1} max={15} step={0.5} onChange={setGuidance} />}
              <p className="text-[11px] leading-4 text-zinc-500">{tt(`aceModelKind_${kind}`)}</p>
            </div>
            <div className="mt-4 space-y-3 border-t border-zinc-100 pt-4 dark:border-white/5">
              <SliderRow label={tt('aceSongs')} value={songs} fallback={1} min={1} max={MAX_TAKES} step={1} onChange={setSongs} />
              <SliderRow label={t('variationsBatch')} value={takes} fallback={1} min={1} max={MAX_TAKES} step={1} onChange={setTakes} />
              <p className="text-[11px] leading-4 text-zinc-500">{tt('aceSongsHint')}</p>
              <SliderRow label={tt('aceBulk')} value={bulk} fallback={1} min={1} max={10} step={1} onChange={setBulk} />
              <p className="text-[11px] leading-4 text-zinc-500">{tt('aceBulkHint')}</p>
              <Switch checked={randomizeSeed} onChange={setRandomizeSeed} label={t('randomizeSeed')} />
              {!randomizeSeed && (
                <Field label={t('seedShort')}>
                  <input value={seed} onChange={event => setSeed(event.target.value)} placeholder="0" inputMode="numeric" className={CONTROL} />
                </Field>
              )}
              {totalTracks > 1 && (
                <p className={`text-[11px] ${totalTracks > MAX_TAKES ? 'text-rose-600 dark:text-rose-300' : 'text-zinc-500'}`}>
                  {t('renderCountPrefix')} <b>{totalTracks}</b>{totalTracks > MAX_TAKES && ` · ${tt('aceTooManyTakes')}`}
                </p>
              )}
            </div>
          </Card>

          <div className="overflow-hidden rounded-xl border border-zinc-200 bg-white dark:border-white/5 dark:bg-suno-card">
            <button
              type="button"
              onClick={() => setShowAdvanced(current => !current)}
              className="flex w-full items-center justify-between gap-2 px-3 py-2 text-[11px] font-bold uppercase tracking-wide text-zinc-500 transition-colors hover:text-black dark:text-zinc-400 dark:hover:text-white"
            >
              {t('advanced')}
              <ChevronDown size={15} className={showAdvanced ? 'rotate-180 transition-transform' : 'transition-transform'} />
            </button>
            {showAdvanced && (
              <div className="space-y-4 border-t border-zinc-100 p-3 dark:border-white/5">
                <Stage title={t('stageLm')} hint={tt('aceStageLmHint')}>
                  <div className="space-y-3">
                    <Switch checked={cotCaption} onChange={setCotCaption} label={tt('aceCotCaption')} hint={tt('aceCotCaptionHint')} />
                    <SliderRow label={tt('aceTemperature')} value={lmTemperature} fallback={0.85} min={0} max={2} step={0.05} onChange={setLmTemperature} />
                    <SliderRow label={t('cfgScale')} value={lmCfg} fallback={2} min={1} max={4} step={0.1} onChange={setLmCfg} />
                    <SliderRow label="Top-P" value={lmTopP} fallback={0.9} min={0} max={1} step={0.01} onChange={setLmTopP} />
                    <SliderRow label={t('topK')} value={lmTopK} fallback={0} min={0} max={200} step={1} onChange={setLmTopK} />
                    <Field label={tt('aceNegative')}><input value={lmNegative} onChange={event => setLmNegative(event.target.value)} className={CONTROL} /></Field>
                    <Field label={t('lmSeedShort')}><input value={lmSeed} onChange={event => setLmSeed(event.target.value)} placeholder={tt('aceRandom')} inputMode="numeric" className={CONTROL} /></Field>
                    <Field label={tt('aceCodes')} hint={tt('aceCodesHint')}>
                      <textarea value={audioCodes} onChange={event => setAudioCodes(event.target.value)} rows={2} className={`${CONTROL} resize-y font-mono text-[11px]`} />
                    </Field>
                  </div>
                </Stage>

                <div className="border-t border-zinc-100 pt-4 dark:border-white/5">
                  <Stage title={tt('aceStageDit')} hint={tt('aceStageDitHint')}>
                    <div className="space-y-3">
                      <SliderRow label={tt('aceShift')} value={shift} fallback={dit.shift} min={1} max={5} step={0.1} onChange={setShift} />
                      <div className="grid grid-cols-2 gap-2">
                        <Field label={tt('aceSolver')}>
                          <select value={solver || dit.solver} onChange={event => setSolver(event.target.value)} className={CONTROL}>
                            {SOLVERS.map(value => <option key={value} value={value}>{tt(`aceSolver_${value}`)}</option>)}
                          </select>
                        </Field>
                        <Field label={tt('aceScheduler')}>
                          <select value={scheduler || dit.scheduler} onChange={event => setScheduler(event.target.value)} className={CONTROL}>
                            {SCHEDULERS.map(value => <option key={value} value={value}>{tt(`aceScheduler_${value}`)}</option>)}
                          </select>
                        </Field>
                      </div>
                      <p className="text-[11px] leading-4 text-zinc-500">{tt('aceSamplerHint')}</p>
                      {!turbo && (
                        <>
                          <Field label={tt('aceGuidanceMode')}>
                            <select value={guidanceMode || 'apg'} onChange={event => setGuidanceMode(event.target.value)} className={CONTROL}>
                              {GUIDANCE.map(value => <option key={value} value={value}>{tt(`aceGuidance_${value}`)}</option>)}
                            </select>
                          </Field>
                          <div className="grid grid-cols-2 gap-2">
                            <SliderRow label={tt('aceApgMomentum')} value={apgMomentum} fallback={0.75} min={0} max={0.99} step={0.01} onChange={setApgMomentum} />
                            <SliderRow label={tt('aceApgNorm')} value={apgNorm} fallback={2.5} min={0} max={10} step={0.1} onChange={setApgNorm} />
                          </div>
                          <div className="grid grid-cols-2 gap-2">
                            <SliderRow label={tt('aceCfgStart')} value={cfgStart} fallback={0} min={0} max={1} step={0.05} onChange={setCfgStart} />
                            <SliderRow label={tt('aceCfgEnd')} value={cfgEnd} fallback={1} min={0} max={1} step={0.05} onChange={setCfgEnd} />
                          </div>
                          <p className="text-[11px] leading-4 text-zinc-500">{tt('aceCfgIntervalHint')}</p>
                        </>
                      )}
                      <div className="grid grid-cols-3 gap-2">
                        <Field label="DCW">
                          <select value={dcwMode} onChange={event => setDcwMode(event.target.value)} className={CONTROL}>
                            <option value="">{tt('aceOff')}</option>
                            {['low', 'high', 'double', 'pix'].map(value => <option key={value} value={value}>{value}</option>)}
                          </select>
                        </Field>
                        <Field label={tt('aceDcwScaler')}><input value={dcwScaler} onChange={event => setDcwScaler(event.target.value)} placeholder="0.1" inputMode="decimal" disabled={!dcwMode} className={CONTROL} /></Field>
                        <Field label={tt('aceDcwHigh')}><input value={dcwHigh} onChange={event => setDcwHigh(event.target.value)} placeholder="0" inputMode="decimal" disabled={dcwMode !== 'double'} className={CONTROL} /></Field>
                      </div>
                      <p className="text-[11px] leading-4 text-zinc-500">{tt('aceDcwHint')}</p>
                      <Field label={tt('aceTimesteps')} hint={tt('aceTimestepsHint')}>
                        <input value={customTimesteps} onChange={event => setCustomTimesteps(event.target.value)} placeholder="0.97,0.76,0.615,0.5,0.395,0.28,0.18,0.085,0" className={`${CONTROL} font-mono text-[11px]`} />
                      </Field>
                      <SliderRow label={tt('aceRetake')} value={retakeVariance} fallback={0} min={0} max={1} step={0.01} onChange={setRetakeVariance} />
                      {numberOrUndefined(retakeVariance) ? (
                        <Field label={tt('aceRetakeSeed')}><input value={retakeSeed} onChange={event => setRetakeSeed(event.target.value)} placeholder={tt('aceRandom')} inputMode="numeric" className={CONTROL} /></Field>
                      ) : null}
                      <p className="text-[11px] leading-4 text-zinc-500">{tt('aceRetakeHint')}</p>
                      <div className="grid grid-cols-2 gap-2">
                        <Field label={tt('aceLatentShift')}><input value={latentShift} onChange={event => setLatentShift(event.target.value)} placeholder="0" inputMode="decimal" className={CONTROL} /></Field>
                        <Field label={tt('aceLatentRescale')}><input value={latentRescale} onChange={event => setLatentRescale(event.target.value)} placeholder="1" inputMode="decimal" className={CONTROL} /></Field>
                      </div>
                    </div>
                  </Stage>
                </div>

                {adapters.length > 0 && (
                  <div className="border-t border-zinc-100 pt-4 dark:border-white/5">
                    <Stage title={tt('aceGroupScales')} hint={tt('aceGroupScalesHint')}>
                      <div className="grid grid-cols-2 gap-3">
                        {GROUPS.map(group => (
                          <SliderRow key={group} label={tt(`aceGroup_${group}`)} value={groups[group] ?? ''} fallback={1} min={0} max={2} step={0.05} onChange={value => setGroups(current => ({ ...current, [group]: value }))} />
                        ))}
                      </div>
                    </Stage>
                  </div>
                )}

                <div className="border-t border-zinc-100 pt-4 dark:border-white/5">
                  <Stage title={t('stageOutput')} hint={t('stageOutputHint')}>
                    <SliderRow label={t('peakClipLabel')} value={peakClip} fallback={10} min={0} max={30} step={1} onChange={setPeakClip} />
                    <div className="mt-3 grid grid-cols-2 gap-2">
                      <Field label={t('mp3Bitrate')}>
                        <select value={mp3Bitrate || '320'} onChange={event => setMp3Bitrate(event.target.value)} disabled={format !== 'mp3'} className={CONTROL}>
                          {['128', '192', '256', '320'].map(rate => <option key={rate} value={rate}>{rate} kbps</option>)}
                        </select>
                      </Field>
                      <Field label={t('outputFormat')}>
                        <select value={format} onChange={event => setFormat(event.target.value as AceCreateRequest['output_format'])} className={CONTROL}>
                          <option value="mp3">MP3</option>
                          <option value="wav16">WAV16</option>
                          <option value="wav24">WAV24</option>
                          <option value="wav32">WAV32</option>
                          <option value="flac">FLAC</option>
                        </select>
                      </Field>
                    </div>
                    <p className="mt-2 text-[11px] leading-4 text-zinc-500">{t('peakClipHint')}</p>
                    <div className="mt-3 grid grid-cols-2 gap-2">
                      <Field label={tt('aceFadeIn')}><input value={fadeIn} onChange={event => setFadeIn(event.target.value)} placeholder="0" inputMode="decimal" className={CONTROL} /></Field>
                      <Field label={tt('aceFadeOut')}><input value={fadeOut} onChange={event => setFadeOut(event.target.value)} placeholder="0" inputMode="decimal" className={CONTROL} /></Field>
                    </div>
                  </Stage>
                </div>

                <div className="border-t border-zinc-100 pt-4 dark:border-white/5">
                  <Stage title={t('componentOverride')} hint={tt('aceComponentsHint')}>
                    <div className="space-y-2">
                      {roles.map(role => (
                        <div key={role.key} className="grid grid-cols-[48px_1fr] items-center gap-2">
                          <span className="text-[11px] font-semibold text-zinc-500 dark:text-zinc-400">{role.label}</span>
                          <select
                            value={models[role.key] ?? ''}
                            onChange={event => setModels(current => {
                              const next = { ...current };
                              if (event.target.value) next[role.key] = event.target.value;
                              else delete next[role.key];
                              return next;
                            })}
                            className={CONTROL}
                          >
                            <option value="">{setup?.profile_files?.[role.key] ? `${t('profileDefault')} · ${setup.profile_files[role.key].replace(/\.gguf$/, '')}` : t('profileDefault')}</option>
                            {role.options.map(option => <option key={option} value={option}>{option.replace(/\.gguf$/, '')}</option>)}
                          </select>
                        </div>
                      ))}
                    </div>
                  </Stage>
                </div>
              </div>
            )}
          </div>

          <div className="flex items-center justify-between px-1 text-[11px] text-zinc-500 dark:text-zinc-400">
            <button type="button" onClick={() => window.dispatchEvent(new CustomEvent('studio:open-settings', { detail: 'models' }))} className="text-left hover:text-pink-500" title={t('changeProfileHint')}>
              {t('profile')}: <b className="text-zinc-700 underline decoration-dotted underline-offset-2 dark:text-zinc-200">{profileLabel}</b>
            </button>
            <button type="button" onClick={() => void refreshSetup().catch(() => undefined)} className="hover:text-pink-500">{t('refresh')}</button>
          </div>
          {error && <div role="alert" className="rounded-xl border border-red-500/30 bg-red-500/10 p-3 text-xs leading-5 text-red-700 dark:text-red-200">{error}</div>}
        </div>
      </div>

      <footer className="shrink-0 border-t border-zinc-200 bg-zinc-50/95 p-4 backdrop-blur dark:border-white/5 dark:bg-suno-panel/95">
        <button
          type="button"
          onClick={submit}
          disabled={activeJobCount >= 10}
          className="flex h-12 w-full items-center justify-center gap-2 rounded-xl bg-gradient-to-r from-orange-500 to-pink-600 text-base font-bold text-white shadow-lg transition hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {isGenerating ? <Square size={18} /> : <Sparkles size={18} />}
          {t('create')}
          {activeJobCount > 0 && <span className="rounded-full bg-white/20 px-2 py-0.5 text-xs">{activeJobCount}/10</span>}
        </button>
      </footer>
    </section>
  );
};

/**
 * Adapter training: the optional pack, engine-neutral datasets and the runs
 * that turn them into LoRA.
 */

export interface PackFile {
  id: string;
  label: string;
  bytes: number;
  installed: boolean;
}

export interface DatasetItem {
  id: string;
  title: string;
  /** Who sings it; the lyrics are looked up by it. */
  artist: string;
  /** Where the lyrics came from: a lyrics database, "recognised", or empty when written by hand. */
  lyrics_source: string;
  style: string;
  lyrics: string;
  instrumental: boolean;
  file: string;
  seconds: number;
  source: string;
  /** Where the lyrics stand: to be found, found and still to be laid out, or done. */
  lyrics_state: 'wanted' | 'found' | 'done';
  /** Where the style stands: to be heard, heard and still to be written, or done. */
  style_state: 'wanted' | 'heard' | 'done';
}

export interface Dataset {
  id: string;
  name: string;
  trigger: string;
  created_at: string;
  items: DatasetItem[];
}

/** A run's settings, as the engine names them; `recipe_fields` says how to show each. */
export type Recipe = Record<string, number | string | boolean>;

export interface FieldCondition {
  field: string;
  values: string[];
}

/** One setting of the recipe form, described by the engine. */
export interface RecipeField {
  key: string;
  group: string;
  kind: 'number' | 'integer' | 'choice' | 'toggle';
  min?: number;
  max?: number;
  step?: number;
  choices?: string[];
  shown_when?: FieldCondition;
  off_when?: FieldCondition;
}

export type RunStatus = 'running' | 'done' | 'failed' | 'cancelled' | 'interrupted';

export interface TrainingRun {
  id: string;
  dataset_id: string;
  dataset_name: string;
  name: string;
  trigger: string;
  recipe: Recipe;
  status: RunStatus;
  stage: string | null;
  stages: string[];
  steps: { step: number; loss: number; ar_kl?: number | null; step_ms?: number | null }[];
  error?: string | null;
  created_at: string;
  finished_at?: string | null;
  installed: number[];
  checkpoints: number[];
  log?: string[];
  /** The step "train further" starts from, when the run can be continued. */
  resume_step?: number;
  /** Why the run cannot be continued: a code the page translates. */
  resume_refused?: 'method' | 'prepared_gone' | 'no_checkpoint' | 'no_state' | 'no_run';
  continuations?: { from: number; to: number; at: string }[];
}

export interface TrainingState {
  pack: PackFile[];
  pack_ready: boolean;
  recipe_defaults: Recipe;
  recipe_fields: RecipeField[];
  /** Video memory a run of the default recipe needs, in GB. */
  min_vram_gb: number;
  /** What a song's style field holds for this engine: a short style, or a structured caption. */
  item_style: 'style' | 'caption';
  download: { downloaded_bytes: number; total_bytes: number; done: boolean; error?: string | null } | null;
  /** The optional pack that describes songs by ear. */
  listen?: {
    pack: PackFile[];
    ready: boolean;
    download: { downloaded_bytes: number; total_bytes: number; done: boolean; error?: string | null } | null;
  };
  /** The dataset preparation in progress or the last one. */
  prepare: PrepareStatus | null;
  datasets: Dataset[];
  runs: TrainingRun[];
  active: string | null;
}

async function call<T>(url: string, init?: RequestInit): Promise<T> {
  const response = await fetch(url, init);
  if (response.status === 204) return undefined as T;
  const body = await response.json().catch(() => null);
  if (!response.ok) throw new Error(body?.error || `Training: HTTP ${response.status}`);
  return body as T;
}

const json = (method: string, body: unknown): RequestInit => ({
  method,
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify(body),
});

export const fetchTraining = () => call<TrainingState>('/v1/training');
export const installTrainingPack = () => call<void>('/v1/training/pack/install', { method: 'POST' });
export const installListenPack = () => call<void>('/v1/training/listen/install', { method: 'POST' });
export const cancelTrainingPack = () => call<void>('/v1/training/pack/cancel', { method: 'POST' });

export const createDataset = (name: string, trigger: string) => call<Dataset>('/v1/training/datasets', json('POST', { name, trigger }));
export const updateDataset = (id: string, patch: { name?: string; trigger?: string }) => call<Dataset>(`/v1/training/datasets/${id}`, json('PATCH', patch));
export const deleteDataset = (id: string) => call<void>(`/v1/training/datasets/${id}`, { method: 'DELETE' });
export const addLibrarySongs = (id: string, songIds: string[]) => call<Dataset>(`/v1/training/datasets/${id}/songs`, json('POST', { song_ids: songIds }));

/** Audio files, with any same-named .txt or .lrc taken as their lyrics. */
export function addFiles(id: string, files: File[], folder = ''): Promise<Dataset> {
  const form = new FormData();
  // the folders a song sits in name its artist and album when its tags do not
  for (const file of files) form.append('files', file, folder ? `${folder}/${file.name}` : file.name);
  return call<Dataset>(`/v1/training/datasets/${id}/files`, { method: 'POST', body: form });
}

/** A dataset folder from another studio: its dataset.json and the audio beside it. */
export function importDataset(files: File[]): Promise<Dataset> {
  const form = new FormData();
  for (const file of files) {
    const path = (file as File & { webkitRelativePath?: string }).webkitRelativePath || file.name;
    if (file.name === 'dataset.json' || file.name.toLowerCase().endsWith('.wav')) form.append('files', file, path);
  }
  return call<Dataset>('/v1/training/datasets/import', { method: 'POST', body: form });
}

export const revealDataset = (id: string) => call<void>(`/v1/training/datasets/${id}/reveal`, { method: 'POST' });

export const updateItem = (id: string, item: string, patch: Partial<Pick<DatasetItem, 'title' | 'artist' | 'style' | 'lyrics' | 'instrumental'>>) =>
  call<Dataset>(`/v1/training/datasets/${id}/items/${item}`, json('PATCH', patch));
export const deleteItem = (id: string, item: string) => call<Dataset>(`/v1/training/datasets/${id}/items/${item}`, { method: 'DELETE' });

export const startRun = (datasetId: string, name: string, recipe: Recipe) => call<TrainingRun>('/v1/training/runs', json('POST', { dataset_id: datasetId, name, recipe }));
export const cancelRun = (id: string) => call<void>(`/v1/training/runs/${id}/cancel`, { method: 'POST' });
export const continueRun = (id: string, steps: number) => call<TrainingRun>(`/v1/training/runs/${id}/continue`, json('POST', { steps }));
export const deleteRun = (id: string) => call<void>(`/v1/training/runs/${id}`, { method: 'DELETE' });
export const installCheckpoint = (id: string, step: number, name?: string) =>
  call<{ id: string }>(`/v1/training/runs/${id}/checkpoints/${step}/install`, json('POST', { name }));

export const gigabytes = (bytes: number) => `${(bytes / 1024 ** 3).toFixed(1)} GB`;
export const clock = (seconds: number) => `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, '0')}`;

/** Has the writing assistant write a song's structured caption, for engines that train on one. */
export const describeItem = (id: string, item: string) => call<Dataset>(`/v1/training/datasets/${id}/items/${item}/describe`, { method: 'POST' });

/** Which songs a preparation step is done for: none, only where the field is empty, or all over again. */
export type Fill = 'none' | 'missing' | 'all';

export interface PrepareRequest {
  items?: string[];
  lyrics: Fill;
  style: Fill;
  language?: string;
  /** Start this run once every song is ready. */
  train?: { name: string; recipe: Recipe };
}

/** The dataset preparation on the server: one job, each model loaded once for all its songs. */
export interface PrepareStage {
  name: 'lookup' | 'vocals' | 'lyrics' | 'listen' | 'writing' | 'styles' | 'training';
  done: number;
  total: number;
  /** The song the stage works on now. */
  current: string | null;
}

export interface PrepareStatus {
  dataset: string;
  /** The stages at work, the latest last. */
  stages: PrepareStage[];
  pending: string[];
  failures: { item: string; step: string; error: string }[];
  notices: string[];
  finished: boolean;
  cancelled: boolean;
  run: string | null;
  device: string | null;
  train_after: boolean;
}

export const prepareDataset = (id: string, request: PrepareRequest) => call<PrepareStatus>(`/v1/training/datasets/${id}/prepare`, json('POST', request));
export const cancelPrepare = () => call<void>('/v1/training/prepare/cancel', { method: 'POST' });
/** Starts this run once the preparation at work is done, or no run with null. */
export const setTrainAfter = (train: { name: string; recipe: Recipe } | null) => call<void>('/v1/training/prepare/train-after', json('POST', { train }));

/** A file picked or dropped, with the folder it came from inside what was chosen. */
export interface PickedFile {
  file: File;
  folder: string;
}

const AUDIO = /\.(wav|mp3|flac|ogg|m4a)$/i;
const SIDECAR = /\.(txt|lrc|cue)$/i;

/** Only what a dataset can use: audio, lyrics beside it, and cue sheets that cut albums. */
export const usable = (files: PickedFile[]) => files.filter(entry => AUDIO.test(entry.file.name) || SIDECAR.test(entry.file.name));
export const audioCount = (files: PickedFile[]) => files.filter(entry => AUDIO.test(entry.file.name)).length;

/** Files from an input, the folder taken from the path a folder picker gives them. */
export function pickedFromInput(list: FileList | null): PickedFile[] {
  return Array.from(list ?? []).map(file => {
    const path = (file as File & { webkitRelativePath?: string }).webkitRelativePath || '';
    return { file, folder: path.includes('/') ? path.slice(0, path.lastIndexOf('/')) : '' };
  });
}

/** Everything dropped, folders walked all the way down. */
export async function pickedFromDrop(items: DataTransferItemList): Promise<PickedFile[]> {
  const entries = Array.from(items)
    .map(item => item.webkitGetAsEntry?.())
    .filter((entry): entry is FileSystemEntry => Boolean(entry));
  const picked: PickedFile[] = [];
  const walk = async (entry: FileSystemEntry, folder: string): Promise<void> => {
    if (entry.isFile) {
      const file = await new Promise<File>((resolve, reject) => (entry as FileSystemFileEntry).file(resolve, reject));
      picked.push({ file, folder });
      return;
    }
    const reader = (entry as FileSystemDirectoryEntry).createReader();
    const inside = folder ? `${folder}/${entry.name}` : entry.name;
    for (;;) {
      const batch = await new Promise<FileSystemEntry[]>((resolve, reject) => reader.readEntries(resolve, reject));
      if (batch.length === 0) break;
      for (const child of batch) await walk(child, inside);
    }
  };
  for (const entry of entries) await walk(entry, '');
  return picked;
}

/** Adds picked files to a dataset folder by folder, so an album and its cue
 * sheet travel together and progress can be told. */
export async function addPicked(id: string, files: PickedFile[], onProgress: (done: number, total: number) => void): Promise<Dataset | null> {
  const folders = new Map<string, File[]>();
  for (const entry of usable(files)) folders.set(entry.folder, [...(folders.get(entry.folder) ?? []), entry.file]);
  const total = audioCount(files);
  let done = 0;
  let dataset: Dataset | null = null;
  onProgress(0, total);
  for (const [folder, group] of folders) {
    if (!group.some(file => AUDIO.test(file.name))) continue;
    dataset = await addFiles(id, group, folder);
    done += group.filter(file => AUDIO.test(file.name)).length;
    onProgress(done, total);
  }
  return dataset;
}

/** A dataset name from what was picked: the top folder, when there is one. */
export const nameFromPicked = (files: PickedFile[]) => files.find(entry => entry.folder)?.folder.split('/')[0] ?? '';

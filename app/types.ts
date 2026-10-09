export interface Song {
  /** A track a tool made from another: which one, by which tool, how. */
  derived?: { from: string; fromTitle: string; tool: string; settings?: Record<string, unknown> } | null;
  id: string;
  title: string;
  lyrics: string;
  style: string;
  coverUrl: string;
  /** The page a cover photograph came from and the terms it is under. */
  coverSource?: string;
  /** Set while the cover is the look's placeholder, not one of the track's own. */
  coverPlaceholder?: string;
  duration: string;
  createdAt: Date;
  isGenerating?: boolean;
  /** The engine job a generation's row follows. */
  jobId?: string;
  /** The playlist the song being made goes into. */
  playlistId?: string;
  /** The job that made a library song; its row becomes this song. */
  madeByJob?: string;
  /** The list row this item is drawn in, when it took over another item's row. */
  viewKey?: string;
  queuePosition?: number; // Position in queue (undefined = actively generating, number = waiting in queue)
  progress?: number;
  stage?: string;
  /** The running stage's counter, e.g. `57/64 · ~2:50`. */
  stageDetail?: string;
  generationParams?: any;
  tags: string[];
  audioUrl?: string;
  /** The thumbs-up, kept with the song in the library. */
  liked?: boolean;
  /** The person's own note on the song, kept in the library. */
  note?: string;
  /** When it was liked: the liked list is read from the latest. */
  likedAt?: Date;
  isPublic?: boolean;
  likeCount?: number;
  viewCount?: number;
  userId?: string;
  creator?: string;
  creator_avatar?: string;
  ditModel?: string;
  lmModel?: string;
  lmBackend?: string;
  openrouterModel?: string | null;
  generationTime?: number;
  lrcContent?: string;
  bpm?: number;
  keyScale?: string;
  timeSignature?: string;
  /** Native provenance is complete enough for POST /v1/music/replay. */
  nativeReplayAvailable?: boolean;
  /** Processed versions kept beside the original; the active one plays. */
  audioVersions?: SongVersion[];
  /** `original`, a version id, or absent for a track never processed. */
  activeVersion?: string;
}

export interface SongVersion {
  id: string;
  label: string;
  createdAt: string;
  settings?: Record<string, unknown>;
}

export interface Playlist {
  id: string;
  name: string;
  description?: string;
  coverUrl?: string;
  cover_url?: string;
  songIds?: string[];
  isPublic?: boolean;
  is_public?: boolean;
  user_id?: string;
  creator?: string;
  created_at?: string;
  song_count?: number;
  songs?: any[];
}

export interface Comment {
  id: string;
  songId: string;
  userId: string;
  username: string;
  content: string;
  createdAt: Date;
}

/**
 * A song request: acestep.cpp's own `AceRequest` fields, flat, plus what the
 * studio does around them (title, cover, the source and reference tracks,
 * adapters, fades). The server checks every engine field by name.
 */
export interface AceCreateRequest {
  caption: string;
  lyrics: string;
  task_type?: 'text2music' | 'cover' | 'cover-nofsq' | 'repaint' | 'lego' | 'extract' | 'complete';
  track?: string;
  think?: boolean;
  use_cot_caption?: boolean;
  bpm?: number;
  duration?: number;
  keyscale?: string;
  timesignature?: string;
  vocal_language?: string;
  seed?: number;
  lm_seed?: number;
  lm_batch_size?: number;
  synth_batch_size?: number;
  lm_temperature?: number;
  lm_cfg_scale?: number;
  lm_top_p?: number;
  lm_top_k?: number;
  lm_rep_penalty?: number;
  lm_rep_mode?: 'presence' | 'frequency' | 'dry';
  lm_rep_window?: number;
  lm_dry_base?: number;
  lm_dry_min_len?: number;
  lm_negative_prompt?: string;
  audio_codes?: string;
  inference_steps?: number;
  guidance_scale?: number;
  shift?: number;
  solver?: string;
  scheduler?: string;
  guidance?: string;
  apg_momentum?: number;
  apg_norm_threshold?: number;
  cfg_interval_start?: number;
  cfg_interval_end?: number;
  retake_seed?: number;
  retake_variance?: number;
  dcw_mode?: string;
  dcw_scaler?: number;
  dcw_high_scaler?: number;
  custom_timesteps?: string;
  latent_shift?: number;
  latent_rescale?: number;
  audio_cover_strength?: number;
  cover_noise_strength?: number;
  repainting_start?: number;
  repainting_end?: number;
  synth_model?: string;
  lm_model?: string;
  vae?: string;
  output_format: 'flac' | 'mp3';
  mp3_bitrate?: number;
  adapter_group_scales?: Record<string, number>;
  /** Studio fields, never sent to the engine as such. */
  title?: string;
  cover_prompt?: string;
  source_song_id?: string;
  reference_song_id?: string;
  adapters?: { id: string; scales: Record<string, number> }[];
  /** The playlist the made songs are added to: a project being worked on. */
  playlist_id?: string;
  fade_in?: number;
  fade_out?: number;
}

/** A song as the library stores and lists it. */
export interface NativeLibrarySong {
  id: string;
  title: string;
  audio_path?: string | null;
  caption: string;
  lyrics: string;
  metadata?: Record<string, unknown> | null;
  generation_settings?: Record<string, unknown> | null;
  engine_id: string;
  profile_id?: string | null;
  replay_request?: unknown | null;
  audio_codes?: unknown | null;
  created_at: string;
  updated_at?: string;
}

export interface AceJobSong {
  id: string;
  audio_url: string;
  /** The library's record of the song, as the library lists it. */
  song: NativeLibrarySong;
}

export interface AceJob {
  id: string;
  /** The mark this window gave the request; an agent's job has none. */
  client_ref?: string;
  /** When the service took the request, in Unix milliseconds. */
  submitted_at: number;
  status: 'queued' | 'running' | 'completed' | 'failed' | 'cancelled';
  phase: string;
  message: string;
  title?: string;
  caption: string;
  lyrics: string;
  duration_seconds: number;
  generation_settings: Record<string, unknown>;
  playlist_id?: string;
  song?: AceJobSong;
  songs?: AceJobSong[];
}

/** How far the engine is with the job it renders, as the service reads its log. */
export interface AceProgress {
  stage: 'planning' | 'rendering' | 'decoding';
  fraction: number;
  detail: string;
}


export interface PlayerState {
  currentSong: Song | null;
  isPlaying: boolean;
  progress: number;
  volume: number;
}

export interface User {
  id: string;
  username: string;
  createdAt: Date;
  followerCount?: number;
  followingCount?: number;
  isFollowing?: boolean;
  isAdmin?: boolean;
  avatar_url?: string;
  banner_url?: string;
}

export interface UserProfile {
  user: User;
  publicSongs: Song[];
  publicPlaylists: Playlist[];
  stats: {
    totalSongs: number;
    totalLikes: number;
  };
}

// Simplified views for ACE-Step UI
export type View = 'create' | 'library' | 'tools' | 'adapters' | 'playlist' | 'search' | 'news';

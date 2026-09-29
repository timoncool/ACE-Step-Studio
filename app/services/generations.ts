import { useCallback, useEffect, useEffectEvent, useMemo, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useI18n } from '../context/I18nContext';
import type { AceCreateRequest, AceJob, AceProgress, Song } from '../types';
import { STUDIO } from '../studio';
import { followEngineProgress } from './engineProgress';
import { mapNativeLibrarySong } from './nativeLibrary';
import { playlistsChanged, queryClient, readJson, updateLibrarySongs, useLibrarySongs } from './studioQueries';

/**
 * The songs being made, as cards in the list. A card follows its engine job
 * and, when the job is done, hands its row to the song the job made: the row
 * keeps its React key and its place and only changes what it shows (the view
 * key of TanStack DB's temporary ids), so nothing closes and reopens.
 */

const activeJobsKey = ['music', 'jobs'] as const;
const POLL_MS = 1_500;
const NO_SONGS: Song[] = [];

const STAGE_LABEL: Record<AceProgress['stage'], string> = {
  planning: 'stagePlanning',
  rendering: 'stageRendering',
  decoding: 'stageDecoding',
};

type Notify = (message: string, type: 'success' | 'error' | 'info') => void;

interface GenerationOptions {
  /** The engine is up; before that there are no jobs to follow. */
  enabled: boolean;
  notify: Notify;
  /** A card became the songs its job made, the first of them in its row. */
  onFinished: (cardId: string, songs: Song[]) => void;
}

interface CardFields {
  title: string;
  style?: string;
  lyrics?: string;
  jobId?: string;
  createdAt?: Date;
}

function card(id: string, fields: CardFields): Song {
  return {
    id,
    jobId: fields.jobId,
    title: fields.title,
    style: fields.style ?? '',
    lyrics: fields.lyrics ?? '',
    coverUrl: '',
    duration: '--:--',
    createdAt: fields.createdAt ?? new Date(),
    isGenerating: true,
    stage: 'stageWaitingInQueue',
    tags: [STUDIO.slug],
  };
}

/** Asks the engine to stop a job; a refusal is told, the card is already marked. */
async function stopJob(jobId: string): Promise<void> {
  const response = await fetch(`/v1/music/jobs/${encodeURIComponent(jobId)}`, { method: 'POST' });
  if (!response.ok) throw new Error(`The engine did not stop the job (${response.status})`);
}

export function useGenerations({ enabled, notify, onFinished }: GenerationOptions) {
  const { t } = useI18n();
  const [cards, setCards] = useState<Song[]>([]);
  // job -> the row its card was drawn in; kept after the card is gone, so the
  // song that took the row keeps it
  const [rowOfJob, setRowOfJob] = useState<ReadonlyMap<string, string>>(() => new Map());
  const library = useLibrarySongs().data ?? NO_SONGS;
  const settling = useRef(new Set<string>());
  const cardsNow = useRef(cards);
  useEffect(() => {
    cardsNow.current = cards;
  }, [cards]);

  // the newest song each job made; the library lists newest first
  const madeBy = useMemo(() => {
    const first = new Map<string, Song>();
    for (const song of library) {
      if (song.madeByJob && !first.has(song.madeByJob)) first.set(song.madeByJob, song);
    }
    return first;
  }, [library]);

  // a card stays until the library holds its song, so its row is never empty
  const waiting = useMemo(() => cards.filter(entry => !(entry.jobId && madeBy.has(entry.jobId))), [cards, madeBy]);
  const following = waiting.filter(entry => entry.jobId && entry.isGenerating).length;

  useEffect(() => {
    if (waiting.length !== cards.length) setCards(prev => prev.filter(entry => !(entry.jobId && madeBy.has(entry.jobId))));
  }, [waiting, cards, madeBy]);

  const follow = useCallback((cardId: string, jobId: string) => {
    setCards(prev => prev.map(entry => (entry.id === cardId ? { ...entry, jobId } : entry)));
    setRowOfJob(prev => (prev.has(jobId) ? prev : new Map(prev).set(jobId, cardId)));
  }, []);

  const remove = useCallback((cardId: string) => {
    setCards(prev => prev.filter(entry => entry.id !== cardId));
  }, []);

  // One read of the running jobs for all cards, while any is running. It goes
  // on in a hidden window: "without stopping" sends the next song from there.
  const jobs = useQuery({
    queryKey: activeJobsKey,
    queryFn: () => readJson<AceJob[]>('/v1/music/jobs'),
    enabled,
    refetchInterval: query => (following > 0 || (query.state.data?.length ?? 0) > 0 ? POLL_MS : false),
    refetchIntervalInBackground: true,
  });

  const finish = useEffectEvent((finished: Song, job: AceJob) => {
    // newest first, as the library lists them, so the first takes the row
    const made = (job.songs?.length ? job.songs : job.song ? [job.song] : [])
      .map(entry => mapNativeLibrarySong(entry.song))
      .sort((a, b) => (a.id < b.id ? 1 : -1));
    if (made.length === 0) {
      remove(finished.id);
      notify(job.message || t('generationFailed'), 'error');
      return;
    }
    updateLibrarySongs(songs => [...made.filter(song => !songs.some(entry => entry.id === song.id)), ...songs]);
    if (job.playlist_id) playlistsChanged();
    onFinished(finished.id, made);
    notify(made.length > 1 ? `${made.length} ${t('tracksReady')}` : t('trackReady'), 'success');
  });

  const settle = useEffectEvent(async (running: Song) => {
    const jobId = running.jobId;
    if (!jobId || settling.current.has(jobId)) return;
    settling.current.add(jobId);
    try {
      const job = await readJson<AceJob>(`/v1/music/jobs/${encodeURIComponent(jobId)}`);
      if (job.status === 'completed') {
        finish(running, job);
      } else if (job.status === 'failed' || job.status === 'cancelled') {
        remove(running.id);
        notify(job.message || t('generationFailed'), job.status === 'failed' ? 'error' : 'info');
      }
      // still queued or running: the list was read before this job was sent
    } catch (error) {
      remove(running.id);
      notify(error instanceof Error ? error.message : String(error), 'error');
    } finally {
      settling.current.delete(jobId);
    }
  });

  const reconcile = useEffectEvent((listed: AceJob[]) => {
    const running = new Set(listed.map(job => job.id));
    // an agent's jobs, and the ones sent before this window was opened
    const shown = new Set(cards.flatMap(entry => (entry.jobId ? [entry.id, entry.jobId] : [entry.id])));
    const fresh = listed.filter(job => !shown.has(job.id) && !(job.client_ref && shown.has(job.client_ref)));
    if (fresh.length > 0) {
      setCards(prev => [
        ...fresh.map(job => card(`restored_${job.id}`, {
          title: job.title || t('generating'),
          style: job.caption,
          lyrics: job.lyrics,
          jobId: job.id,
          createdAt: new Date(job.submitted_at),
        })),
        ...prev,
      ]);
      setRowOfJob(prev => {
        const next = new Map(prev);
        for (const job of fresh) next.set(job.id, `restored_${job.id}`);
        return next;
      });
    }
    // a job that left the list finished, failed or was stopped elsewhere
    for (const entry of waiting) {
      if (entry.jobId && entry.isGenerating && !running.has(entry.jobId)) void settle(entry);
    }
  });

  useEffect(() => {
    if (jobs.data) reconcile(jobs.data);
  }, [jobs.data, jobs.dataUpdatedAt]);

  // The engine renders one job at a time in the order they came, so the
  // oldest running card is the one its progress belongs to.
  const making = following > 0;
  useEffect(() => {
    if (!making) return;
    return followEngineProgress(progress => {
      if (!progress) return;
      const stage = STAGE_LABEL[progress.stage];
      setCards(prev => {
        const running = prev.filter(entry => entry.isGenerating && entry.jobId);
        if (running.length === 0) return prev;
        const active = running.reduce((oldest, entry) => (entry.createdAt < oldest.createdAt ? entry : oldest));
        return prev.map(entry => {
          if (!entry.isGenerating || !entry.jobId) return entry;
          if (entry.id === active.id) {
            return entry.progress === progress.fraction && entry.stage === stage ? entry : { ...entry, progress: progress.fraction, stage };
          }
          return !entry.progress && entry.stage === 'stageWaitingInQueue' ? entry : { ...entry, progress: 0, stage: 'stageWaitingInQueue' };
        });
      });
    });
  }, [making]);

  const generate = useCallback(async (request: AceCreateRequest) => {
    const id = `temp_${Date.now()}_${Math.random().toString(36).slice(2, 11)}`;
    setCards(prev => [card(id, { title: request.title?.trim() || t('generating'), style: request.caption, lyrics: request.lyrics }), ...prev]);
    try {
      const response = await fetch('/v1/music/jobs', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ ...request, client_ref: id }),
      });
      const job = (await response.json().catch(() => null)) as (AceJob & { error?: string }) | null;
      if (!response.ok || !job || job.status === 'failed') {
        throw new Error(job?.message || job?.error || `The engine rejected this request (${response.status})`);
      }
      follow(id, job.id);
    } catch (error) {
      remove(id);
      notify(error instanceof Error ? error.message : t('generationFailed'), 'error');
    }
  }, [follow, remove, notify, t]);

  /** A job sent from elsewhere in this window (a re-render) gets its card. */
  const track = useCallback((jobId: string, fields: CardFields) => {
    const id = `replay_${jobId}`;
    setCards(prev => (prev.some(entry => entry.jobId === jobId)
      ? prev.map(entry => (entry.jobId === jobId ? { ...entry, title: fields.title, style: fields.style ?? entry.style, lyrics: fields.lyrics ?? entry.lyrics } : entry))
      : [card(id, { ...fields, jobId }), ...prev]));
    setRowOfJob(prev => (prev.has(jobId) ? prev : new Map(prev).set(jobId, id)));
  }, []);

  const cancel = useCallback(async (jobId: string) => {
    setCards(prev => prev.map(entry => (entry.jobId === jobId ? { ...entry, isGenerating: false, stage: 'cancelled' } : entry)));
    try {
      await stopJob(jobId);
    } catch (error) {
      notify(error instanceof Error ? error.message : String(error), 'error');
    }
  }, [notify]);

  /** Drops a cancelled card, stopping its job if it still runs. */
  const reset = useCallback(async (key: string) => {
    const target = cardsNow.current.find(entry => entry.jobId === key || entry.id === key);
    if (!target) return;
    remove(target.id);
    if (target.jobId && target.isGenerating) {
      try {
        await stopJob(target.jobId);
      } catch (error) {
        notify(error instanceof Error ? error.message : String(error), 'error');
      }
    }
  }, [remove, notify]);

  const cancelAll = useCallback(async () => {
    // "without stopping" would send the form again the moment the queue empties
    window.dispatchEvent(new CustomEvent('studio:cancel-all'));
    const mine = cardsNow.current.flatMap(entry => (entry.jobId && entry.isGenerating ? [entry.jobId] : []));
    setCards(prev => prev.filter(entry => !entry.isGenerating));
    // The service's list, not only this window's: a request whose answer is
    // still on its way back is on neither list here, and would run to the end.
    let listed: AceJob[] = [];
    try {
      listed = await readJson<AceJob[]>('/v1/music/jobs');
    } catch (error) {
      notify(error instanceof Error ? error.message : String(error), 'error');
    }
    const results = await Promise.allSettled([...new Set([...mine, ...listed.map(job => job.id)])].map(stopJob));
    const refused = results.find((result): result is PromiseRejectedResult => result.status === 'rejected');
    if (refused) notify(refused.reason instanceof Error ? refused.reason.message : String(refused.reason), 'error');
    void queryClient.invalidateQueries({ queryKey: activeJobsKey });
  }, [notify]);

  // The list for the create page: the cards, then the library; the newest
  // song of a job is drawn in the row its card had.
  const keyed = useMemo(() => library.map(song => {
    const row = song.madeByJob && madeBy.get(song.madeByJob) === song ? rowOfJob.get(song.madeByJob) : undefined;
    return row ? { ...song, viewKey: row } : song;
  }), [library, madeBy, rowOfJob]);
  const songs = useMemo(() => (waiting.length ? [...waiting, ...keyed] : keyed), [waiting, keyed]);

  return {
    songs,
    activeJobCount: following,
    isGenerating: waiting.some(entry => entry.isGenerating),
    generate,
    track,
    cancel,
    reset,
    cancelAll,
  };
}

if (typeof window !== 'undefined') {
  // an agent's job, or one sent before a reload, is read at once
  window.addEventListener('studio:jobs-changed', () => {
    void queryClient.invalidateQueries({ queryKey: activeJobsKey });
  });
}

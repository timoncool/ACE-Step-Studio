import { apiUrl } from './apiBase';
import type { AceProgress } from '../types';

/**
 * The running job's progress, pushed by the studio service as the engine log
 * moves: one stream while songs are being made, instead of a request every
 * moment. Returns the unsubscribe.
 */
export function followEngineProgress(onProgress: (progress: AceProgress | null) => void): () => void {
  const events = new EventSource(apiUrl('/v1/engine/progress'));
  events.onmessage = (message) => onProgress(JSON.parse(message.data) as AceProgress | null);
  return () => events.close();
}

export interface StageClock { stage: string; done: number; at: number }

/** The stage's counter and, once it has moved, the time its remaining steps take at the pace so far. */
export function stageDetail(detail: string, stage: string, clock: { current: StageClock | null }, now = Date.now()): string {
  const [doneText, totalText] = detail.split('/');
  const done = Number(doneText);
  const total = Number(totalText);
  if (!detail || !Number.isFinite(done)) return '';
  if (clock.current?.stage !== stage || done < clock.current.done) clock.current = { stage, done, at: now };
  if (!totalText || !Number.isFinite(total)) return String(done);
  const moved = done - clock.current.done;
  if (moved <= 0 || done >= total) return `${done}/${total}`;
  const left = Math.round(((now - clock.current.at) / moved) * (total - done) / 1000);
  return `${done}/${total} · ~${Math.floor(left / 60)}:${String(left % 60).padStart(2, '0')}`;
}

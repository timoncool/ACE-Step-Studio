import { mapNativeLibrarySong } from './nativeLibrary';
import { libraryChanged } from './studioQueries';
import type { NativeLibrarySong, Song } from '../types';

/**
 * The MIDI editor is signal (github.com/ryohey/signal; the studio build of our
 * fork sits in public/midi-editor) opened in a frame. It holds one song at a
 * time: the studio hands it a track's MIDI or a new song, and asks for the
 * song back as a MIDI file, or as WAV played through its SoundFont when the
 * song becomes a library track of its own.
 */

export const MIDI_EDITOR_PAGE = `${import.meta.env.BASE_URL}midi-editor/studio.html`;

export type EditorEvent = { type: 'ready' } | { type: 'dirty'; dirty: boolean; name: string };

type Reply =
  | { type: 'midi'; id: number; data: ArrayBuffer; name: string }
  | { type: 'wav'; id: number; data: ArrayBuffer }
  | { type: 'failed'; id: number; error: string };

export class MidiEditorLink {
  private nextId = 1;
  private waiting = new Map<number, { resolve: (reply: Reply) => void; reject: (error: Error) => void }>();

  constructor(private readonly frame: HTMLIFrameElement, private readonly onEvent: (event: EditorEvent) => void) {}

  /** Starts hearing the editor; the returned function stops it. */
  listen(): () => void {
    const onMessage = (event: MessageEvent) => {
      const data = event.data;
      if (event.source !== this.frame.contentWindow || event.origin !== window.location.origin || data?.signal !== true) return;
      if (data.type === 'ready' || data.type === 'dirty') {
        this.onEvent(data as EditorEvent);
        return;
      }
      const waiting = this.waiting.get(data.id);
      if (!waiting) return;
      this.waiting.delete(data.id);
      if (data.type === 'failed') waiting.reject(new Error(data.error));
      else waiting.resolve(data as Reply);
    };
    window.addEventListener('message', onMessage);
    return () => {
      window.removeEventListener('message', onMessage);
      for (const waiting of this.waiting.values()) waiting.reject(new Error('the MIDI editor was closed'));
      this.waiting.clear();
    };
  }

  private send(message: object, transfer: Transferable[] = []): void {
    const target = this.frame.contentWindow;
    if (!target) throw new Error('the MIDI editor is not open');
    target.postMessage({ studio: true, ...message }, window.location.origin, transfer);
  }

  private ask(type: 'midi' | 'wav'): Promise<Reply> {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.waiting.set(id, { resolve, reject });
      try {
        this.send({ type, id });
      } catch (error) {
        this.waiting.delete(id);
        reject(error instanceof Error ? error : new Error(String(error)));
      }
    });
  }

  open(data: ArrayBuffer, name: string): void {
    this.send({ type: 'open', data, name }, [data]);
  }

  newSong(name: string): void {
    this.send({ type: 'new', name });
  }

  saved(): void {
    this.send({ type: 'saved' });
  }

  look(language: string, theme: 'dark' | 'light'): void {
    this.send({ type: 'look', language, theme });
  }

  async midi(): Promise<{ data: ArrayBuffer; name: string }> {
    const reply = await this.ask('midi');
    if (reply.type !== 'midi') throw new Error('the MIDI editor answered with something else');
    return { data: reply.data, name: reply.name };
  }

  async wav(): Promise<ArrayBuffer> {
    const reply = await this.ask('wav');
    if (reply.type !== 'wav') throw new Error('the MIDI editor answered with something else');
    return reply.data;
  }
}

async function answer<T>(response: Response): Promise<T> {
  const body = await response.json().catch(() => null);
  if (!response.ok) throw new Error(body?.error || `HTTP ${response.status}`);
  return body as T;
}

function base64Of(data: ArrayBuffer): string {
  const bytes = new Uint8Array(data);
  let raw = '';
  for (let index = 0; index < bytes.length; index += 0x8000) raw += String.fromCharCode(...bytes.subarray(index, index + 0x8000));
  return btoa(raw);
}

/** A library track's MIDI file. */
export async function trackMidi(songId: string): Promise<ArrayBuffer> {
  const response = await fetch(`/v1/library/songs/${encodeURIComponent(songId)}/midi/file`);
  if (!response.ok) await answer(response);
  return response.arrayBuffer();
}

/** The edited MIDI kept as the track's MIDI, in place of the one it had. */
export async function keepTrackMidi(songId: string, data: ArrayBuffer): Promise<void> {
  await answer(await fetch(`/v1/library/songs/${encodeURIComponent(songId)}/midi`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ data: base64Of(data) }),
  }));
}

/** A new library track: the song's audio as the editor rendered it, with its MIDI kept beside it. */
export async function addMidiTrack(title: string, wav: ArrayBuffer, midi: ArrayBuffer): Promise<Song> {
  const name = title.trim() || 'MIDI';
  const form = new FormData();
  form.append('audio', new Blob([wav], { type: 'audio/wav' }), `${name.replace(/[\\/:*?"<>|]+/g, ' ').trim() || 'MIDI'}.wav`);
  form.append('title', name);
  const song = await answer<NativeLibrarySong>(await fetch('/v1/library/import', { method: 'POST', body: form }));
  await keepTrackMidi(song.id, midi);
  libraryChanged();
  return mapNativeLibrarySong(song);
}

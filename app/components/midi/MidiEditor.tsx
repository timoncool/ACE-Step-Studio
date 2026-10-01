import React, { useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { AudioLines, Library, Loader2, Piano, Save, X } from 'lucide-react';
import { useI18n } from '../../context/I18nContext';
import type { TranslationKey } from '../../i18n/translations';
import { addMidiTrack, keepTrackMidi, MIDI_EDITOR_PAGE, MidiEditorLink, trackMidi, type EditorEvent } from '../../services/midiEditor';
import { mapNativeLibrarySong } from '../../services/nativeLibrary';
import type { Song } from '../../types';
import { ConfirmDialog } from '../ConfirmDialog';

/**
 * What the editor opens and where it goes back to: a new song, kept as a
 * library track; or a library track's MIDI, kept on the track.
 */
export type MidiEditorSource =
  | { kind: 'new' }
  | { kind: 'track'; songId: string; title: string };

const theme = (): 'dark' | 'light' => (document.documentElement.classList.contains('dark') ? 'dark' : 'light');

const BUTTON = 'inline-flex h-8 items-center gap-1.5 rounded-lg px-3 text-xs font-semibold transition disabled:cursor-not-allowed disabled:opacity-50';
const PLAIN = `${BUTTON} border border-zinc-300 text-zinc-700 hover:border-pink-400 hover:text-pink-600 dark:border-white/15 dark:text-zinc-200`;
const MAIN = `${BUTTON} bg-linear-to-r from-orange-500 to-pink-600 font-bold text-white hover:brightness-110`;

/**
 * The one piano-roll editor of the studio (signal, in a frame) in a window
 * over the page, whichever place opened it.
 */
export const MidiEditor: React.FC<{ source: MidiEditorSource; onClose: () => void }> = ({ source, onClose }) => {
  const { t, language } = useI18n();
  const say = useCallback((key: string, values: Record<string, string | number> = {}) => Object.entries(values).reduce((text, [name, value]) => text.split(`{${name}}`).join(String(value)), t(key as TranslationKey)), [t]);
  const frame = useRef<HTMLIFrameElement | null>(null);
  const link = useRef<MidiEditorLink | null>(null);
  const opened = useRef(false);
  const [ready, setReady] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [name, setName] = useState(source.kind === 'new' ? '' : source.title);
  const [made, setMade] = useState<Song | null>(null);
  const [busy, setBusy] = useState<'save' | 'cover' | null>(null);
  const [status, setStatus] = useState<{ text: string; bad: boolean } | null>(null);
  const [asking, setAsking] = useState(false);
  const [page] = useState(() => `${MIDI_EDITOR_PAGE}?studio=1&lang=${language}&theme=${theme()}`);
  // what the window was opened on: the link to the frame lives as long as the
  // window, and a request waiting on it (a render takes seconds) must not be
  // dropped by a re-render
  const start = useRef({ source, untitled: say('midiEdUntitled') });

  useEffect(() => {
    const element = frame.current;
    if (!element) return;
    const { source: opening, untitled } = start.current;
    const fail = (problem: unknown) => setStatus({ text: problem instanceof Error ? problem.message : String(problem), bad: true });
    const editor = new MidiEditorLink(element, (event: EditorEvent) => {
      if (event.type === 'dirty') {
        setDirty(event.dirty);
        if (event.name) setName(event.name);
        return;
      }
      setReady(true);
      if (opened.current) return;
      opened.current = true;
      if (opening.kind === 'track') {
        trackMidi(opening.songId).then(data => editor.open(data, opening.title)).catch(fail);
      } else {
        editor.newSong(untitled);
      }
    });
    link.current = editor;
    const stop = editor.listen();
    return () => {
      stop();
      link.current = null;
    };
  }, []);

  useEffect(() => {
    if (ready) link.current?.look(language, theme());
  }, [language, ready]);

  useEffect(() => {
    const watch = new MutationObserver(() => link.current?.look(language, theme()));
    watch.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] });
    return () => watch.disconnect();
  }, [language]);

  const editor = (): MidiEditorLink => {
    if (!link.current) throw new Error('the MIDI editor is not open');
    return link.current;
  };

  /** Keeps the song: on its track, or as a new track; the track it is kept on. */
  const keep = async (): Promise<Song> => {
    if (source.kind === 'track') {
      const { data } = await editor().midi();
      await keepTrackMidi(source.songId, data);
      editor().saved();
      window.dispatchEvent(new CustomEvent('studio:midi-changed', { detail: { songId: source.songId } }));
      setStatus({ text: say('midiEdSavedTrack', { title: source.title }), bad: false });
      const response = await fetch(`/v1/library/songs/${encodeURIComponent(source.songId)}`);
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      return mapNativeLibrarySong(await response.json());
    }
    const { data, name: songName } = await editor().midi();
    setStatus({ text: say('midiEdRendering'), bad: false });
    const wav = await editor().wav();
    const song = await addMidiTrack(songName || say('midiEdUntitled'), wav, data);
    editor().saved();
    setMade(song);
    setStatus({ text: say('midiEdSavedNew', { title: song.title }), bad: false });
    return song;
  };

  const run = async (what: 'save' | 'cover') => {
    setBusy(what);
    setStatus(null);
    try {
      const song = what === 'cover' && source.kind === 'new' && made && !dirty ? made : await keep();
      if (what === 'cover') {
        window.dispatchEvent(new CustomEvent('studio:cover-song', { detail: song }));
        onClose();
      }
    } catch (problem) {
      setStatus({ text: problem instanceof Error ? problem.message : String(problem), bad: true });
    } finally {
      setBusy(null);
    }
  };

  const close = () => {
    if (dirty) setAsking(true);
    else onClose();
  };

  return createPortal(
    <>
      <div className="fixed inset-0 z-70 flex items-center justify-center bg-black/60 p-2 sm:p-4">
        <div
          role="dialog"
          aria-modal="true"
          aria-labelledby="midi-editor-title"
          onKeyDown={event => {
            if (event.key !== 'Escape' || asking) return;
            event.preventDefault();
            event.stopPropagation();
            close();
          }}
          className="flex h-[94vh] w-full max-w-[1600px] flex-col overflow-hidden rounded-2xl bg-white shadow-2xl dark:bg-zinc-900"
        >
          <div className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-zinc-200 px-4 py-3 dark:border-white/10 sm:px-5">
            <h3 id="midi-editor-title" className="flex min-w-0 items-center gap-2 text-base font-bold text-zinc-900 dark:text-white">
              <Piano size={18} className="shrink-0 text-pink-500" />
              <span className="truncate">{name.trim() ? say('midiEdTitleOf', { title: name.trim() }) : say('midiEdTitle')}</span>
              {dirty && <span className="shrink-0 rounded-full bg-amber-500/15 px-2 py-0.5 text-[10px] font-semibold text-amber-700 dark:text-amber-300">{say('midiEdUnsaved')}</span>}
            </h3>
            <div className="ml-auto flex flex-wrap items-center gap-2">
              {status && (
                <span role={status.bad ? 'alert' : 'status'} className={`max-w-[28rem] truncate text-xs ${status.bad ? 'text-rose-600 dark:text-rose-400' : 'text-zinc-500'}`} title={status.text}>
                  {status.text}
                </span>
              )}
              <button
                type="button"
                onClick={() => void run('save')}
                disabled={!ready || busy !== null}
                title={say(source.kind === 'track' ? 'midiEdSaveTrackHint' : 'midiEdSaveNewHint')}
                className={PLAIN}
              >
                {busy === 'save' ? <Loader2 size={13} className="animate-spin" /> : source.kind === 'track' ? <Save size={13} /> : <Library size={13} />}
                {say(source.kind === 'track' ? 'midiEdSaveTrack' : 'midiEdSaveNew')}
              </button>
              <button type="button" onClick={() => void run('cover')} disabled={!ready || busy !== null} title={say('midiEdCoverHint')} className={MAIN}>
                {busy === 'cover' ? <Loader2 size={13} className="animate-spin" /> : <AudioLines size={13} />}
                {say('midiEdCover')}
              </button>
              <button type="button" onClick={close} className="ml-1 text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200" aria-label={say('midiEdClose')} title={say('midiEdClose')}>
                <X size={18} />
              </button>
            </div>
          </div>
          <div className="relative min-h-0 flex-1">
            <iframe ref={frame} src={page} title={say('midiEdTitle')} allow="midi; autoplay" className="absolute inset-0 h-full w-full border-0" />
            {!ready && (
              <div className="absolute inset-0 flex items-center justify-center gap-2 bg-white text-sm text-zinc-500 dark:bg-zinc-900">
                <Loader2 size={16} className="animate-spin text-pink-500" /> {say('midiEdLoading')}
              </div>
            )}
          </div>
        </div>
      </div>
      <ConfirmDialog
        isOpen={asking}
        title={say('midiEdCloseTitle')}
        message={say('midiEdCloseMessage')}
        confirmLabel={say('midiEdCloseOk')}
        cancelLabel={say('midiEdKeepEditing')}
        onConfirm={() => { setAsking(false); onClose(); }}
        onCancel={() => setAsking(false)}
      />
    </>,
    document.body,
  );
};

import './src-styles.css';
import React, { useCallback, useEffect, useState } from 'react';
import ReactDOM from 'react-dom/client';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { I18nProvider, useI18n } from './context/I18nContext';
import { receiveVisualizerFeed, type ReceivedFeed } from './services/visualizerFeed';
import { VisualizerView } from './components/player/VisualizerView';
import { EngineTabs, VisualizerControls } from './components/player/VisualizerPanel';
import { X } from 'lucide-react';
import { onVisualizer, setVisualizer, visualizer } from './services/visualizerState';

/**
 * The visualiser in a window of its own. It hears the studio through the feed
 * the studio window sends; F or a double click is fullscreen, Esc leaves it
 * or closes the window. Where it is and fullscreen are the shared state, so
 * the studio and the agent move it too.
 */

const Window: React.FC = () => {
  const { t } = useI18n();
  const [feed, setFeed] = useState<ReceivedFeed | null>(null);
  const [waiting, setWaiting] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [bar, setBar] = useState(true);
  const [view, setView] = useState(visualizer);
  useEffect(() => onVisualizer(setView), []);
  const name = view.engine === 'milkdrop' ? view.preset ?? '' : t(`vizLook_${view.look}` as never);

  // the page's own menu (save image, inspect) has no place over the picture
  useEffect(() => {
    const block = (event: MouseEvent) => event.preventDefault();
    window.addEventListener('contextmenu', block);
    return () => window.removeEventListener('contextmenu', block);
  }, []);

  useEffect(() => {
    let opened: ReceivedFeed | null = null;
    receiveVisualizerFeed()
      .then((ready) => {
        opened = ready;
        setFeed(ready);
        setWaiting(ready.context.state === 'suspended');
      })
      .catch((error) => setProblem(error instanceof Error ? error.message : String(error)));
    return () => opened?.close();
  }, []);

  // the window follows the shared state: fullscreen, and closing when it is put elsewhere
  useEffect(() => {
    const current = getCurrentWindow();
    let shown = visualizer().place === 'window';
    const unsubscribe = onVisualizer((state) => {
      if (state.place === 'window') shown = true;
      else if (shown) void current.close();
      void current.isFullscreen().then((now) => {
        if (now !== state.fullscreen) void current.setFullscreen(state.fullscreen);
      });
    });
    const bye = () => {
      if (visualizer().place === 'window') setVisualizer({ place: 'closed', fullscreen: false });
    };
    window.addEventListener('beforeunload', bye);
    return () => {
      unsubscribe();
      window.removeEventListener('beforeunload', bye);
    };
  }, []);

  // the bar hides while the picture plays and comes back with the mouse
  useEffect(() => {
    let timer = window.setTimeout(() => setBar(false), 2500);
    const show = () => {
      setBar(true);
      window.clearTimeout(timer);
      timer = window.setTimeout(() => setBar(false), 2500);
    };
    window.addEventListener('pointermove', show);
    return () => {
      window.clearTimeout(timer);
      window.removeEventListener('pointermove', show);
    };
  }, []);

  const fullscreen = useCallback(() => setVisualizer({ fullscreen: !visualizer().fullscreen }), []);

  const escape = useCallback(() => {
    if (visualizer().fullscreen) setVisualizer({ fullscreen: false });
    else setVisualizer({ place: 'closed' });
  }, []);

  if (problem) return <div className="grid h-full place-items-center p-6 text-center text-sm text-rose-300">{t('vizFailed')}: {problem}</div>;
  if (!feed) return <div className="grid h-full place-items-center text-sm text-zinc-400">{t('vizLoading')}</div>;

  return (
    <div className="relative h-full w-full">
      <VisualizerView context={feed.context} source={feed.source} onFullscreen={fullscreen} onEscape={escape} />
      <div className={`absolute left-1/2 top-3 z-20 w-[min(760px,calc(100%-24px))] -translate-x-1/2 transition-opacity ${bar ? 'opacity-100' : 'pointer-events-none opacity-0'}`}>
        <div className="flex items-center gap-2 rounded-lg bg-black/70 px-2 py-1 backdrop-blur">
          <EngineTabs />
          <span className="min-w-0 flex-1 truncate text-center text-[11px] text-zinc-200" title={name}>{name}</span>
          <VisualizerControls onFullscreen={fullscreen} tone="dark" />
          <button type="button" onClick={escape} title={t('close')} className="rounded p-1 text-zinc-300 hover:bg-white/10 hover:text-white">
            <X size={15} />
          </button>
        </div>
      </div>
      {waiting && (
        <button
          type="button"
          onClick={() => void feed.context.resume().then(() => setWaiting(false))}
          className="absolute inset-0 z-30 grid place-items-center bg-black/70 text-sm text-white"
        >
          {t('vizClickToStart')}
        </button>
      )}
    </div>
  );
};

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <I18nProvider>
      <Window />
    </I18nProvider>
  </React.StrictMode>,
);

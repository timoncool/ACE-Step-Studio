import React, { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { AudioLines, Disc3, ExternalLink, Plus, SlidersVertical } from 'lucide-react';
import { useI18n } from '../../context/I18nContext';
import { equalizer, onEqualizer } from '../../services/audioGraph';
import { equalizerPanelOpen, onEqualizerPanel, setEqualizerPanelOpen } from '../../services/playerPanels';
import { onVisualizer, setVisualizer, visualizer } from '../../services/visualizerState';
import { addWinampSkin, onWinamp, SCALE_RANGE, setWinampOn, setWinampSettings, SKIN_MUSEUM, winampSettings, winampSkins, type WinampSkin } from '../../services/winamp';
import { openExternal } from '../../services/externalLinks';

/**
 * The player bar's buttons for the extras: the equalizer, the visualiser, and
 * the Winamp mode with its skins - which one, random, add a .wsz, the museum -
 * its scale, and whether the button asks first or a long press does. The
 * player bar carries them, and the sidebar too, for when no song is loaded
 * and the bar is not there.
 */

/** bar: the player bar's icons; rail: the collapsed sidebar; list: the open sidebar, with names. */
export type ExtrasLayout = 'bar' | 'rail' | 'list';

const HOLD_MS = 500;

const ON = 'text-pink-600 dark:text-pink-500';
const LOOK: Record<ExtrasLayout, string> = {
  bar: 'p-1.5 lg:p-2 rounded-full transition-colors hover:bg-zinc-100 dark:hover:bg-white/10',
  rail: 'flex aspect-square w-full items-center justify-center rounded-xl text-zinc-500 transition-colors hover:bg-zinc-100 hover:text-black dark:text-zinc-400 dark:hover:bg-white/5 dark:hover:text-white',
  list: 'flex w-full items-center gap-3 rounded-xl px-3 py-2 text-zinc-500 transition-colors hover:bg-zinc-100 hover:text-black dark:text-zinc-400 dark:hover:bg-white/5 dark:hover:text-white',
};

export const PlayerExtraButtons: React.FC<{ layout?: ExtrasLayout }> = ({ layout = 'bar' }) => {
  const { t } = useI18n();
  const [eqOpen, setEqOpen] = useState(equalizerPanelOpen);
  const [eqOn, setEqOn] = useState(() => equalizer().enabled);
  const [vizOpen, setVizOpen] = useState(() => visualizer().place !== 'closed');
  const [menu, setMenu] = useState(false);
  const [skins, setSkins] = useState<WinampSkin[]>([]);
  const [settings, setSettings] = useState(winampSettings);
  const [problem, setProblem] = useState<string | null>(null);
  const picker = useRef<HTMLInputElement>(null);
  const box = useRef<HTMLDivElement>(null);
  const opener = useRef<HTMLButtonElement>(null);
  const popover = useRef<HTMLDivElement>(null);
  const [at, setAt] = useState<React.CSSProperties>({});
  const hold = useRef<{ timer: number; opened: boolean } | null>(null);

  useEffect(() => onEqualizerPanel(setEqOpen), []);
  useEffect(() => onEqualizer((state) => setEqOn(state.enabled)), []);
  useEffect(() => onVisualizer((state) => setVizOpen(state.place !== 'closed')), []);
  useEffect(() => onWinamp(() => setSettings(winampSettings())), []);

  useEffect(() => {
    if (!menu) return undefined;
    setProblem(null);
    winampSkins().then(setSkins).catch((error) => setProblem(error instanceof Error ? error.message : String(error)));
    // the menu lives over everything, next to its button: the sidebar would clip it
    const place = opener.current?.getBoundingClientRect();
    if (place) {
      setAt(layout === 'bar'
        ? { right: Math.max(8, window.innerWidth - place.right), bottom: window.innerHeight - place.top + 8 }
        : { left: place.right + 8, bottom: Math.max(8, window.innerHeight - place.bottom) });
    }
    const close = (event: MouseEvent) => {
      const target = event.target as Node;
      if (!box.current?.contains(target) && !popover.current?.contains(target)) setMenu(false);
    };
    document.addEventListener('mousedown', close);
    return () => document.removeEventListener('mousedown', close);
  }, [menu, layout]);

  const add = async (file: File) => {
    setProblem(null);
    try {
      const added = await addWinampSkin(file, file.name.replace(/\.(wsz|zip)$/i, ''));
      setWinampSettings({ skin: added.name, random: false });
      setSkins(await winampSkins());
    } catch (error) {
      setProblem(`${t('winampSkinNotKept')}: ${error instanceof Error ? error.message : String(error)}`);
    }
  };

  return (
    <>
      <button type="button" onClick={() => setEqualizerPanelOpen(!eqOpen)} title={t('eqTitle')} className={`${LOOK[layout]} ${layout === 'bar' ? 'hidden md:block' : ''} ${eqOpen || eqOn ? ON : ''}`}>
        <SlidersVertical size={layout === 'bar' ? 17 : 20} className="shrink-0" />
        {layout === 'list' && <span className="truncate text-sm font-medium">{t('eqTitle')}</span>}
      </button>
      <button
        type="button"
        onClick={() => setVisualizer({ place: visualizer().place === 'closed' ? 'panel' : 'closed', fullscreen: false })}
        title={t('vizTitle')}
        className={`${LOOK[layout]} ${layout === 'bar' ? 'hidden md:block' : ''} ${vizOpen ? ON : ''}`}
      >
        <AudioLines size={layout === 'bar' ? 17 : 20} className="shrink-0" />
        {layout === 'list' && <span className="truncate text-sm font-medium">{t('vizTitle')}</span>}
      </button>
      <div ref={box} className={layout === 'bar' ? 'relative hidden md:block' : 'w-full'}>
        <button
          ref={opener}
          type="button"
          onPointerDown={() => {
            // a long press opens the settings even when the button starts Winamp at once
            const timer = window.setTimeout(() => {
              if (hold.current) hold.current.opened = true;
              setMenu(true);
            }, HOLD_MS);
            hold.current = { timer, opened: false };
          }}
          onPointerLeave={() => hold.current && window.clearTimeout(hold.current.timer)}
          onClick={() => {
            const held = hold.current;
            if (held) window.clearTimeout(held.timer);
            hold.current = null;
            if (held?.opened) return;
            if (settings.skipMenu && !menu) setWinampOn(true);
            else setMenu(!menu);
          }}
          onContextMenu={(event) => {
            event.preventDefault();
            setMenu(true);
          }}
          title={settings.skipMenu ? t('winampTitleHold') : t('winampTitle')}
          className={`${LOOK[layout]} ${menu ? ON : ''}`}
        >
          <Disc3 size={layout === 'bar' ? 17 : 20} className="shrink-0" />
          {layout === 'list' && <span className="truncate text-sm font-medium">{t('winampTitle')}</span>}
        </button>
        {menu && createPortal(
          <div ref={popover} style={at} className="fixed z-[58] w-64 rounded-xl border border-zinc-200 bg-white p-2 shadow-xl dark:border-white/10 dark:bg-zinc-800">
            <button
              type="button"
              onClick={() => {
                setMenu(false);
                setWinampOn(true);
              }}
              className="w-full rounded-lg bg-pink-500 px-3 py-2 text-sm font-semibold text-white hover:bg-pink-600"
            >
              {t('winampStart')}
            </button>
            <label className="mt-2 flex items-center justify-between text-[10px] font-semibold uppercase tracking-wide text-zinc-500">
              <span>{t('winampScale')}</span>
              <span className="font-mono normal-case text-zinc-700 dark:text-zinc-200">{Math.round(settings.scale * 100)}%</span>
            </label>
            <input
              type="range"
              min={SCALE_RANGE.min}
              max={SCALE_RANGE.max}
              step={SCALE_RANGE.step}
              value={settings.scale}
              onChange={(event) => setWinampSettings({ scale: Number(event.target.value) })}
              onDoubleClick={() => setWinampSettings({ scale: 1 })}
              className="mt-1 w-full cursor-pointer accent-pink-500"
            />
            <label className="mt-2 block text-[10px] font-semibold uppercase tracking-wide text-zinc-500">{t('winampSkin')}</label>
            <select
              value={settings.random ? '' : settings.skin ?? ''}
              onChange={(event) => setWinampSettings(event.target.value ? { skin: event.target.value, random: false } : { random: true })}
              className="mt-1 w-full rounded-lg border border-zinc-200 bg-white px-2 py-1.5 text-xs text-zinc-800 dark:border-white/10 dark:bg-black/30 dark:text-zinc-100"
            >
              <option value="">{t('winampRandomSkin')}</option>
              {skins.map((skin) => <option key={skin.url} value={skin.name}>{skin.name}</option>)}
            </select>
            <div className="mt-2 grid grid-cols-2 gap-1.5">
              <button type="button" onClick={() => picker.current?.click()} className="flex items-center justify-center gap-1 rounded-lg border border-zinc-200 px-2 py-1.5 text-xs text-zinc-600 hover:border-pink-400 hover:text-pink-600 dark:border-white/10 dark:text-zinc-300">
                <Plus size={13} />{t('winampAddSkin')}
              </button>
              <button type="button" onClick={() => void openExternal(SKIN_MUSEUM)} className="flex items-center justify-center gap-1 rounded-lg border border-zinc-200 px-2 py-1.5 text-xs text-zinc-600 hover:border-pink-400 hover:text-pink-600 dark:border-white/10 dark:text-zinc-300">
                <ExternalLink size={13} />{t('winampMuseum')}
              </button>
            </div>
            <label className="mt-2 flex cursor-pointer items-start gap-2 text-xs text-zinc-700 dark:text-zinc-200">
              <input type="checkbox" checked={settings.skipMenu} onChange={(event) => setWinampSettings({ skipMenu: event.target.checked })} className="mt-0.5 accent-pink-500" />
              <span>{t('winampSkipMenu')}</span>
            </label>
            <p className="mt-2 text-[10px] leading-snug text-zinc-500">{t('winampSkinsHint')}</p>
            {problem && <p className="mt-1 text-[11px] text-rose-600 dark:text-rose-300">{problem}</p>}
            <input
              ref={picker}
              type="file"
              accept=".wsz,.zip"
              className="hidden"
              onChange={(event) => {
                const file = event.target.files?.[0];
                event.target.value = '';
                if (file) void add(file);
              }}
            />
          </div>,
          document.body,
        )}
      </div>
    </>
  );
};

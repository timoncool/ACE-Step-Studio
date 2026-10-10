import React, { useEffect, useState } from 'react';
import { AlertTriangle, Download, Loader2 } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { openExternal } from '../services/externalLinks';

type Invoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>;
type Listen = (event: string, handler: (event: { payload: unknown }) => void) => Promise<() => void>;
type UpdateOffer = { version: string; portable: boolean; page: string };

function shell(): { invoke: Invoke; listen: Listen } | null {
  const tauri = (window as unknown as { __TAURI__?: { core?: { invoke?: Invoke }; event?: { listen?: Listen } } }).__TAURI__;
  const invoke = tauri?.core?.invoke;
  const listen = tauri?.event?.listen;
  return invoke && listen ? { invoke, listen } : null;
}

const BUTTON = 'px-4 py-2 text-sm font-medium rounded-lg transition-colors disabled:opacity-60';
const SECONDARY = `${BUTTON} text-zinc-700 dark:text-zinc-300 bg-zinc-100 dark:bg-zinc-800 hover:bg-zinc-200 dark:hover:bg-zinc-700`;

const Frame: React.FC<{ icon: React.ReactNode; title: string; children: React.ReactNode }> = ({ icon, title, children }) => (
  <div className="fixed inset-0 z-100 flex items-center justify-center bg-black/60 backdrop-blur-xs animate-in fade-in duration-150">
    <div role="dialog" aria-modal="true" aria-label={title}
         className="bg-white dark:bg-zinc-900 rounded-2xl shadow-2xl border border-zinc-200 dark:border-white/10 w-full max-w-sm mx-4 p-6 animate-in zoom-in-95 fade-in duration-200">
      <div className="flex items-start gap-3">
        <div className="shrink-0 w-10 h-10 rounded-full flex items-center justify-center bg-zinc-100 dark:bg-white/5">{icon}</div>
        <div className="flex-1 min-w-0">
          <h3 className="text-base font-semibold text-zinc-900 dark:text-white">{title}</h3>
          {children}
        </div>
      </div>
    </div>
  </div>
);

/**
 * What the desktop shell asks the person, drawn by the window in the studio's own language: quitting while a song
 * is made, and an update that is out. In a plain browser there is no shell and nothing is drawn.
 */
export const ShellPrompts: React.FC = () => {
  const { t } = useI18n();
  const [quitAsked, setQuitAsked] = useState(false);
  const [update, setUpdate] = useState<UpdateOffer | null>(null);
  const [installing, setInstalling] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    const bridge = shell();
    if (!bridge) return;
    const unlisten: Array<Promise<() => void>> = [
      bridge.listen('studio://quit-asked', () => setQuitAsked(true)),
      bridge.listen('studio://update-available', (event) => setUpdate(event.payload as UpdateOffer)),
    ];
    bridge.invoke('pending_update')
      .then((offer) => { if (offer) setUpdate(offer as UpdateOffer); })
      .catch((error: unknown) => console.error('[ERROR] asking the shell for a found update:', error));
    return () => { for (const off of unlisten) void off.then((stop) => stop()); };
  }, []);

  useEffect(() => {
    if (quitAsked) void shell()?.invoke('quit_prompt_shown');
  }, [quitAsked]);

  const stayOpen = () => {
    setQuitAsked(false);
    void shell()?.invoke('quit_prompt_cancelled');
  };

  const install = async () => {
    const bridge = shell();
    if (!bridge || !update) return;
    if (update.portable) {
      void openExternal(update.page);
      setUpdate(null);
      return;
    }
    setInstalling(true);
    setFailure(null);
    try {
      await bridge.invoke('install_update');
    } catch (error) {
      setFailure(String(error));
      setInstalling(false);
    }
  };

  if (quitAsked) {
    return (
      <Frame icon={<AlertTriangle size={20} className="text-red-500" />} title={t('shellQuitTitle')}>
        <p className="mt-1 text-sm text-zinc-500 dark:text-zinc-400 leading-relaxed">{t('shellQuitMessage')}</p>
        <div className="flex gap-3 justify-end mt-6">
          <button type="button" onClick={stayOpen} className={SECONDARY}>{t('cancel')}</button>
          <button type="button" onClick={() => void shell()?.invoke('quit_studio')} className={`${BUTTON} bg-red-600 hover:bg-red-700 text-white`}>
            {t('shellQuitConfirm')}
          </button>
        </div>
      </Frame>
    );
  }

  if (update) {
    return (
      <Frame icon={<Download size={20} className="text-pink-500" />} title={t('shellUpdateTitle')}>
        <p className="mt-1 text-sm text-zinc-500 dark:text-zinc-400 leading-relaxed">
          {t(update.portable ? 'shellUpdatePortable' : 'shellUpdateMessage').replace('{version}', update.version)}
        </p>
        {failure && <p className="mt-2 text-sm text-red-600 dark:text-red-400">{t('shellUpdateFailed').replace('{error}', failure)}</p>}
        <div className="flex gap-3 justify-end mt-6">
          <button type="button" onClick={() => setUpdate(null)} disabled={installing} className={SECONDARY}>{t('shellUpdateLater')}</button>
          <button type="button" onClick={() => void install()} disabled={installing}
                  className={`${BUTTON} bg-pink-600 hover:bg-pink-700 text-white inline-flex items-center gap-2`}>
            {installing && <Loader2 size={14} className="animate-spin" />}
            {installing ? t('shellUpdateInstalling') : t(update.portable ? 'shellUpdateOpen' : 'shellUpdateInstall')}
          </button>
        </div>
      </Frame>
    );
  }

  return null;
};

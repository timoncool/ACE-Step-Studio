import React, { useEffect } from 'react';
import { X } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { openExternal } from '../services/externalLinks';
import { HUB_GRADIENTS, HUB_TEXT, hubMediaUrl, hubText, reportNotice, stackTheme, type HubButton, type HubItem } from '../services/studioHub';
import { HubMarkdown } from './HubMarkdown';

interface Props {
  item: HubItem;
  /** Closed by the cross, Esc or a click outside: recorded as closed. */
  onClose: () => void;
  onOpen: (target: NonNullable<HubButton['target']>) => void;
}

/** A hub popup in the studio's own dialog style: shown once, closed by Esc, the cross or a button. */
export const HubPopup: React.FC<Props> = ({ item, onClose, onOpen }) => {
  const { language, t } = useI18n();
  const content = hubText(item, language);
  const theme = stackTheme(item.theme, 0);

  useEffect(() => {
    reportNotice(item.id, 'shown').catch((error) => console.warn('[hub] popup shown not recorded:', error));
  }, [item.id]);

  const dismiss = React.useCallback(() => {
    reportNotice(item.id, 'dismissed').catch((error) => console.warn('[hub] popup close not recorded:', error));
    onClose();
  }, [item.id, onClose]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && item.dismissible) dismiss();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [item.dismissible, dismiss]);

  if (!content) return null;

  const press = (button: HubButton, index: number) => {
    reportNotice(item.id, button.action === 'dismiss' ? 'dismissed' : 'clicked', button.action === 'dismiss' ? undefined : `b${index}`)
      .catch((error) => console.warn('[hub] popup button not recorded:', error));
    if (button.action === 'url' && button.url) void openExternal(button.url);
    if (button.action === 'open' && button.target) onOpen(button.target);
    onClose();
  };

  return (
    <div className="fixed inset-0 z-[90] flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
         onClick={(event) => { if (event.target === event.currentTarget && item.dismissible) dismiss(); }}>
      <div role="dialog" aria-modal="true" aria-labelledby={`hub-${item.id}`}
           className="w-full max-w-md overflow-hidden rounded-2xl border border-zinc-200 bg-white shadow-2xl dark:border-white/10 dark:bg-suno-panel">
        <div className="flex items-center justify-between gap-3 px-5 py-4" style={{ background: HUB_GRADIENTS[theme], color: HUB_TEXT[theme] }}>
          <h2 id={`hub-${item.id}`} className="text-lg font-semibold leading-snug">{content.title}</h2>
          {item.dismissible && (
            <button type="button" onClick={dismiss} aria-label={t('hubClose')} className="rounded-md p-1 opacity-85 hover:opacity-100">
              <X size={18} />
            </button>
          )}
        </div>
        {item.image && <img src={hubMediaUrl(item.image)} alt="" className="max-h-56 w-full object-cover" />}
        <div onClickCapture={(event) => {
          if ((event.target as HTMLElement).closest('a')) reportNotice(item.id, 'clicked', 'link').catch((error) => console.warn('[hub] popup link not recorded:', error));
        }}>
          <HubMarkdown text={content.body} className="space-y-2 px-5 py-4 text-sm leading-relaxed text-zinc-700 dark:text-zinc-300 [&_a]:text-pink-600 dark:[&_a]:text-pink-400" />
        </div>
        {item.ad?.label && <p className="px-5 pb-2 text-xs text-zinc-400">{t('hubAd')}{item.ad.erid ? ` · erid ${item.ad.erid}` : ''}</p>}
        {content.buttons.length > 0 && (
          <div className="flex justify-end gap-2 px-5 pb-5">
            {content.buttons.map((button, index) => (
              <button key={index} type="button" onClick={() => press(button, index)}
                      className={button.style === 'primary'
                        ? 'rounded-xl px-4 py-2 text-sm font-semibold shadow-sm transition-opacity hover:opacity-90'
                        : 'rounded-xl bg-zinc-100 px-4 py-2 text-sm font-medium text-zinc-700 transition-colors hover:bg-zinc-200 dark:bg-white/10 dark:text-zinc-200 dark:hover:bg-white/15'}
                      style={button.style === 'primary' ? { background: HUB_GRADIENTS[theme], color: HUB_TEXT[theme] } : undefined}>
                {button.label}
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
};

import React, { useEffect } from 'react';
import { X } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { HUB_GRADIENTS, HUB_TEXT, MAX_VISIBLE_BARS, hubText, reportNotice, showsNow, stackTheme, type HubItem } from '../services/studioHub';
import { hubInline } from './HubMarkdown';

interface Props {
  items: HubItem[];
  /** Seconds since the window became ready, for the notices' delays. */
  elapsed: number;
  /** The section on screen: create, library, news, tools. */
  view: string;
  /** Closed in this launch: hidden at once, before the service's state catches up. */
  closed: Set<string>;
  onClosed: (id: string) => void;
}

/**
 * The strip stack across the top of the window, as on ArtGen: full width, no gaps, centred text with links, a close
 * button on the right; at most three, the colour by place in the stack unless the notice names one.
 */
export const HubBars: React.FC<Props> = ({ items, elapsed, view, closed, onClosed }) => {
  const { language, t } = useI18n();
  const visible = items
    .filter((item) => item.kind === 'bar' && !closed.has(item.id) && showsNow(item, elapsed, view))
    .slice(0, MAX_VISIBLE_BARS);

  useEffect(() => {
    for (const item of visible) {
      reportNotice(item.id, 'shown').catch((error) => console.warn('[hub] shown not recorded:', error));
    }
  }, [visible.map((item) => item.id).join(',')]);

  if (!visible.length) return null;

  const close = (id: string) => {
    reportNotice(id, 'dismissed').catch((error) => console.warn('[hub] close not recorded:', error));
    onClosed(id);
  };

  return (
    <div role="region" aria-label={t('hubNoticesRegion')} className="shrink-0">
      {visible.map((item, index) => {
        const theme = stackTheme(item.theme, index);
        const content = hubText(item, language);
        return (
          <div key={item.id}
               className="flex items-start justify-between gap-3 px-6 py-2.5 text-sm leading-6 max-md:px-3 max-md:py-1.5 max-md:text-xs"
               style={{ background: HUB_GRADIENTS[theme], color: HUB_TEXT[theme] }}
               onClick={(event) => {
                 if ((event.target as HTMLElement).closest('a')) reportNotice(item.id, 'clicked', 'link').catch(() => undefined);
               }}>
            <div className="w-full text-center">
              {hubInline((content?.body || content?.title || '').replace(/\s*\n\s*/g, ' '), item.id)}
              {item.ad?.label && <span className="ml-2 text-xs opacity-75">{t('hubAd')}{item.ad.erid ? ` · erid ${item.ad.erid}` : ''}</span>}
            </div>
            {item.dismissible && (
              <button type="button" onClick={() => close(item.id)} aria-label={t('hubClose')}
                      className="shrink-0 rounded-md p-0.5 opacity-85 transition-opacity hover:opacity-100">
                <X size={18} />
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
};

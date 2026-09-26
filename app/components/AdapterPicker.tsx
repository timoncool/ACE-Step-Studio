import React, { useEffect, useRef, useState } from 'react';
import { AlertTriangle, Layers, Plus, X } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { AdapterUse, InstalledAdapter, familyLabel, localized, startingScales, useAdapterLibrary } from '../services/adapters';

/**
 * The LoRA card of the create form: the adapters this song uses, one strength
 * per part of the model each one touches. The parts come from the engine; the
 * card names them by what they change.
 */

interface AdapterPickerProps {
  value: AdapterUse[];
  onChange: (next: AdapterUse[]) => void;
  /** A picked adapter's trigger word goes into the style, a removed one comes out. */
  onTrigger: (word: string, present: boolean) => void;
  /** Renders the card frame the rest of the form uses. */
  frame: (title: string, icon: React.ReactNode, actions: React.ReactNode, body: React.ReactNode) => React.ReactElement;
  iconClass: string;
}

export const AdapterPicker: React.FC<AdapterPickerProps> = ({ value, onChange, onTrigger, frame, iconClass }) => {
  const { t, language } = useI18n();
  const tt = t as unknown as (key: string) => string;
  const { installed, slots, model } = useAdapterLibrary();
  /** An adapter made for the other size of the model, which the engine cannot merge. */
  const otherSize = (adapter?: InstalledAdapter) => Boolean(adapter?.model && adapter.model !== 'lm' && model && adapter.model !== model);
  const [choosing, setChoosing] = useState(false);
  const menu = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!choosing) return;
    const close = (event: MouseEvent) => {
      if (menu.current && !menu.current.contains(event.target as Node)) setChoosing(false);
    };
    document.addEventListener('mousedown', close);
    return () => document.removeEventListener('mousedown', close);
  }, [choosing]);

  const roleOf = (slot: string) => slots.find(entry => entry.id === slot)?.role ?? '';
  const openPage = () => window.dispatchEvent(new CustomEvent('studio:navigate', { detail: 'adapters' }));

  const add = (adapter: InstalledAdapter) => {
    setChoosing(false);
    onChange([...value, { id: adapter.id, scales: startingScales(adapter, slots) }]);
    if (adapter.trigger) onTrigger(adapter.trigger, true);
  };

  const remove = (id: string) => {
    onChange(value.filter(use => use.id !== id));
    const trigger = installed.find(adapter => adapter.id === id)?.trigger;
    if (trigger && !value.some(use => use.id !== id && installed.find(adapter => adapter.id === use.id)?.trigger === trigger)) {
      onTrigger(trigger, false);
    }
  };

  const setScale = (id: string, slot: string, scale: number) =>
    onChange(value.map(use => (use.id === id ? { ...use, scales: { ...use.scales, [slot]: scale } } : use)));

  const available = installed.filter(adapter => !adapter.error && !value.some(use => use.id === adapter.id));

  const actions = (
    <button type="button" onClick={openPage} className={iconClass} title={t('adaptersOpenPage')}>
      <Layers size={14} />
    </button>
  );

  const body = (
    <div className="space-y-3">
      {value.map(use => {
        const adapter = installed.find(entry => entry.id === use.id);
        const touched = adapter ? (adapter.slots.length ? adapter.slots : Object.keys(use.scales)) : Object.keys(use.scales);
        const range = adapter?.range ?? [0, 1.5];
        return (
          <div key={use.id} className="rounded-lg border border-zinc-200 p-2.5 dark:border-white/10">
            <div className="flex items-center justify-between gap-2">
              <span className="min-w-0 truncate text-sm font-semibold text-zinc-900 dark:text-white">
                {adapter ? localized(adapter.name, language) : use.id}
              </span>
              <button type="button" onClick={() => remove(use.id)} className={iconClass} title={t('adaptersRemoveFromSong')}>
                <X size={13} />
              </button>
            </div>
            {!adapter && (
              <p className="mt-1 flex items-center gap-1 text-[11px] text-amber-600 dark:text-amber-300">
                <AlertTriangle size={12} />
                {t('adaptersMissing')}
              </p>
            )}
            {adapter?.model && otherSize(adapter) && model && (
              <p className="mt-1 flex items-start gap-1 text-[11px] leading-4 text-amber-600 dark:text-amber-300">
                <AlertTriangle size={12} className="mt-0.5 shrink-0" />
                {t('adaptersFamilyOther').replace('{model}', familyLabel(adapter.model)).replace('{current}', familyLabel(model))}
              </p>
            )}
            {touched.map(slot => {
              const scale = use.scales[slot] ?? 0;
              return (
                <div key={slot} className="mt-2">
                  <div className="flex items-baseline justify-between gap-2">
                    <span className="text-[11px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400" title={tt(`adapterRoleHint_${roleOf(slot)}`)}>
                      {roleOf(slot) ? tt(`adapterRole_${roleOf(slot)}`) : slot}
                    </span>
                    <span className="text-[11px] tabular-nums text-zinc-600 dark:text-zinc-300">{scale.toFixed(2)}</span>
                  </div>
                  <input
                    type="range"
                    min={range[0]}
                    max={range[1]}
                    step={0.05}
                    value={scale}
                    aria-label={`${adapter ? localized(adapter.name, language) : use.id} · ${roleOf(slot) ? tt(`adapterRole_${roleOf(slot)}`) : slot}`}
                    onChange={event => setScale(use.id, slot, Number(event.target.value))}
                    className="mt-1.5 h-1 w-full cursor-pointer accent-pink-500"
                  />
                </div>
              );
            })}
          </div>
        );
      })}

      {installed.length === 0 ? (
        <div className="flex flex-wrap items-center justify-between gap-2">
          <span className="text-[11px] text-zinc-500">{t('adaptersNoneInstalled')}</span>
          <button type="button" onClick={openPage} className="text-xs font-semibold text-pink-600 hover:underline dark:text-pink-400">
            {t('adaptersOpenPage')}
          </button>
        </div>
      ) : (
        available.length > 0 && (
          <div ref={menu} className="relative">
            <button
              type="button"
              onClick={() => setChoosing(open => !open)}
              aria-expanded={choosing}
              className="inline-flex w-full items-center justify-center gap-1.5 rounded-lg border border-dashed border-zinc-300 py-2 text-xs font-semibold text-zinc-600 hover:border-pink-400 hover:text-pink-600 dark:border-white/15 dark:text-zinc-300"
            >
              <Plus size={13} />
              {t('adaptersAdd')}
            </button>
            {choosing && (
              <div className="mt-1 max-h-72 overflow-y-auto rounded-lg border border-zinc-200 bg-white dark:border-white/10 dark:bg-zinc-900">
                {available.map(adapter => (
                  <button
                    key={adapter.id}
                    type="button"
                    onClick={() => add(adapter)}
                    className="flex w-full items-center justify-between gap-2 border-b border-zinc-100 px-3 py-2 text-left last:border-b-0 hover:bg-zinc-100 dark:border-white/5 dark:hover:bg-white/5"
                  >
                    <span className="min-w-0 truncate text-sm text-zinc-800 dark:text-zinc-200">{localized(adapter.name, language)}</span>
                    <span className="flex shrink-0 items-center gap-1.5">
                      {adapter.model && (
                        <span className={`text-[10px] font-bold ${otherSize(adapter) ? 'text-amber-600 dark:text-amber-300' : 'text-zinc-400'}`}>{familyLabel(adapter.model)}</span>
                      )}
                      <span className="text-[10px] font-bold uppercase tracking-wide text-pink-600 dark:text-pink-300">
                        {tt(`adapterKind_${adapter.kind}`) || adapter.kind}
                      </span>
                    </span>
                  </button>
                ))}
              </div>
            )}
          </div>
        )
      )}
    </div>
  );

  return frame(t('adaptersCardTitle'), <Layers size={13} />, actions, body);
};

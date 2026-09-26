import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Download, Move, RotateCcw, Save, Upload, X } from 'lucide-react';
import { useI18n } from '../../context/I18nContext';
import { EQ_BANDS, EQ_RANGE_DB, equalizer, equalizerResponse, onEqualizer, setEqualizer, type EqualizerState } from '../../services/audioGraph';
import { BAND_LABELS, OWN_CHANGED, SITUATION_PRESETS, WINAMP_PRESETS, ownPresets, readEqf, storeOwnPresets, writeEqf, type EqPreset } from '../../services/equalizerPresets';
import { saveFile } from '../../services/saveFile';
import { useFloatable } from '../../services/useFloatable';

/**
 * The ten-band equalizer, as a panel over the studio: Winamp's bands and
 * preamp, the response curve the filters really play, Winamp's presets, ones
 * for common situations and the user's own, .EQF files in and out, and the
 * balance and mono that apply with the equalizer off too.
 */

const CURVE_W = 300;
const CURVE_H = 72;
const FREQUENCIES = (() => {
  const points = 120;
  const list = new Float32Array(points);
  for (let i = 0; i < points; i += 1) list[i] = 20 * Math.pow(1000, i / (points - 1));
  return list;
})();

const Slider: React.FC<{ value: number; label: string; title: string; onChange: (value: number) => void; disabled: boolean }> = ({ value, label, title, onChange, disabled }) => (
  <label className="flex flex-col items-center gap-1" title={title}>
    <span className="text-[9px] tabular-nums text-zinc-400">{value > 0 ? `+${value.toFixed(0)}` : value.toFixed(0)}</span>
    <input
      type="range"
      min={-EQ_RANGE_DB}
      max={EQ_RANGE_DB}
      step={0.5}
      value={value}
      onChange={(event) => onChange(Number(event.target.value))}
      onDoubleClick={() => onChange(0)}
      disabled={disabled}
      className="h-28 w-4 cursor-pointer accent-pink-500 disabled:opacity-40 [direction:rtl] [writing-mode:vertical-lr]"
    />
    <span className="text-[9px] font-semibold text-zinc-500">{label}</span>
  </label>
);

export const EqualizerPanel: React.FC<{ onClose: () => void }> = ({ onClose }) => {
  const { t } = useI18n();
  const [eq, setEq] = useState<EqualizerState>(equalizer);
  const [own, setOwn] = useState(ownPresets);
  const [problem, setProblem] = useState<string | null>(null);
  const [naming, setNaming] = useState<string | null>(null);
  const importer = useRef<HTMLInputElement>(null);
  const panel = useFloatable('equalizer', { x: Math.max(8, window.innerWidth / 2 - 190), y: window.innerHeight - 420 });

  useEffect(() => onEqualizer(setEq), []);
  useEffect(() => {
    const reread = () => setOwn(ownPresets());
    window.addEventListener(OWN_CHANGED, reread);
    return () => window.removeEventListener(OWN_CHANGED, reread);
  }, []);

  const curve = useMemo(() => {
    const response = equalizerResponse(FREQUENCIES, eq);
    const y = (db: number) => CURVE_H / 2 - (Math.max(-EQ_RANGE_DB * 1.5, Math.min(EQ_RANGE_DB * 1.5, db)) / (EQ_RANGE_DB * 1.5)) * (CURVE_H / 2 - 4);
    return Array.from(response, (db, index) => `${index === 0 ? 'M' : 'L'}${((index / (response.length - 1)) * CURVE_W).toFixed(1)},${y(db).toFixed(1)}`).join(' ');
  }, [eq]);

  const band = (index: number, value: number) => {
    const gains = [...eq.gains];
    gains[index] = value;
    setEqualizer({ gains, preset: null });
  };

  const choose = (name: string) => {
    const preset = [...own, ...WINAMP_PRESETS, ...SITUATION_PRESETS].find((entry) => entry.name === name);
    if (preset) setEqualizer({ preamp: preset.preamp, gains: [...preset.gains], preset: preset.name, enabled: true });
  };

  const current = (name: string): EqPreset => ({ name, preamp: eq.preamp, gains: [...eq.gains] });

  const saveOwn = (name: string) => {
    const clean = name.trim();
    if (!clean) return;
    const next = [...own.filter((preset) => preset.name !== clean), current(clean)];
    storeOwnPresets(next);
    setOwn(next);
    setEqualizer({ preset: clean });
    setNaming(null);
  };

  const importEqf = async (file: File) => {
    setProblem(null);
    try {
      const presets = readEqf(await file.arrayBuffer());
      if (!presets.length) throw new Error(t('eqFileEmpty'));
      const names = new Set(presets.map((preset) => preset.name));
      const next = [...own.filter((preset) => !names.has(preset.name)), ...presets];
      storeOwnPresets(next);
      setOwn(next);
      setEqualizer({ preamp: presets[0].preamp, gains: [...presets[0].gains], preset: presets[0].name, enabled: true });
    } catch (error) {
      setProblem(`${t('eqFileUnreadable')}: ${error instanceof Error ? error.message : String(error)}`);
    }
  };

  const exportEqf = () => {
    const name = eq.preset ?? 'Studio';
    void saveFile(`${name}.eqf`, { blob: writeEqf([current(name)]) });
  };

  return (
    <div className="fixed z-[45] w-[380px] max-w-[96vw]" style={{ left: panel.pos.x, top: panel.pos.y }}>
      <div className="overflow-hidden rounded-xl border border-zinc-200 bg-white shadow-2xl dark:border-white/10 dark:bg-suno-card">
        <div
          onPointerDown={panel.onDragStart}
          className={`flex items-center gap-2 border-b border-zinc-200 px-3 py-2 dark:border-white/10 ${panel.dragging ? 'cursor-grabbing' : 'cursor-grab'}`}
        >
          <Move size={12} className="text-zinc-400" />
          <span className="text-[11px] font-semibold uppercase tracking-wide text-zinc-500">{t('eqTitle')}</span>
          <button
            type="button"
            onPointerDown={(event) => event.stopPropagation()}
            onClick={() => setEqualizer({ enabled: !eq.enabled })}
            className={`ml-1 rounded-md px-2 py-0.5 text-[10px] font-bold ${eq.enabled ? 'bg-pink-500 text-white' : 'bg-zinc-200 text-zinc-500 dark:bg-white/10 dark:text-zinc-400'}`}
          >
            {eq.enabled ? t('eqOn') : t('eqOff')}
          </button>
          <button type="button" onPointerDown={(event) => event.stopPropagation()} onClick={onClose} title={t('close')} className="ml-auto text-zinc-400 hover:text-zinc-900 dark:hover:text-white">
            <X size={15} />
          </button>
        </div>

        <div className="space-y-3 p-3">
          <svg viewBox={`0 0 ${CURVE_W} ${CURVE_H}`} className="h-[72px] w-full rounded-lg bg-zinc-50 dark:bg-black/30" preserveAspectRatio="none">
            <line x1="0" x2={CURVE_W} y1={CURVE_H / 2} y2={CURVE_H / 2} className="stroke-zinc-300 dark:stroke-white/10" strokeDasharray="3 3" />
            {EQ_BANDS.map((frequency) => {
              const x = (Math.log(frequency / 20) / Math.log(1000)) * CURVE_W;
              return <line key={frequency} x1={x} x2={x} y1="0" y2={CURVE_H} className="stroke-zinc-200 dark:stroke-white/5" />;
            })}
            <path d={curve} fill="none" strokeWidth="2" className={eq.enabled ? 'stroke-pink-500' : 'stroke-zinc-400'} />
          </svg>

          <div className="flex items-end justify-between gap-1">
            <Slider value={eq.preamp} label={t('eqPreamp')} title={t('eqPreampHint')} onChange={(value) => setEqualizer({ preamp: value, preset: null })} disabled={!eq.enabled} />
            <div className="mx-1 h-28 w-px self-center bg-zinc-200 dark:bg-white/10" />
            {EQ_BANDS.map((frequency, index) => (
              <Slider key={frequency} value={eq.gains[index] ?? 0} label={BAND_LABELS[index]} title={`${frequency} Hz`} onChange={(value) => band(index, value)} disabled={!eq.enabled} />
            ))}
          </div>

          <div className="flex flex-wrap items-center gap-1.5">
            <select
              aria-label={t('eqPresetLabel')}
              value={eq.preset ?? ''}
              onChange={(event) => choose(event.target.value)}
              className="min-w-0 flex-1 rounded-lg border border-zinc-200 bg-white px-2 py-1.5 text-xs text-zinc-800 dark:border-white/10 dark:bg-black/30 dark:text-zinc-100"
            >
              <option value="">{t('eqCustom')}</option>
              {own.length > 0 && (
                <optgroup label={t('eqOwnPresets')}>
                  {own.map((preset) => <option key={`own-${preset.name}`} value={preset.name}>{preset.name}</option>)}
                </optgroup>
              )}
              <optgroup label="Winamp">
                {WINAMP_PRESETS.map((preset) => <option key={preset.name} value={preset.name}>{preset.name}</option>)}
              </optgroup>
              <optgroup label={t('eqSituations')}>
                {SITUATION_PRESETS.map((preset) => <option key={preset.name} value={preset.name}>{preset.name}</option>)}
              </optgroup>
            </select>
            <button type="button" onClick={() => setEqualizer({ preamp: 0, gains: EQ_BANDS.map(() => 0), preset: null })} title={t('eqReset')} className="rounded-lg border border-zinc-200 p-1.5 text-zinc-500 hover:border-pink-400 hover:text-pink-600 dark:border-white/10">
              <RotateCcw size={14} />
            </button>
            <button type="button" onClick={() => setNaming(eq.preset ?? '')} title={t('eqSavePreset')} className="rounded-lg border border-zinc-200 p-1.5 text-zinc-500 hover:border-pink-400 hover:text-pink-600 dark:border-white/10">
              <Save size={14} />
            </button>
            <button type="button" onClick={() => importer.current?.click()} title={t('eqImport')} className="rounded-lg border border-zinc-200 p-1.5 text-zinc-500 hover:border-pink-400 hover:text-pink-600 dark:border-white/10">
              <Upload size={14} />
            </button>
            <button type="button" onClick={exportEqf} title={t('eqExport')} className="rounded-lg border border-zinc-200 p-1.5 text-zinc-500 hover:border-pink-400 hover:text-pink-600 dark:border-white/10">
              <Download size={14} />
            </button>
            <input
              ref={importer}
              type="file"
              accept=".eqf,.EQF,.q1"
              className="hidden"
              onChange={(event) => {
                const file = event.target.files?.[0];
                event.target.value = '';
                if (file) void importEqf(file);
              }}
            />
          </div>
          <div className="flex items-center gap-2 text-[10px] font-semibold text-zinc-500">
            <span>{t('eqBalanceLeft')}</span>
            <input
              type="range"
              min={-1}
              max={1}
              step={0.05}
              value={eq.balance}
              onChange={(event) => setEqualizer({ balance: Number(event.target.value) })}
              onDoubleClick={() => setEqualizer({ balance: 0 })}
              title={t('eqBalance')}
              className="h-1 min-w-0 flex-1 cursor-pointer accent-pink-500"
            />
            <span>{t('eqBalanceRight')}</span>
            <button
              type="button"
              onClick={() => setEqualizer({ mono: !eq.mono })}
              title={t('eqMonoHint')}
              className={`ml-1 rounded-md px-2 py-0.5 text-[10px] font-bold ${eq.mono ? 'bg-pink-500 text-white' : 'bg-zinc-200 text-zinc-500 dark:bg-white/10 dark:text-zinc-400'}`}
            >
              {eq.mono ? 'MONO' : 'STEREO'}
            </button>
          </div>
          {naming !== null && (
            <form
              className="flex items-center gap-1.5"
              onSubmit={(event) => {
                event.preventDefault();
                saveOwn(naming);
              }}
            >
              <input
                autoFocus
                value={naming}
                onChange={(event) => setNaming(event.target.value)}
                placeholder={t('eqPresetName')}
                className="min-w-0 flex-1 rounded-lg border border-zinc-200 bg-white px-2 py-1.5 text-xs text-zinc-800 dark:border-white/10 dark:bg-black/30 dark:text-zinc-100"
              />
              <button type="submit" disabled={!naming.trim()} className="rounded-lg bg-pink-500 px-2.5 py-1.5 text-xs font-semibold text-white disabled:opacity-40">{t('save')}</button>
              <button type="button" onClick={() => setNaming(null)} className="rounded-lg border border-zinc-200 px-2 py-1.5 text-xs text-zinc-500 dark:border-white/10">{t('cancel')}</button>
            </form>
          )}
          {problem && <p className="text-[11px] text-rose-600 dark:text-rose-300">{problem}</p>}
        </div>
      </div>
    </div>
  );
};

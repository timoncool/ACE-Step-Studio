import React, { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Check, FolderDown, FolderOpen, Loader2, Minimize2, Move, X } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { onSaving, revealSaved, type SavingFile } from '../services/saveFile';
import { dockSlot, useFloatable } from '../services/useFloatable';

/**
 * The files saved this session, from Dub Studio's files panel: a chip in the
 * sidebar, or a panel dragged anywhere over the window. Each save shows its
 * progress while it is written and a way to it in Explorer once it is.
 */

const size = (bytes?: number | null) => {
  if (!bytes || bytes <= 0) return '';
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1e3))} KB`;
};

export const FilesPanel: React.FC = () => {
  const { t } = useI18n();
  const [files, setFiles] = useState<SavingFile[]>([]);
  const [, redraw] = useState(0);
  const panel = useFloatable('files', { x: window.innerWidth - 360, y: 64 });
  const shown = useRef(0);

  useEffect(() => onSaving((change) => {
    setFiles((current) => {
      const known = current.find((file) => file.id === change.id);
      if (!known) return change.name ? [{ state: 'saving', ...change } as SavingFile, ...current] : current;
      return current.map((file) => (file.id === change.id ? { ...file, ...change } : file));
    });
  }), []);

  // the sidebar's slot exists once the sidebar has drawn
  useEffect(() => { redraw((n) => n + 1); }, []);

  // the first save opens the panel, so the user sees where it went
  useEffect(() => {
    if (shown.current === 0 && files.length > 0 && !panel.floating) panel.pop();
    shown.current = files.length;
  }, [files.length]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!files.length) return null;
  const busy = files.filter((file) => file.state === 'saving').length;

  if (!panel.floating) {
    const slot = dockSlot();
    if (!slot) return null;
    return createPortal(
      <button
        type="button"
        onClick={panel.pop}
        title={t('filesTitle')}
        className="flex w-full items-center gap-2 rounded-xl border border-zinc-200 px-3 py-2 text-left text-zinc-600 transition-colors hover:border-pink-400 hover:text-pink-600 dark:border-white/10 dark:text-zinc-300"
      >
        {busy ? <Loader2 size={15} className="shrink-0 animate-spin text-pink-500" /> : <FolderDown size={15} className="shrink-0 text-pink-500" />}
        <span className="min-w-0 flex-1 truncate text-xs font-medium">{t('filesTitle')}</span>
        <span className="text-[11px] tabular-nums text-zinc-400">{files.length}</span>
      </button>,
      slot,
    );
  }

  return (
    <div className="fixed z-[90] w-[min(90vw,340px)]" style={{ left: panel.pos.x, top: panel.pos.y }}>
      <div className="w-full overflow-hidden rounded-xl border border-zinc-200 bg-white shadow-2xl dark:border-white/10 dark:bg-suno-card">
        <div
          onPointerDown={panel.onDragStart}
          className={`flex items-center justify-between border-b border-zinc-200 px-3 py-2 dark:border-white/10 ${panel.dragging ? 'cursor-grabbing' : 'cursor-grab'}`}
        >
          <span className="flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wide text-zinc-500">
            <Move size={12} />
            {t('filesTitle')}
          </span>
          <button type="button" onClick={panel.dock} title={t('filesDock')} className="text-zinc-400 hover:text-zinc-900 dark:hover:text-white">
            <Minimize2 size={14} />
          </button>
        </div>
        <div className="max-h-[52vh] space-y-1.5 overflow-y-auto p-2">
          {files.map((file) => {
            const percent = file.total ? Math.min(100, Math.round(((file.written ?? 0) / file.total) * 100)) : null;
            return (
              <div key={file.id} className="rounded-lg bg-zinc-50 p-2.5 dark:bg-white/5">
                <div className="flex items-center gap-2">
                  {file.state === 'saving' ? (
                    <Loader2 size={14} className="shrink-0 animate-spin text-pink-500" />
                  ) : file.state === 'error' ? (
                    <X size={14} className="shrink-0 text-rose-500" />
                  ) : (
                    <Check size={14} className="shrink-0 text-emerald-500" />
                  )}
                  <span className="min-w-0 flex-1 truncate text-[13px] text-zinc-800 dark:text-zinc-100" title={file.path ?? file.name}>{file.name}</span>
                  <span className="shrink-0 text-[10px] tabular-nums text-zinc-400">{size(file.state === 'done' ? file.written : file.total)}</span>
                </div>
                {file.state === 'saving' && (
                  <div className="mt-1.5 h-1 w-full overflow-hidden rounded-full bg-zinc-200 dark:bg-white/10">
                    <div
                      className={`h-full rounded-full bg-gradient-to-r from-orange-500 to-pink-500 ${percent === null ? 'w-1/3 animate-pulse' : 'transition-[width]'}`}
                      style={percent === null ? undefined : { width: `${Math.max(2, percent)}%` }}
                    />
                  </div>
                )}
                {file.path && <div className="mt-1 truncate text-[10px] text-zinc-500" title={file.path}>{file.path}</div>}
                {file.state === 'done' && file.path && (
                  <button
                    type="button"
                    onClick={() => void revealSaved(file.path as string)}
                    className="mt-2 inline-flex items-center gap-1.5 rounded-md border border-zinc-300 px-2.5 py-1 text-[12px] text-zinc-600 hover:border-pink-400 hover:text-pink-600 dark:border-white/15 dark:text-zinc-300"
                  >
                    <FolderOpen size={13} /> {t('filesShowInFolder')}
                  </button>
                )}
                {file.state === 'error' && file.error && <div className="mt-1 break-words text-[11px] text-rose-600 dark:text-rose-300">{file.error}</div>}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
};

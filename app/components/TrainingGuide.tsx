import React, { useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { BookOpen, CheckSquare, ChevronDown, Move, Square, X } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { GuideSection, trainingGuide } from '../i18n/trainingGuide';

/**
 * The training guide, in a window of its own that stays over the page: it is
 * dragged by its title bar and resized from its corner, so the steps can be
 * read beside the form they describe. Where it was left, how large it was and
 * which checklist items are ticked are kept between sessions.
 */

const GEOMETRY_KEY = 'studio.trainingGuide.geometry';
const CHECKED_KEY = 'studio.trainingGuide.checked';
const MIN_WIDTH = 340;
const MIN_HEIGHT = 260;

interface Geometry {
  x: number;
  y: number;
  width: number;
  height: number;
}

function load<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    if (raw) return JSON.parse(raw) as T;
  } catch {
    // A blocked or corrupt entry only costs the remembered layout.
  }
  return fallback;
}

function save(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // The layout is a convenience, not state worth failing over.
  }
}

function fitted(geometry: Geometry): Geometry {
  const width = Math.max(MIN_WIDTH, Math.min(window.innerWidth - 16, geometry.width));
  const height = Math.max(MIN_HEIGHT, Math.min(window.innerHeight - 16, geometry.height));
  return {
    width,
    height,
    x: Math.max(0, Math.min(window.innerWidth - 120, geometry.x)),
    y: Math.max(0, Math.min(window.innerHeight - 48, geometry.y)),
  };
}

function initialGeometry(): Geometry {
  const width = Math.min(460, window.innerWidth - 32);
  const height = Math.min(640, window.innerHeight - 96);
  return fitted(load<Geometry>(GEOMETRY_KEY, { x: window.innerWidth - width - 24, y: 72, width, height }));
}

const Section: React.FC<{
  section: GuideSection;
  index: number;
  open: boolean;
  onToggle: () => void;
  checked: Record<string, boolean>;
  onCheck: (id: string) => void;
}> = ({ section, index, open, onToggle, checked, onCheck }) => (
  <div className="rounded-lg border border-zinc-200 dark:border-white/10">
    <button type="button" onClick={onToggle} className="flex w-full items-center gap-2 px-3 py-2 text-left">
      <ChevronDown size={14} className={`shrink-0 text-zinc-400 transition-transform ${open ? '' : '-rotate-90'}`} />
      <span className="text-sm font-semibold text-zinc-800 dark:text-zinc-100">{section.title}</span>
    </button>
    {open && (
      <div className="space-y-2 px-3 pb-3 text-[13px] leading-6 text-zinc-600 dark:text-zinc-300">
        {section.text?.map((paragraph, at) => <p key={at}>{paragraph}</p>)}
        {section.steps && (
          <ol className="list-decimal space-y-1 pl-5 marker:text-zinc-400">
            {section.steps.map((step, at) => <li key={at}>{step}</li>)}
          </ol>
        )}
        {section.list && (
          <ul className="list-disc space-y-1 pl-5 marker:text-zinc-400">
            {section.list.map((entry, at) => <li key={at}>{entry}</li>)}
          </ul>
        )}
        {section.checklist && (
          <ul className="space-y-1">
            {section.checklist.map((entry, at) => {
              const id = `${index}.${at}`;
              const done = Boolean(checked[id]);
              return (
                <li key={at}>
                  <button type="button" onClick={() => onCheck(id)} className="flex w-full items-start gap-2 text-left">
                    {done ? <CheckSquare size={15} className="mt-1 shrink-0 text-pink-500" /> : <Square size={15} className="mt-1 shrink-0 text-zinc-400" />}
                    <span className={done ? 'text-zinc-400 line-through dark:text-zinc-500' : ''}>{entry}</span>
                  </button>
                </li>
              );
            })}
          </ul>
        )}
        {section.examples?.map((example, at) => (
          <div key={at} className="rounded-md bg-zinc-100 px-2.5 py-1.5 dark:bg-black/30">
            {example.label && <div className="text-[11px] font-semibold uppercase tracking-wide text-zinc-400">{example.label}</div>}
            <pre className="whitespace-pre-wrap break-words font-mono text-[12px] leading-5 text-zinc-700 dark:text-zinc-200">{example.body}</pre>
          </div>
        ))}
      </div>
    )}
  </div>
);

export const TrainingGuide: React.FC<{ onClose: () => void }> = ({ onClose }) => {
  const { language } = useI18n();
  const guide = trainingGuide[language] ?? trainingGuide.en;
  const [geometry, setGeometry] = useState<Geometry>(initialGeometry);
  const [open, setOpen] = useState<Record<number, boolean>>({ 0: true });
  const [checked, setChecked] = useState<Record<string, boolean>>(() => load(CHECKED_KEY, {}));
  const drag = useRef<{ dx: number; dy: number } | null>(null);
  const resize = useRef<{ x: number; y: number; width: number; height: number } | null>(null);

  useEffect(() => {
    const onWindowResize = () => setGeometry(current => fitted(current));
    window.addEventListener('resize', onWindowResize);
    return () => window.removeEventListener('resize', onWindowResize);
  }, []);

  const onDragStart = useCallback((event: React.PointerEvent) => {
    if ((event.target as HTMLElement).closest('button')) return;
    // No text selection starts under a drag
    event.preventDefault();
    drag.current = { dx: event.clientX - geometry.x, dy: event.clientY - geometry.y };
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }, [geometry]);

  const onDragMove = useCallback((event: React.PointerEvent) => {
    if (!drag.current) return;
    const { dx, dy } = drag.current;
    setGeometry(current => fitted({ ...current, x: event.clientX - dx, y: event.clientY - dy }));
  }, []);

  const onResizeStart = useCallback((event: React.PointerEvent) => {
    event.preventDefault();
    resize.current = { x: event.clientX, y: event.clientY, width: geometry.width, height: geometry.height };
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    event.stopPropagation();
  }, [geometry]);

  const onResizeMove = useCallback((event: React.PointerEvent) => {
    if (!resize.current) return;
    const start = resize.current;
    setGeometry(current => fitted({ ...current, width: start.width + event.clientX - start.x, height: start.height + event.clientY - start.y }));
  }, []);

  const onPointerEnd = useCallback(() => {
    if (!drag.current && !resize.current) return;
    drag.current = null;
    resize.current = null;
    setGeometry(current => {
      save(GEOMETRY_KEY, current);
      return current;
    });
  }, []);

  const toggleCheck = useCallback((id: string) => {
    setChecked(current => {
      const next = { ...current, [id]: !current[id] };
      save(CHECKED_KEY, next);
      return next;
    });
  }, []);

  const allOpen = guide.sections.every((_, index) => open[index]);

  return createPortal(
    <div
      className="fixed z-[70] flex flex-col overflow-hidden rounded-xl border border-zinc-200 bg-white/95 shadow-2xl backdrop-blur dark:border-white/10 dark:bg-zinc-900/95"
      style={{ left: geometry.x, top: geometry.y, width: geometry.width, height: geometry.height }}
      role="dialog"
      aria-label={guide.title}
    >
      <div
        onPointerDown={onDragStart}
        onPointerMove={onDragMove}
        onPointerUp={onPointerEnd}
        className="flex shrink-0 cursor-grab select-none items-center gap-2 border-b border-zinc-200 px-3 py-2 active:cursor-grabbing dark:border-white/5"
      >
        <Move size={13} className="text-zinc-400" />
        <BookOpen size={14} className="text-pink-500" />
        <span className="min-w-0 flex-1 truncate text-sm font-semibold text-zinc-900 dark:text-white">{guide.title}</span>
        <button
          type="button"
          onClick={() => setOpen(allOpen ? {} : Object.fromEntries(guide.sections.map((_, index) => [index, true])))}
          className="rounded-md px-2 py-0.5 text-[11px] font-semibold text-zinc-500 hover:text-pink-500"
        >
          {allOpen ? guide.collapseAll : guide.expandAll}
        </button>
        <button type="button" onClick={onClose} title={guide.close} className="text-zinc-400 hover:text-pink-500">
          <X size={15} />
        </button>
      </div>
      <div className="min-h-0 flex-1 space-y-2 overflow-y-auto p-3">
        {guide.intro && <p className="text-[13px] leading-6 text-zinc-600 dark:text-zinc-300">{guide.intro}</p>}
        {guide.sections.map((section, index) => (
          <Section
            key={index}
            section={section}
            index={index}
            open={Boolean(open[index])}
            onToggle={() => setOpen(current => ({ ...current, [index]: !current[index] }))}
            checked={checked}
            onCheck={toggleCheck}
          />
        ))}
      </div>
      <div
        onPointerDown={onResizeStart}
        onPointerMove={onResizeMove}
        onPointerUp={onPointerEnd}
        title={guide.resize}
        className="absolute bottom-0 right-0 h-4 w-4 cursor-nwse-resize"
        style={{ background: 'linear-gradient(135deg, transparent 50%, rgba(161,161,170,.6) 50%, rgba(161,161,170,.6) 60%, transparent 60%, transparent 72%, rgba(161,161,170,.6) 72%, rgba(161,161,170,.6) 82%, transparent 82%)' }}
      />
    </div>,
    document.body,
  );
};

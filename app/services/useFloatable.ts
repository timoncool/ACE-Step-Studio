import { useCallback, useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react';

/**
 * A panel that sits docked as a chip or floats wherever it is dragged, from
 * Dub Studio. Where it floats and whether it does are remembered.
 */

export interface Floatable {
  floating: boolean;
  pos: { x: number; y: number };
  dragging: boolean;
  pop: () => void;
  dock: () => void;
  onDragStart: (event: ReactPointerEvent) => void;
}

function remembered(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function remember(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // a panel that forgets its place still works
  }
}

// A place saved in a bigger window, or a broken one, is pulled back into view.
function clampPos(p: { x: number; y: number }): { x: number; y: number } {
  const vw = typeof window !== 'undefined' ? window.innerWidth : 1280;
  const vh = typeof window !== 'undefined' ? window.innerHeight : 800;
  const x = Math.round(Math.max(4, Math.min(vw - 60, Number.isFinite(p.x) ? p.x : vw - 360)));
  const y = Math.round(Math.max(4, Math.min(vh - 40, Number.isFinite(p.y) ? p.y : 64)));
  return { x, y };
}

export function useFloatable(key: string, initial: { x: number; y: number }): Floatable {
  const [floating, setFloating] = useState<boolean>(() => remembered(`fl:${key}:on`) === '1');
  // where the user put the panel; a smaller window draws it within itself without moving it for good
  const [placed, setPlaced] = useState<{ x: number; y: number }>(() => {
    const saved = remembered(`fl:${key}:pos`);
    if (saved) {
      try {
        return clampPos(JSON.parse(saved));
      } catch {
        // an unreadable place is the initial one
      }
    }
    return clampPos(initial);
  });
  const [, redraw] = useState(0);
  const pos = clampPos(placed);
  const [dragging, setDragging] = useState(false);
  const offset = useRef({ x: 0, y: 0 });

  useEffect(() => { remember(`fl:${key}:on`, floating ? '1' : '0'); }, [key, floating]);
  useEffect(() => { remember(`fl:${key}:pos`, JSON.stringify(placed)); }, [key, placed]);
  useEffect(() => {
    const onResize = () => redraw((n) => n + 1);
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);

  const onDragStart = useCallback((event: ReactPointerEvent) => {
    if (event.button !== 0) return;
    const from = clampPos(placed);
    offset.current = { x: event.clientX - from.x, y: event.clientY - from.y };
    setDragging(true);
    event.preventDefault();
  }, [placed]);

  useEffect(() => {
    if (!dragging) return;
    const move = (event: PointerEvent) => {
      const x = Math.max(4, Math.min(window.innerWidth - 60, event.clientX - offset.current.x));
      const y = Math.max(4, Math.min(window.innerHeight - 40, event.clientY - offset.current.y));
      setPlaced({ x, y });
    };
    const up = () => setDragging(false);
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
    return () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
    };
  }, [dragging]);

  return {
    floating,
    pos,
    dragging,
    pop: () => setFloating(true),
    dock: () => setFloating(false),
    onDragStart,
  };
}

/** Where a docked panel shows its chip. */
export function dockSlot(): HTMLElement | null {
  return typeof document !== 'undefined' ? document.getElementById('dock-slot') : null;
}

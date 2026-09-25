/**
 * The page's side of the MCP bridge: an agent connected to the studio's MCP
 * server sees this window and works it like a user - a screenshot, what can
 * be clicked, clicks and typing, the pages, the player and the video editor.
 *
 * The service streams each command here and waits for the answer this page
 * posts back. Commands that belong to a screen (the player, the editor) are
 * answered by that screen through `useBridgeCommand`.
 */
import { useEffect, useRef } from 'react';
import { domToPng } from 'modern-screenshot';
import { apiUrl } from './apiBase';

type Handler = (args: Record<string, unknown>) => unknown | Promise<unknown>;

const handlers = new Map<string, Handler>();

/** Answers a command while the calling component is mounted. */
export function useBridgeCommand(command: string, handler: Handler): void {
  const latest = useRef(handler);
  latest.current = handler;
  useEffect(() => {
    const run: Handler = (args) => latest.current(args);
    handlers.set(command, run);
    return () => {
      if (handlers.get(command) === run) handlers.delete(command);
    };
  }, [command]);
}

// ---------------------------------------------------------------- what is on screen

const INTERACTIVE = 'button, a[href], input, textarea, select, [role="button"], [role="tab"], [role="menuitem"], [role="checkbox"], [role="switch"], [role="slider"], [contenteditable="true"], [data-mcp-context]';

function visible(element: Element): boolean {
  const box = element.getBoundingClientRect();
  if (box.width === 0 || box.height === 0) return false;
  const style = getComputedStyle(element);
  return style.visibility !== 'hidden' && style.display !== 'none';
}

/// The name of a lucide icon a button shows instead of words.
function iconName(element: Element): string {
  const classes = element.querySelector('svg')?.getAttribute('class') ?? '';
  const icon = classes.split(/\s+/).find((name) => name.startsWith('lucide-'));
  return icon ? `${icon.slice('lucide-'.length).replace(/-/g, ' ')} icon` : '';
}

/// The text written next to a field: a label, a heading, a caption before it.
function nearbyText(element: Element): string {
  let node: Element | null = element;
  for (let depth = 0; depth < 2 && node; depth += 1) {
    for (let sibling = node.previousElementSibling; sibling; sibling = sibling.previousElementSibling) {
      if (sibling.matches(INTERACTIVE) || sibling.querySelector(INTERACTIVE) || sibling.matches('h1, h2') || sibling.querySelector('h1, h2')) continue;
      const text = (sibling.textContent || '').replace(/\s+/g, ' ').trim();
      if (text && text.length <= 80) return text;
    }
    node = node.parentElement;
  }
  return '';
}

function label(element: Element): string {
  const own = element.getAttribute('aria-label') || element.getAttribute('title') || '';
  if (own) return own.slice(0, 80);
  if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement || element instanceof HTMLSelectElement) {
    const tied = element.labels?.[0]?.textContent?.replace(/\s+/g, ' ').trim();
    return (tied || nearbyText(element) || element.getAttribute('placeholder') || '').slice(0, 80);
  }
  const text = (element.textContent || '').replace(/\s+/g, ' ').trim();
  return (text || iconName(element) || nearbyText(element)).slice(0, 80);
}

let nextRef = 1;

function refOf(element: Element): string {
  const existing = element.getAttribute('data-mcp-ref');
  if (existing) return existing;
  const ref = `e${nextRef++}`;
  element.setAttribute('data-mcp-ref', ref);
  return ref;
}

/// The dialog on top, when one is open: its controls are what the user can reach.
function openDialog(): Element | null {
  const dialogs = Array.from(document.querySelectorAll('[role="dialog"], [aria-modal="true"], .fixed.inset-0')).filter(visible);
  return dialogs.length ? dialogs[dialogs.length - 1] : null;
}

/// Every visible control, one line each, with the ref the other commands take.
/// A control inside a song row names the song it belongs to.
function readPage(): string {
  const lines: string[] = [];
  const title = document.querySelector('h1, h2')?.textContent?.trim();
  if (title) lines.push(`Page: ${title}`);
  const dialog = openDialog();
  if (dialog) lines.push('A dialog is open; its controls are listed first, then the page behind it.');
  const scope = dialog ? [dialog, document.body] : [document.body];
  const seen = new Set<Element>();
  // a song row and its buttons read as one line, not six
  const rows = new Map<string, { at: number; parts: string[] }>();
  for (const root of scope) {
    for (const element of Array.from(root.querySelectorAll(INTERACTIVE))) {
      if (seen.has(element) || !visible(element)) continue;
      seen.add(element);
      const tag = element.tagName.toLowerCase();
      const kind = element.getAttribute('role') || (tag === 'input' ? `input ${(element as HTMLInputElement).type}` : tag);
      const context = element.closest('[data-mcp-context]')?.getAttribute('data-mcp-context');
      if (context) {
        const row = rows.get(context) ?? { at: lines.length, parts: [] };
        if (!rows.has(context)) {
          rows.set(context, row);
          lines.push('');
        }
        row.parts.push(`${refOf(element)} ${element.hasAttribute('data-mcp-context') ? 'open' : label(element)}${(element as HTMLButtonElement).disabled ? ' (disabled)' : ''}`);
        continue;
      }
      const parts = [refOf(element), kind, JSON.stringify(label(element))];
      if (element instanceof HTMLInputElement && (element.type === 'checkbox' || element.type === 'radio')) {
        parts.push(element.checked ? 'checked' : 'unchecked');
      } else if (element instanceof HTMLInputElement && element.type === 'range') {
        parts.push(`value=${element.value}`, `range=${element.min || 0}..${element.max || 100}`, `step=${element.step || 1}`);
      } else if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement) {
        if (element.value) parts.push(`value=${JSON.stringify(element.value.slice(0, 160))}`);
        else parts.push('empty', ...(element.placeholder ? [`placeholder=${JSON.stringify(element.placeholder.slice(0, 80))}`] : []));
      }
      if (element instanceof HTMLSelectElement) {
        parts.push(`value=${JSON.stringify(element.value)}`, `options=${JSON.stringify(Array.from(element.options).map((option) => option.value))}`);
      }
      const pressed = element.getAttribute('aria-checked') ?? element.getAttribute('aria-pressed') ?? element.getAttribute('aria-selected');
      if (pressed) parts.push(pressed === 'true' ? 'on' : 'off');
      if ((element as HTMLButtonElement).disabled) parts.push('disabled');
      lines.push(parts.join(' '));
    }
  }
  for (const [context, row] of rows) lines[row.at] = `${context}: ${row.parts.join(', ')}`;
  return lines.join('\n');
}

function find(args: Record<string, unknown>): HTMLElement {
  const ref = typeof args.ref === 'string' ? args.ref : '';
  const text = typeof args.text === 'string' ? args.text.trim().toLowerCase() : '';
  let element: Element | null = null;
  if (ref) element = document.querySelector(`[data-mcp-ref="${CSS.escape(ref)}"]`);
  if (!element && text) {
    element = Array.from(document.querySelectorAll(INTERACTIVE)).find((candidate) => visible(candidate) && label(candidate).toLowerCase() === text)
      ?? Array.from(document.querySelectorAll(INTERACTIVE)).find((candidate) => visible(candidate) && label(candidate).toLowerCase().includes(text))
      ?? null;
  }
  if (!element) throw new Error(ref ? `No element ${ref} on the page now; call ui_read_page again, refs change when the page does.` : `No control labelled "${args.text}" is visible; call ui_read_page to see what is.`);
  return element as HTMLElement;
}

/** Sets a value the way typing does, so React sees it. */
function setValue(element: HTMLElement, value: string): void {
  if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement) {
    const prototype = element instanceof HTMLInputElement ? HTMLInputElement.prototype : HTMLTextAreaElement.prototype;
    Object.getOwnPropertyDescriptor(prototype, 'value')?.set?.call(element, value);
    element.dispatchEvent(new Event('input', { bubbles: true }));
    element.dispatchEvent(new Event('change', { bubbles: true }));
  } else if (element.isContentEditable) {
    element.textContent = value;
    element.dispatchEvent(new InputEvent('input', { bubbles: true }));
  } else {
    throw new Error('That element does not take text.');
  }
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 250));

// ---------------------------------------------------------------- what the page logged

const logged: Array<{ level: string; text: string; at: string }> = [];

function remember(level: string, parts: unknown[]): void {
  const text = parts.map((part) => (part instanceof Error ? `${part.name}: ${part.message}` : typeof part === 'string' ? part : JSON.stringify(part))).join(' ');
  logged.push({ level, text: text.slice(0, 2000), at: new Date().toISOString() });
  if (logged.length > 100) logged.shift();
}

/// Keeps the page's errors and warnings for an agent to read.
function watchConsole(): void {
  for (const level of ['error', 'warn'] as const) {
    const original = console[level].bind(console);
    console[level] = (...parts: unknown[]) => {
      remember(level, parts);
      original(...parts);
    };
  }
  window.addEventListener('error', (event) => remember('error', [event.message, `${event.filename}:${event.lineno}`]));
  window.addEventListener('unhandledrejection', (event) => remember('error', ['unhandled rejection', event.reason]));
}

const builtIn: Record<string, Handler> = {
  async screenshot(args) {
    const scale = Math.min(1, Number(args.max_width ?? 1600) / window.innerWidth);
    const data = await domToPng(document.documentElement, { scale, width: window.innerWidth, height: window.innerHeight, backgroundColor: getComputedStyle(document.body).backgroundColor });
    return { image: data.replace(/^data:image\/png;base64,/, ''), text: `${window.innerWidth}x${window.innerHeight} window` };
  },
  read_page: () => ({ text: readPage() }),
  console: () => ({ text: logged.length ? logged.map((entry) => `${entry.at} ${entry.level}: ${entry.text}`).join('\n') : 'Nothing logged.' }),
  async click(args) {
    const element = find(args);
    element.scrollIntoView({ block: 'center' });
    element.click();
    await settle();
    return { text: `Clicked ${label(element) || element.tagName.toLowerCase()}.` };
  },
  async type(args) {
    const element = find(args);
    element.focus();
    setValue(element, String(args.value ?? ''));
    if (args.submit) element.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    element.blur();
    await settle();
    return { text: 'Typed.' };
  },
  async select(args) {
    const element = find(args);
    if (!(element instanceof HTMLSelectElement)) throw new Error('That element is not a list; use ui_click on its options.');
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value')?.set?.call(element, String(args.value ?? ''));
    element.dispatchEvent(new Event('change', { bubbles: true }));
    await settle();
    return { text: `Selected ${element.value}.` };
  },
  async press_key(args) {
    const target = (document.activeElement as HTMLElement | null) ?? document.body;
    const key = String(args.key ?? '');
    target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
    target.dispatchEvent(new KeyboardEvent('keyup', { key, bubbles: true }));
    await settle();
    return { text: `Pressed ${key}.` };
  },
  async scroll(args) {
    const amount = Number(args.amount ?? 600) * (args.direction === 'up' ? -1 : 1);
    const element = args.ref || args.text ? find(args) : null;
    if (element) element.scrollIntoView({ block: 'center' });
    else (document.querySelector('main') ?? document.scrollingElement ?? document.body).scrollBy({ top: amount });
    await settle();
    return { text: 'Scrolled.' };
  },
};

// ---------------------------------------------------------------- the connection

let started = false;

export function startBridge(): void {
  if (started || typeof window === 'undefined' || typeof EventSource === 'undefined') return;
  started = true;
  watchConsole();
  const connect = () => {
    const events = new EventSource(apiUrl('/mcp/window'));
    // the service names this window first; a command for another window is not ours
    let ours: number | null = null;
    events.onmessage = async (message) => {
      const data = JSON.parse(message.data) as { window: number; id?: string; command?: string; args?: Record<string, unknown> };
      if (data.id === undefined) {
        ours = data.window;
        return;
      }
      if (data.window !== ours) return;
      const command = data as { id: string; command: string; args: Record<string, unknown> };
      const handler = handlers.get(command.command) ?? builtIn[command.command];
      let body: { id: string; result?: unknown; error?: string };
      if (!handler) {
        body = { id: command.id, error: `The window cannot do "${command.command}" on this screen; open the screen it belongs to first.` };
      } else {
        try {
          body = { id: command.id, result: (await handler(command.args ?? {})) ?? null };
        } catch (problem) {
          body = { id: command.id, error: problem instanceof Error ? problem.message : String(problem) };
        }
      }
      await fetch(apiUrl('/mcp/window/result'), { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }).catch((problem) => console.error('[ERROR] the MCP answer did not reach the studio:', problem));
    };
    // the service restarting closes the stream; the page subscribes again
    events.onerror = () => {
      events.close();
      setTimeout(connect, 2000);
    };
  };
  connect();
}

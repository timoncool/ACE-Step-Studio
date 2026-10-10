/**
 * Studio Hub in the window: the notices the service fetched (strip stack, popups, news) and the telemetry choice.
 * The service keeps the state and the hub addresses; the window draws everything in the studio's own style.
 */
import { apiUrl } from './apiBase';

export type HubTheme = 'sunset' | 'orchid' | 'lime' | 'graphite';

export interface HubButton {
  label: string;
  action: 'url' | 'dismiss' | 'open';
  url?: string;
  target?: 'news' | 'settings' | 'models' | 'update';
  style: 'primary' | 'secondary';
}

export interface HubContent {
  title: string;
  body: string;
  buttons: HubButton[];
}

export interface HubItem {
  id: string;
  kind: 'news' | 'bar' | 'popup';
  priority: number;
  theme: HubTheme | null;
  dismissible: boolean;
  rules: HubRules;
  image: string | null;
  date: string | null;
  ad: { label: boolean; erid?: string } | null;
  content: Record<string, HubContent>;
  seen: { shows: number; last_shown: number | null; shown_session: number | null; clicked_at: number | null; dismissed_at: number | null };
  /** The hub's rules allow it in this launch; the delay and the sections are applied here. */
  eligible: boolean;
}

export interface HubRules {
  delay_s: number;
  after_sessions: number;
  views: string[] | null;
  frequency: 'once' | 'session' | 'interval';
  interval_h: number;
  max_shows: number | null;
  after_dismiss: 'never' | 'snooze';
  snooze_h: number;
  audience: 'all' | 'new' | 'returning';
  new_days: number;
}

export interface HubState {
  telemetry: { enabled: boolean; acknowledged: boolean; disabledByEnv: boolean; install: string | null };
  source: string | null;
  fetchedAt: number | null;
  lastError: string | null;
  test: boolean;
  items: HubItem[];
}

export function hubText(item: HubItem, lang: string): HubContent | undefined {
  return item.content[lang] ?? item.content.en ?? Object.values(item.content)[0];
}

async function call<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(apiUrl(path), init);
  if (!response.ok) throw new Error(`${path}: HTTP ${response.status}`);
  return response.json() as Promise<T>;
}

export const fetchHubState = (lang: string) => call<HubState>(`/v1/hub/state?lang=${encodeURIComponent(lang)}`);

/** `button` names a click: `b0`, `b1`... the notice's buttons in order, `link` a link in its text. */
export const reportNotice = (id: string, event: 'shown' | 'clicked' | 'dismissed', button?: string) =>
  call<{ ok: true }>(`/v1/hub/notices/${encodeURIComponent(id)}/${event}${button ? `?button=${encodeURIComponent(button)}` : ''}`, { method: 'POST' });

export const setTelemetry = (enabled: boolean, acknowledge = false) =>
  call<HubState['telemetry']>('/v1/hub/telemetry', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ enabled, acknowledge }),
  });

export const telemetryPreview = () => call<{ enabled: boolean; report: unknown }>('/v1/hub/telemetry/preview');

export const resetInstall = () => call<unknown>('/v1/hub/telemetry/reset', { method: 'POST' });

export const refreshHub = () => call<unknown>('/v1/hub/refresh', { method: 'POST' });


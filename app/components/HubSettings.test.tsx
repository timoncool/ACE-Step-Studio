import { describe, expect, it, vi } from 'vitest';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ t: (key: string) => key, language: 'en' }) }));
vi.mock('../services/studioQueries', () => ({ hubStateChanged: vi.fn(), useHubState: () => ({ data: null }) }));
vi.mock('../services/studioHub', () => ({ refreshHub: vi.fn(), resetInstall: vi.fn(), setTelemetry: vi.fn() }));
vi.mock('./HubTelemetryPreview', () => ({ HubTelemetryPreview: () => null }));

const { hubFeedLine } = await import('./HubSettings');

describe('the news line in Settings', () => {
  it('says when the news were fetched, never the hub address', () => {
    const t = (key: string) => key;
    const line = hubFeedLine({ source: 'https://hub.neuro-cartel.com', fetchedAt: 1_800_000_000 }, 'en', t);
    expect(line.startsWith('hubFeedUpdated ')).toBe(true);
    expect(line).not.toContain('neuro-cartel');
    expect(line).not.toContain('http');
    expect(hubFeedLine({ source: null }, 'en', t)).toBe('hubFeedNever');
    expect(hubFeedLine(null, 'en', t)).toBe('hubFeedNever');
  });
});

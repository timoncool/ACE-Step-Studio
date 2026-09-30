import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { translations } from '../i18n/translations';

describe('the journal in every language', () => {
  it('has the words for every change the studio writes into it', () => {
    // the service's table of what each tool's change is written as
    const source = readFileSync(resolve(__dirname, '../../crates/music-server/src/mcp.rs'), 'utf8');
    const start = source.indexOf('const JOURNAL:');
    const facts = [...source.slice(start, source.indexOf('];', start)).matchAll(/\("[a-z0-9_]+", "([a-z_]+)", "([a-z_]+)"\)/g)];
    expect(facts.length).toBeGreaterThan(40);
    // a like has its own route, with its own two verbs
    const verbs = new Set([...facts.map(fact => fact[1]), 'liked', 'unliked']);
    const kinds = new Set(facts.map(fact => fact[2]));
    for (const [language, words] of Object.entries(translations)) {
      const missing = [...[...verbs].map(verb => `journalVerb_${verb}`), ...[...kinds].map(kind => `journalKind_${kind}`)]
        .filter(key => !(words as Record<string, string | undefined>)[key]);
      expect(missing, language).toEqual([]);
    }
  });
});

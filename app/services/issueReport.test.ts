import fs from 'fs';
import path from 'path';
import { describe, expect, it } from 'vitest';
import { bugTemplate, formWords, issueReportUrl, issueTemplate, MAX_URL } from './issueReport';

const base = 'https://github.com/timoncool/ACE-Step-Studio';
const FORMS = path.resolve(__dirname, '../../.github/ISSUE_TEMPLATE');
const LANGUAGES = [['en', '1'], ['ru', '2'], ['zh', '3'], ['ja', '4'], ['ko', '5']] as const;
const read = (file: string) => fs.readFileSync(path.join(FORMS, file), 'utf-8');
const ids = (form: string) => [...form.matchAll(/^\s+id: (\S+)$/gm)].map(match => match[1]);
const options = (form: string) => [...form.matchAll(/^\s+options:\n((?:\s{8}- .+\n)+)/gm)].map(match => match[1]);

describe('the report link', () => {
  it('opens the bug form of the window language', () => {
    for (const [language, order] of LANGUAGES) {
      const file = `${order}-${language}-1-bug.yml`;
      expect(bugTemplate(language)).toBe(file);
      expect(fs.existsSync(path.join(FORMS, file)), file).toBe(true);
      const url = new URL(issueReportUrl('https://github.com/timoncool/ACE-Step-Studio', '4.0.0', language, { selected_profile_id: 'quality-q8' }));
      expect(url.pathname).toBe('/timoncool/ACE-Step-Studio/issues/new');
      expect(url.searchParams.get('template')).toBe(file);
      expect(url.searchParams.get('version')).toBe('4.0.0');
      expect(url.searchParams.get('models')).toBe('quality-q8');
    }
    expect(bugTemplate('de')).toBe('1-en-1-bug.yml');
    for (const [language, order] of LANGUAGES) {
      for (const [kind, file] of [['idea', '2-idea'], ['question', '3-question']] as const) {
        expect(issueTemplate(language, kind)).toBe(`${order}-${language}-${file}.yml`);
        expect(fs.existsSync(path.join(FORMS, issueTemplate(language, kind)))).toBe(true);
      }
    }
  });

  it('finds the same fields, choices and labels in all five languages of a form, English first in the list', () => {
    for (const kind of ['1-bug', '2-idea', '3-question']) {
      const english = read(`1-en-${kind}.yml`);
      for (const [language, order] of LANGUAGES.slice(1)) {
        const form = read(`${order}-${language}-${kind}.yml`);
        expect(ids(form), `${language}-${kind}`).toEqual(ids(english));
        expect(options(form), `${language}-${kind}`).toEqual(options(english));
        expect(form.match(/^labels: .*$/m)?.[0]).toBe(english.match(/^labels: .*$/m)?.[0]);
      }
    }
    const files = fs.readdirSync(FORMS).filter(file => file !== 'config.yml').sort();
    expect(files.length).toBe(15);
    expect(files.slice(0, 3).every(file => file.startsWith('1-en-'))).toBe(true);
    const names = files.map(file => read(file).match(/^name: (.+)$/m)?.[1]);
    expect(new Set(names).size, 'every form has a name of its own').toBe(names.length);
  });

  it('fills the fields of the form of each kind with what was typed in the window, empty ones left out', () => {
    const words = { what: 'The song stops', steps: 'Press Create', expected: 'A whole song' };
    const bug = new URL(issueReportUrl(base, '4.0.0', 'ru', {}, 'bug', words)).searchParams;
    expect([bug.get('template'), bug.get('what'), bug.get('steps'), bug.get('expected')]).toEqual(['2-ru-1-bug.yml', 'The song stops', 'Press Create', 'A whole song']);
    const idea = new URL(issueReportUrl(base, '4.0.0', 'ja', {}, 'idea', { what: 'Slow on long songs', steps: 'A preview first', expected: '' })).searchParams;
    expect([idea.get('template'), idea.get('problem'), idea.get('idea'), idea.has('examples')]).toEqual(['4-ja-2-idea.yml', 'Slow on long songs', 'A preview first', false]);
    const question = new URL(issueReportUrl(base, '4.0.0', 'en', {}, 'question', { what: 'Can it sing in Thai?' })).searchParams;
    expect([question.get('template'), question.get('question'), question.has('what')]).toEqual(['1-en-3-question.yml', 'Can it sing in Thai?', false]);
    for (const [language, order] of LANGUAGES) {
      for (const kind of ['1-bug', '2-idea', '3-question']) {
        const fields = ids(read(`${order}-${language}-${kind}.yml`));
        const used = Object.keys(formWords(kind === '1-bug' ? 'bug' : kind === '2-idea' ? 'idea' : 'question', words));
        expect(used.every(id => fields.includes(id)), `${language}-${kind}`).toBe(true);
      }
    }
  });

  it('titles the issue with the first line of the first field, as the hub does', () => {
    const titled = (what: string, kind: 'bug' | 'idea' | 'question' = 'bug') => new URL(issueReportUrl(base, '4.0.0', 'en', {}, kind, { what })).searchParams;
    expect(titled('  The song stops at decoding\nevery time, after 30 s  ').get('title')).toBe('The song stops at decoding');
    expect(titled('A'.repeat(120)).get('title')).toBe('A'.repeat(80));
    expect(titled('Can it sing in Thai?', 'question').get('title')).toBe('Can it sing in Thai?');
    expect(titled('   ').has('title')).toBe(false);
    expect(new URL(issueReportUrl(base, '4.0.0', 'en', {})).searchParams.has('title')).toBe(false);
    const long = new URL(issueReportUrl(base, '4.0.0', 'ru', {}, 'bug', { what: 'Ж'.repeat(3000), steps: 'Ж'.repeat(3000) })).toString();
    expect(long.length).toBeLessThanOrEqual(MAX_URL);
  });

  it('keeps the link under the limit by cutting the longest field', () => {
    const long = 'Ж'.repeat(5000);
    const url = issueReportUrl(base, '4.0.0', 'ru', {}, 'bug', { what: 'short', steps: long });
    expect(url.length).toBeLessThanOrEqual(MAX_URL);
    const steps = new URL(url).searchParams.get('steps') ?? '';
    expect(steps.endsWith('…') && steps.length < long.length).toBe(true);
    expect(new URL(url).searchParams.get('what')).toBe('short');
  });
});

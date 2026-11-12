import fs from 'fs';
import path from 'path';
import { describe, expect, it } from 'vitest';
import { bugTemplate, issueReportUrl } from './issueReport';

const FORMS = path.resolve(__dirname, '../../.github/ISSUE_TEMPLATE');
const read = (file: string) => fs.readFileSync(path.join(FORMS, file), 'utf-8');
const ids = (form: string) => [...form.matchAll(/^\s+id: (\S+)$/gm)].map(match => match[1]);
const options = (form: string) => [...form.matchAll(/^\s+options:\n((?:\s{8}- .+\n)+)/gm)].map(match => match[1]);

describe('the report link', () => {
  it('opens the bug form of the window language, the English one for the rest', () => {
    for (const [language, file] of [['en', 'en-1-bug.yml'], ['ru', 'ru-1-bug.yml'], ['zh', 'zh-1-bug.yml'], ['ja', 'en-1-bug.yml'], ['ko', 'en-1-bug.yml']]) {
      expect(bugTemplate(language)).toBe(file);
      expect(fs.existsSync(path.join(FORMS, file)), file).toBe(true);
      const url = new URL(issueReportUrl('https://github.com/timoncool/ACE-Step-Studio', '4.0.0', language, { selected_profile_id: 'quality-q8' }));
      expect(url.pathname).toBe('/timoncool/ACE-Step-Studio/issues/new');
      expect(url.searchParams.get('template')).toBe(file);
      expect(url.searchParams.get('version')).toBe('4.0.0');
      expect(url.searchParams.get('models')).toBe('quality-q8');
    }
  });

  it('finds the same fields and the same choices in every language of a form', () => {
    for (const kind of ['1-bug', '2-idea', '3-question']) {
      const english = read(`en-${kind}.yml`);
      for (const language of ['ru', 'zh']) {
        const form = read(`${language}-${kind}.yml`);
        expect(ids(form), `${language}-${kind}`).toEqual(ids(english));
        expect(options(form), `${language}-${kind}`).toEqual(options(english));
        expect(form.match(/^labels: .*$/m)?.[0]).toBe(english.match(/^labels: .*$/m)?.[0]);
      }
    }
    const names = fs.readdirSync(FORMS).filter(file => file !== 'config.yml').map(file => read(file).match(/^name: (.+)$/m)?.[1]);
    expect(new Set(names).size, 'every form has a name of its own').toBe(names.length);
  });
});

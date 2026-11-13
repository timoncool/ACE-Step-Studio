/** What the bug form on GitHub asks about this computer, as far as the studio knows it. */
export interface ReportFacts {
  hardware?: { gpuName?: string; totalVramGb?: number; totalRamGb?: number };
  selected_profile_id?: string | null;
  portable?: boolean;
}

function system(portable: boolean | undefined): string {
  const agent = navigator.userAgent;
  const name = /Windows/.test(agent) ? 'Windows' : /Mac OS X|Macintosh/.test(agent) ? 'macOS' : /Linux/.test(agent) ? 'Linux' : '';
  return portable ? `${name} (portable)`.trim() : name;
}

/** The kinds of the GitHub forms, in their files' order. */
export type IssueKind = 'bug' | 'idea' | 'question';
const KINDS: Record<IssueKind, string> = { bug: '1-bug', idea: '2-idea', question: '3-question' };

/** Each window language's place in GitHub's list of forms: English first. */
const ORDER: Record<string, string> = { en: '1', ru: '2', zh: '3', ja: '4', ko: '5' };

/** The form of `kind` in the window's language, the English one for a language the forms do not have. */
export function issueTemplate(language: string, kind: IssueKind = 'bug'): string {
  const lang = ORDER[language] ? language : 'en';
  return `${ORDER[lang]}-${lang}-${KINDS[kind]}.yml`;
}

/** The bug form in the window's language. */
export function bugTemplate(language: string): string {
  return issueTemplate(language, 'bug');
}

/**
 * The bug form in the window's language with what the studio knows passed
 * along. The keys are the `id`s of the text fields in
 * `.github/ISSUE_TEMPLATE/*.yml`, the same in every language; the form fills a
 * field from the query parameter of the same name.
 */
/** What the person wrote in the Report a problem window: the first field, the second and the third of its kind. */
export interface ReportWords {
  what?: string;
  steps?: string;
  expected?: string;
}

/** The longest link to the GitHub form the window opens. */
export const MAX_URL = 7000;

/**
 * The window's three fields under the ids of the GitHub form of `kind`, the same in every language: a bug's
 * what, steps and expected; an idea's problem, idea and examples; a question's question.
 */
export function formWords(kind: IssueKind, words?: ReportWords): Record<string, string> {
  const { what = '', steps = '', expected = '' } = words ?? {};
  switch (kind) {
    case 'bug':
      return { what, steps, expected };
    case 'idea':
      return { problem: what, idea: steps, examples: expected };
    default:
      return { question: what };
  }
}

export function issueReportUrl(repositoryUrl: string, version: string, language: string, facts?: ReportFacts, kind: IssueKind = 'bug', words?: ReportWords): string {
  const query = new URLSearchParams({ template: issueTemplate(language, kind), version });
  const os = system(facts?.portable);
  if (os) query.set('os', os);
  const hardware = facts?.hardware;
  if (hardware?.gpuName) {
    const memory = [
      hardware.totalVramGb ? `${Math.round(hardware.totalVramGb)} GB` : '',
      hardware.totalRamGb ? `RAM ${Math.round(hardware.totalRamGb)} GB` : '',
    ].filter(Boolean).join(', ');
    query.set('gpu', memory ? `${hardware.gpuName}, ${memory}` : hardware.gpuName);
  }
  if (facts?.selected_profile_id) query.set('models', facts.selected_profile_id);
  // the issue's title as the hub makes it for a report without an account: the first line of the first field
  const title = (words?.what ?? '').trim().split('\n')[0].trim().slice(0, 80);
  if (title) query.set('title', title);
  const typed: [string, string][] = Object.entries(formWords(kind, words)).filter(([, text]) => text.trim() !== '').map(([id, text]) => [id, text.trim()]);
  const address = () => {
    const full = new URLSearchParams(query);
    for (const [id, text] of typed) full.set(id, text);
    return `${repositoryUrl}/issues/new?${full.toString()}`;
  };
  // a link GitHub takes stays under MAX_URL: the longest of the person's fields is cut, with an ellipsis, until it fits
  let url = address();
  while (url.length > MAX_URL && typed.length > 0) {
    const longest = typed.reduce((most, field) => (field[1].length > most[1].length ? field : most));
    const over = Math.ceil((url.length - MAX_URL) / 3) + 1;
    longest[1] = `${longest[1].slice(0, Math.max(0, longest[1].length - 1 - over))}…`;
    url = address();
  }
  return url;
}

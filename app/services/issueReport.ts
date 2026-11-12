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
export function issueReportUrl(repositoryUrl: string, version: string, language: string, facts?: ReportFacts, kind: IssueKind = 'bug'): string {
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
  return `${repositoryUrl}/issues/new?${query.toString()}`;
}

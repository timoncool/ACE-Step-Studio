// Builds the changelog the news page shows, from the history itself.
//
// The page used to fetch /api/changelog from the Node backend this fork does
// not have, and nothing hand-written stays in step with the code. This reads
// git and writes app/data/changelog.json before the frontend is built.
import { execFileSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';

const FIELD = '';
const RECORD = '';

// Where this project starts. The repository carries ACE-Step Studio's whole
// history because it is a fork of it, but the changelog is a product changelog:
// a user reading it wants to know what happened to *this* studio, not what
// happened to a Python application whose code is no longer here.
const FIRST_COMMIT = '1b70091';

const raw = execFileSync(
  'git',
  ['log', '--date=short', `--pretty=format:%h${FIELD}%ad${FIELD}%s${FIELD}%b${RECORD}`, `${FIRST_COMMIT}^..HEAD`],
  { encoding: 'utf8' },
);

const entries = raw
  .split(RECORD)
  .map((chunk) => chunk.replace(/^\n/, '').split(FIELD))
  .filter((parts) => parts.length >= 3 && parts[0].trim())
  .map(([hash, date, subject, body = '']) => ({
    hash: hash.trim(),
    date: date.trim(),
    subject: subject.trim(),
    // The first paragraph of the message is the part written for a reader.
    body: body.trim().split('\n\n')[0]?.trim() ?? '',
  }))
  // Merge commits describe the repository, not the product.
  .filter((entry) => entry.subject && !entry.subject.startsWith('Merge '));

const byDate = new Map();
for (const entry of entries) {
  if (!byDate.has(entry.date)) byDate.set(entry.date, []);
  byDate.get(entry.date).push(entry);
}

const changelog = [...byDate.entries()].map(([date, items]) => ({ date, items }));
writeFileSync(new URL('../app/data/changelog.json', import.meta.url), `${JSON.stringify(changelog, null, 2)}\n`);
console.log(`changelog: ${entries.length} commits across ${changelog.length} days`);

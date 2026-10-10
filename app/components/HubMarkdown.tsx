import React from 'react';

/** The hub's markdown subset: **bold**, *italic*, [text](https://...), "- " lists, blank-line paragraphs. No HTML. */
export function hubInline(text: string, key: string, linkClass = 'underline underline-offset-2 hover:opacity-90'): React.ReactNode[] {
  const out: React.ReactNode[] = [];
  const pattern = /\*\*([^*]+)\*\*|\*([^*]+)\*|\[([^\]]+)\]\((https:\/\/[^)\s]+)\)/g;
  let last = 0;
  let index = 0;
  for (const match of text.matchAll(pattern)) {
    if (match.index > last) out.push(text.slice(last, match.index));
    const id = `${key}-${index++}`;
    if (match[1]) out.push(<strong key={id} className="font-semibold">{match[1]}</strong>);
    else if (match[2]) out.push(<em key={id}>{match[2]}</em>);
    else out.push(<a key={id} href={match[4]} target="_blank" rel="noopener noreferrer" className={linkClass}>{match[3]}</a>);
    last = match.index + match[0].length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

export const HubMarkdown: React.FC<{ text: string; className?: string }> = ({ text, className }) => (
  <div className={className}>
    {text.split(/\n\s*\n/).filter((block) => block.trim()).map((block, b) => {
      const lines = block.split('\n').filter((line) => line.trim());
      if (lines.every((line) => line.trimStart().startsWith('- '))) {
        return (
          <ul key={b} className="list-disc space-y-1 pl-5">
            {lines.map((line, l) => <li key={l}>{hubInline(line.trimStart().slice(2), `${b}-${l}`)}</li>)}
          </ul>
        );
      }
      return <p key={b}>{hubInline(lines.join(' '), String(b))}</p>;
    })}
  </div>
);

import React from 'react';

/** The author's news channels: one list for About and for Report a problem. */
export const AUTHOR_NEWS = [
  { url: 'https://t.me/nerual_dreming', label: 'Telegram · @nerual_dreming', className: 'bg-[#2AABEE]' },
  { url: 'https://x.com/nerualdreming', label: 'X · @nerualdreming', className: 'bg-black' },
] as const;

export const AuthorNewsLinks: React.FC = () => (
  <>
    {AUTHOR_NEWS.map(link => (
      <a key={link.url} href={link.url} target="_blank" rel="noopener noreferrer" className={`rounded-lg ${link.className} px-3 py-1.5 text-xs font-medium text-white`}>{link.label}</a>
    ))}
  </>
);

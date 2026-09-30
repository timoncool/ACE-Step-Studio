/** An icon button's name: what a screen reader says and the tooltip shows, in the same words. */
export function named(text: string): { 'aria-label': string; title: string } {
  return { 'aria-label': text, title: text };
}

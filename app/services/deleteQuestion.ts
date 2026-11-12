type Translate = (key: 'deleteSongConfirm' | 'deleteSongsConfirm') => string;

/// The question before songs go to the bin, with the days the service keeps them there (`trash_days` of the setup
/// status), so asking it reads nothing.
export function deleteQuestion(t: Translate, songs: { title: string }[], days: number): string {
  const text = songs.length === 1
    ? t('deleteSongConfirm').replace('{title}', () => songs[0].title)
    : t('deleteSongsConfirm').replace('{count}', String(songs.length));
  return text.replace('{days}', String(days));
}

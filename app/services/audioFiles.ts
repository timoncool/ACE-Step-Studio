/** Every audio file the studio's decoder reads (audio_pcm::AUDIO_EXTENSIONS); Opus is not read. */
export const AUDIO_EXTENSIONS = ['mp3', 'mp2', 'mp1', 'wav', 'flac', 'ogg', 'oga', 'm4a', 'mp4', 'aac', 'aiff', 'aif', 'aifc', 'caf', 'mka'] as const;

/** The file picker's filter for audio the studio reads. */
export const AUDIO_ACCEPT = ['audio/*', ...AUDIO_EXTENSIONS.map((extension) => `.${extension}`)].join(',');

import { en } from './en';
import { zh } from './zh';
import { ja } from './ja';
import { ko } from './ko';
import { ru } from './ru';
import { processingStrings } from './processing';
import { adapterStrings } from './adapters';
import { trainingStrings } from './training';
import { hubStrings } from './hub';
import { aceStrings } from './ace';
import { midiEditorStrings } from './midiEditor';

export type Language = 'en' | 'zh' | 'ja' | 'ko' | 'ru';

const enAll = { ...en, ...processingStrings.en, ...adapterStrings.en, ...trainingStrings.en, ...hubStrings.en, ...aceStrings.en, ...midiEditorStrings.en };

export type TranslationKey = keyof typeof enAll;

export const translations: Record<Language, Partial<Record<TranslationKey, string>>> = {
  en: enAll,
  zh: { ...zh, ...processingStrings.zh, ...adapterStrings.zh, ...trainingStrings.zh, ...hubStrings.zh, ...aceStrings.zh, ...midiEditorStrings.zh },
  ja: { ...ja, ...processingStrings.ja, ...adapterStrings.ja, ...trainingStrings.ja, ...hubStrings.ja, ...aceStrings.ja, ...midiEditorStrings.ja },
  ko: { ...ko, ...processingStrings.ko, ...adapterStrings.ko, ...trainingStrings.ko, ...hubStrings.ko, ...aceStrings.ko, ...midiEditorStrings.ko },
  ru: { ...ru, ...processingStrings.ru, ...adapterStrings.ru, ...trainingStrings.ru, ...hubStrings.ru, ...aceStrings.ru, ...midiEditorStrings.ru },
};

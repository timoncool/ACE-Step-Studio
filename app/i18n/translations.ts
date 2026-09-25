import { en } from './en';
import { zh } from './zh';
import { ja } from './ja';
import { ko } from './ko';
import { ru } from './ru';
import { processingStrings } from './processing';
import { adapterStrings } from './adapters';
import { trainingStrings } from './training';
import { aceStrings } from './ace';

export type Language = 'en' | 'zh' | 'ja' | 'ko' | 'ru';

const enAll = { ...en, ...processingStrings.en, ...adapterStrings.en, ...trainingStrings.en, ...aceStrings.en };

export type TranslationKey = keyof typeof enAll;

export const translations: Record<Language, Partial<Record<TranslationKey, string>>> = {
  en: enAll,
  zh: { ...zh, ...processingStrings.zh, ...adapterStrings.zh, ...trainingStrings.zh, ...aceStrings.zh },
  ja: { ...ja, ...processingStrings.ja, ...adapterStrings.ja, ...trainingStrings.ja, ...aceStrings.ja },
  ko: { ...ko, ...processingStrings.ko, ...adapterStrings.ko, ...trainingStrings.ko, ...aceStrings.ko },
  ru: { ...ru, ...processingStrings.ru, ...adapterStrings.ru, ...trainingStrings.ru, ...aceStrings.ru },
};

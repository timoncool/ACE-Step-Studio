import React, { createContext, useContext, useState, ReactNode } from 'react';
import { translations, Language, TranslationKey } from '../i18n/translations';
import { STUDIO } from '../studio';

interface I18nContextType {
  language: Language;
  setLanguage: (lang: Language) => void;
  t: (key: TranslationKey) => string;
  /** "1 song", "3 песни", "61 песня": the form each language asks for. */
  songCount: (count: number) => string;
}

// One context object for the life of the page: when a strings file changes in
// development, this module is evaluated again, and a second context would leave
// every consumer outside the provider that is already mounted.
const I18nContext: React.Context<I18nContextType | undefined> =
  import.meta.hot?.data.i18nContext ?? createContext<I18nContextType | undefined>(undefined);
if (import.meta.hot) import.meta.hot.data.i18nContext = I18nContext;

export const I18nProvider: React.FC<{ children: ReactNode }> = ({ children }) => {
  const [language, setLanguage] = useState<Language>(() => {
    const stored = localStorage.getItem('language') as Language;
    if (stored === 'zh' || stored === 'en' || stored === 'ja' || stored === 'ko' || stored === 'ru') return stored;
    if (typeof navigator !== 'undefined' && navigator.language?.startsWith('ru')) return 'ru';
    return 'en';
  });

  const handleSetLanguage = (lang: Language) => {
    setLanguage(lang);
    localStorage.setItem('language', lang);
  };

  // `{studio}` is the studio's own name, so the strings are shared by every studio.
  const t = (key: TranslationKey): string => (translations[language][key] || key).replaceAll('{studio}', STUDIO.name);

  const songCount = (count: number): string =>
    t(`songCount_${new Intl.PluralRules(language).select(count)}` as TranslationKey).replace('{count}', String(count));

  return (
    <I18nContext.Provider value={{ language, setLanguage: handleSetLanguage, t, songCount }}>
      {children}
    </I18nContext.Provider>
  );
};

export const useI18n = () => {
  const context = useContext(I18nContext);
  if (!context) {
    throw new Error('useI18n must be used within I18nProvider');
  }
  return context;
};

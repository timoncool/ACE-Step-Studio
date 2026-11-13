import React from 'react';
import { CreditCard, ExternalLink } from 'lucide-react';
import { SiBoosty } from '@icons-pack/react-simple-icons';
import { useI18n } from '../context/I18nContext';
import { ABOUT_LINK } from './AuthorLinks';

/** Support the author: a card, a monthly subscription, and the donation page of the studio's site for the rest. */
export const SupportAuthor: React.FC<{ donatePage: string }> = ({ donatePage }) => {
  const { t, language } = useI18n();
  const page = new URL(donatePage);
  page.searchParams.set('from', 'app');
  page.searchParams.set('lang', language);
  return (
    <div className="flex flex-wrap gap-2">
      <a href="https://dalink.to/nerual_dreming" target="_blank" rel="noopener noreferrer" className={ABOUT_LINK}><CreditCard size={14} aria-hidden="true" />{t('donateCardButton')}</a>
      <a href="https://boosty.to/neuro_art" target="_blank" rel="noopener noreferrer" className={ABOUT_LINK}><SiBoosty size={14} aria-hidden="true" />{t('donateMonthlyButton')}</a>
      <a href={page.href} target="_blank" rel="noopener noreferrer" className={ABOUT_LINK}>{t('supportPageButton')}<ExternalLink size={13} aria-hidden="true" /></a>
    </div>
  );
};

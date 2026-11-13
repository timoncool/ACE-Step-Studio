import React, { useState } from 'react';
import { Check, Copy, CreditCard, ExternalLink } from 'lucide-react';
import { SiBoosty } from '@icons-pack/react-simple-icons';
import { useI18n } from '../context/I18nContext';
import { ABOUT_LINK } from './AuthorLinks';

/** The author's wallets, as the donation page lists them, in its order. */
export const WALLETS: { coin: string; network: string; address: string; note?: 'donateUsdtNote' }[] = [
  { coin: 'USDT', network: 'TRON (TRC20)', address: 'TQST9Lp2TjK6FiVkn4fwfGUee7NmkxEE7C', note: 'donateUsdtNote' },
  { coin: 'BTC', network: 'Bitcoin', address: '1E7dHL22RpyhJGVpcvKdbyZgksSYkYeEBC' },
  { coin: 'ETH', network: 'Ethereum (ERC20)', address: '0xb5db65adf478983186d4897ba92fe2c25c594a0c' },
];

const Wallet: React.FC<{ wallet: (typeof WALLETS)[number] }> = ({ wallet }) => {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  const copy = () => {
    void navigator.clipboard.writeText(wallet.address).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    });
  };
  return (
    <div className="space-y-1">
      <p className="text-xs text-zinc-500 dark:text-zinc-400">{wallet.coin} · {wallet.network}</p>
      <div className="flex items-center gap-2">
        <code className="min-w-0 flex-1 select-all break-all rounded-md bg-zinc-100 px-2 py-1 font-mono text-[11px] text-zinc-700 dark:bg-black/30 dark:text-zinc-300">{wallet.address}</code>
        <button type="button" onClick={copy} className={ABOUT_LINK}>
          {copied ? <Check size={13} className="text-emerald-500" /> : <Copy size={13} />}
          {copied ? t('donateCopied') : t('donateCopy')}
        </button>
      </div>
      {wallet.note && <p className="text-[11px] text-zinc-500 dark:text-zinc-400">{t(wallet.note)}</p>}
    </div>
  );
};

/** Support the author, as the donation page of the studio's site has it: a card, a monthly subscription, crypto. */
export const SupportAuthor: React.FC<{ donatePage: string }> = ({ donatePage }) => {
  const { t } = useI18n();
  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <p className="text-xs font-medium text-zinc-900 dark:text-white">{t('donateCardTitle')}</p>
        <a href="https://dalink.to/nerual_dreming" target="_blank" rel="noopener noreferrer" className={ABOUT_LINK}><CreditCard size={14} />{t('donateCardButton')}</a>
      </div>
      <div className="space-y-2">
        <p className="text-xs font-medium text-zinc-900 dark:text-white">{t('donateMonthlyTitle')}</p>
        <a href="https://boosty.to/neuro_art" target="_blank" rel="noopener noreferrer" className={ABOUT_LINK}><SiBoosty size={14} />{t('donateMonthlyButton')}</a>
      </div>
      <div className="space-y-3">
        <p className="text-xs font-medium text-zinc-900 dark:text-white">{t('donateCryptoTitle')}</p>
        {WALLETS.map(wallet => <Wallet key={wallet.coin} wallet={wallet} />)}
      </div>
      <a href={donatePage} target="_blank" rel="noopener noreferrer" className="inline-flex items-center gap-1 text-xs text-zinc-500 underline underline-offset-2 hover:text-pink-500 dark:text-zinc-400"><ExternalLink size={12} />{t('donateAllWays')}</a>
    </div>
  );
};

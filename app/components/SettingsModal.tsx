import React, { useEffect, useRef, useState } from 'react';
import { Boxes, Cloud, Cpu, Github, Image as ImageIcon, Info, Monitor, Plug, User as UserIcon, X } from 'lucide-react';
import { useAuth } from '../context/AuthContext';
import { useI18n } from '../context/I18nContext';
import type { Language } from '../i18n/translations';
import { ProviderSettings } from './ProviderSettings';
import { EngineSettings } from './EngineSettings';
import { SetupGate } from './SetupGate';
import { CoverTemplateSettings } from './CoverTemplateSettings';
import { AgentPanel } from './AgentPanel';
import { STUDIO, STUDIO_REPO_URL } from '../studio';

/**
 * Settings.
 *
 * This began as one long scroll of unrelated panels, where finding the engine
 * flags meant scrolling past the display name and the language picker. It is a
 * two-pane dialog now: sections on the left, one at a time on the right,
 * grouped by what a setting actually governs - what runs on this machine, what
 * runs in the cloud, the two optional extras, and the interface.
 */

/// The assistant and the karaoke recogniser are not sections of their own: they
/// are set up where they are installed, on the models page. Having both was one
/// capability drawn twice, and the two drawings disagreed.
type SectionId = 'account' | 'models' | 'engine' | 'cloud' | 'agent' | 'covers' | 'interface' | 'about';

const INPUT =
  'w-full rounded-lg border-2 border-zinc-300 bg-white px-4 py-2.5 text-sm font-medium text-zinc-900 outline-none focus:border-indigo-500 dark:border-zinc-700 dark:bg-zinc-800 dark:text-white';

interface SettingsModalProps {
  isOpen: boolean;
  /// Which page to land on, when the caller has one in mind.
  initialSection?: string | null;
  onClose: () => void;
  theme: 'light' | 'dark';
  onToggleTheme: () => void;
}

export const SettingsModal: React.FC<SettingsModalProps> = ({ isOpen, initialSection, onClose, theme, onToggleTheme }) => {
  const { user, setDisplayName } = useAuth();
  const { t, language, setLanguage } = useI18n();
  const [section, setSection] = useState<SectionId>('account');
  /// A part of the page to open at, after a colon: `models:assistant`.
  const [focus, setFocus] = useState<string | null>(null);
  // Opened from somewhere with a page in mind - the profile line, say.
  useEffect(() => {
    // Only a section that exists. Two of them were removed - the assistant and
    // karaoke are set up where they are installed now - and a button still
    // pointed at one, which silently opened whatever was first instead.
    const [page, part] = (initialSection ?? '').split(':');
    if (page && sections.some((entry) => entry.id === page)) {
      setSection(page as SectionId);
      setFocus(part || null);
    }
  }, [initialSection]);
  const [showLangInfo, setShowLangInfo] = useState(false);
  const langInfoRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!showLangInfo) return;
    const handleClick = (event: MouseEvent) => {
      if (langInfoRef.current && !langInfoRef.current.contains(event.target as Node)) setShowLangInfo(false);
    };
    document.addEventListener('mousedown', handleClick);
    return () => document.removeEventListener('mousedown', handleClick);
  }, [showLangInfo]);

  if (!isOpen) return null;

  const sections: Array<{ id: SectionId; label: string; hint: string; icon: React.ReactNode }> = [
    { id: 'account', label: t('account'), hint: t('accountSectionHint'), icon: <UserIcon size={16} /> },
    { id: 'models', label: t('modelsSection'), hint: t('modelsSectionHint'), icon: <Boxes size={16} /> },
    { id: 'engine', label: t('localEngine'), hint: t('engineSectionHint'), icon: <Cpu size={16} /> },
    { id: 'cloud', label: t('providers'), hint: t('cloudSectionHint'), icon: <Cloud size={16} /> },
    { id: 'agent', label: t('agentSection'), hint: t('agentSectionHint'), icon: <Plug size={16} /> },
    { id: 'covers', label: t('coversSection'), hint: t('coversSectionHint'), icon: <ImageIcon size={16} /> },
    { id: 'interface', label: t('appearance'), hint: t('interfaceSectionHint'), icon: <Monitor size={16} /> },
    { id: 'about', label: t('about'), hint: t('aboutSectionHint'), icon: <Info size={16} /> },
  ];

  const active = sections.find((entry) => entry.id === section) ?? sections[0];

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50 p-4" onClick={onClose}>
      <div
        className="flex h-[85vh] w-full max-w-4xl overflow-hidden rounded-2xl bg-white shadow-2xl dark:bg-zinc-900"
        onClick={(event) => event.stopPropagation()}
      >
        <nav className="hidden w-60 shrink-0 flex-col border-r border-zinc-200 bg-zinc-50 p-3 dark:border-white/5 dark:bg-black/20 sm:flex">
          <h2 className="px-2 pb-3 pt-2 text-lg font-bold text-zinc-900 dark:text-white">{t('settings')}</h2>
          <div className="space-y-1">
            {sections.map((entry) => (
              <button
                key={entry.id}
                type="button"
                onClick={() => setSection(entry.id)}
                className={`flex w-full items-start gap-2.5 rounded-lg px-2.5 py-2 text-left transition-colors ${
                  section === entry.id
                    ? 'bg-white text-zinc-900 shadow-sm dark:bg-zinc-800 dark:text-white'
                    : 'text-zinc-600 hover:bg-zinc-200/60 dark:text-zinc-400 dark:hover:bg-white/5'
                }`}
              >
                <span className="mt-0.5 shrink-0">{entry.icon}</span>
                <span className="min-w-0">
                  <span className="block truncate text-sm font-semibold">{entry.label}</span>
                  <span className="block truncate text-[11px] text-zinc-500 dark:text-zinc-400">{entry.hint}</span>
                </span>
              </button>
            ))}
          </div>
        </nav>

        <div className="flex min-w-0 flex-1 flex-col">
          <div className="flex items-center justify-between border-b border-zinc-200 px-6 py-4 dark:border-white/5">
            <div className="min-w-0">
              <h3 className="truncate text-lg font-bold text-zinc-900 dark:text-white">{active.label}</h3>
              <p className="truncate text-xs text-zinc-500 dark:text-zinc-400">{active.hint}</p>
            </div>
            <button onClick={onClose} className="rounded-full p-2 transition-colors hover:bg-zinc-100 dark:hover:bg-white/5">
              <X size={20} className="text-zinc-500" />
            </button>
          </div>

          {/* On a narrow window the sections become a row of chips. */}
          <div className="flex gap-1 overflow-x-auto border-b border-zinc-200 px-3 py-2 dark:border-white/5 sm:hidden">
            {sections.map((entry) => (
              <button
                key={entry.id}
                type="button"
                onClick={() => setSection(entry.id)}
                className={`shrink-0 rounded-full px-3 py-1.5 text-xs font-semibold ${section === entry.id ? 'bg-zinc-900 text-white dark:bg-white dark:text-black' : 'bg-zinc-100 text-zinc-600 dark:bg-white/5 dark:text-zinc-300'}`}
              >
                {entry.label}
              </button>
            ))}
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto p-6">
            {section === 'account' && (
              <div className="max-w-lg space-y-2">
                <label className="text-sm text-zinc-500 dark:text-zinc-400">{t('username')}</label>
                <input defaultValue={user.username} onBlur={(event) => setDisplayName(event.target.value)} className={INPUT} />
                <p className="text-xs text-zinc-400">{t('displayNameHint')}</p>
              </div>
            )}

            {/* The same chooser as the first run, because it is the same
                decision - only now nothing is waiting on it. */}
            {section === 'covers' && <CoverTemplateSettings />}
            {section === 'models' && <div className="-m-6"><SetupGate mode="settings" focus={focus} /></div>}
            {section === 'engine' && <EngineSettings />}
            {section === 'cloud' && <ProviderSettings />}
            {section === 'agent' && <AgentPanel />}

            {section === 'interface' && (
              <div className="max-w-lg space-y-6">
                <div className="space-y-3">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-semibold text-zinc-900 dark:text-white">{t('language')}</span>
                    <div className="relative" ref={langInfoRef}>
                      <button
                        onClick={() => setShowLangInfo(!showLangInfo)}
                        className="rounded-full p-1 text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-zinc-600 dark:hover:bg-white/10 dark:hover:text-zinc-300"
                      >
                        <Info size={14} />
                      </button>
                      {showLangInfo && (
                        <div className="absolute left-0 top-8 z-10 w-64 rounded-xl border border-zinc-200 bg-white p-3 shadow-xl dark:border-zinc-700 dark:bg-zinc-800">
                          <p className="mb-2 text-xs text-zinc-500 dark:text-zinc-400">{t('localizedBy')}</p>
                          <div className="flex flex-wrap gap-1.5">
                            <a href="https://x.com/bdsqlsz" target="_blank" rel="noopener noreferrer" className="rounded-lg bg-black px-2.5 py-1.5 text-xs font-medium text-white dark:bg-white dark:text-black">@bdsqlsz</a>
                            <a href="https://space.bilibili.com/219296" target="_blank" rel="noopener noreferrer" className="rounded-lg bg-[#00A1D6] px-2.5 py-1.5 text-xs font-medium text-white">青龙圣者</a>
                          </div>
                        </div>
                      )}
                    </div>
                  </div>
                  <select value={language} onChange={(event) => setLanguage(event.target.value as Language)} className={INPUT}>
                    <option value="ru">{t('russianLanguage')}</option>
                    <option value="en">{t('english')}</option>
                    <option value="zh">{t('chinese')}</option>
                    <option value="ja">{t('japaneseLanguage')}</option>
                    <option value="ko">{t('koreanLanguage')}</option>
                  </select>
                </div>

                <div className="space-y-3">
                  <span className="text-sm font-semibold text-zinc-900 dark:text-white">{t('appearance')}</span>
                  <div className="flex gap-3">
                    <button
                      onClick={theme === 'dark' ? onToggleTheme : undefined}
                      className={`flex-1 rounded-lg border-2 px-4 py-3 font-medium transition-colors ${theme === 'light' ? 'border-indigo-500 bg-indigo-50 text-indigo-700' : 'border-zinc-300 dark:border-zinc-700'}`}
                    >
                      {t('light')}
                    </button>
                    <button
                      onClick={theme === 'light' ? onToggleTheme : undefined}
                      className={`flex-1 rounded-lg border-2 px-4 py-3 font-medium transition-colors ${theme === 'dark' ? 'border-indigo-500 bg-indigo-950 text-indigo-300' : 'border-zinc-300 dark:border-zinc-700'}`}
                    >
                      {t('dark')}
                    </button>
                  </div>
                </div>
              </div>
            )}

            {section === 'about' && (
              <div className="max-w-lg space-y-6 text-sm text-zinc-600 dark:text-zinc-400">
                <div className="space-y-1">
                  <p className="text-zinc-900 dark:text-white">{STUDIO.name} · {t('version')} {__APP_VERSION__}</p>
                  <p>{t('localAIMusicGenerator')}</p>
                  <p className="text-xs text-zinc-400 dark:text-zinc-500">{t('poweredBy')}</p>
                </div>

                {/* Whose studio this is, and where to find the rest of it. */}
                <div className="space-y-3 border-t border-zinc-200 pt-4 dark:border-zinc-700/50">
                  <p className="font-medium text-zinc-900 dark:text-white">Nerual Dreming</p>
                  <p className="text-xs leading-5">{t('authorLine')}</p>
                  <div className="flex flex-wrap gap-2">
                    <a href="https://t.me/nerual_dreming" target="_blank" rel="noopener noreferrer" className="rounded-lg bg-[#2AABEE] px-3 py-1.5 text-xs font-medium text-white">Telegram · @nerual_dreming</a>
                    <a href="https://t.me/neuroport" target="_blank" rel="noopener noreferrer" className="rounded-lg bg-[#2AABEE]/80 px-3 py-1.5 text-xs font-medium text-white">Telegram · @neuroport</a>
                    <a href="https://github.com/timoncool" target="_blank" rel="noopener noreferrer" className="inline-flex items-center gap-1.5 rounded-lg bg-zinc-800 px-3 py-1.5 text-xs font-medium text-white dark:bg-zinc-700">
                      <Github size={14} />timoncool
                    </a>
                    <a href="https://neuro-cartel.com" target="_blank" rel="noopener noreferrer" className="rounded-lg border border-zinc-300 px-3 py-1.5 text-xs font-medium text-zinc-700 dark:border-zinc-600 dark:text-zinc-200">neuro-cartel.com</a>
                    <a href="https://artgeneration.me" target="_blank" rel="noopener noreferrer" className="rounded-lg border border-zinc-300 px-3 py-1.5 text-xs font-medium text-zinc-700 dark:border-zinc-600 dark:text-zinc-200">ArtGeneration.me</a>
                  </div>
                </div>

                {/* Support, in the author's own words: the software is free. */}
                <div className="space-y-3 border-t border-zinc-200 pt-4 dark:border-zinc-700/50">
                  <p className="font-medium text-zinc-900 dark:text-white">{t('supportTheAuthor')}</p>
                  <p className="text-xs leading-5">{t('supportLine')}</p>
                  <div className="flex flex-wrap gap-2">
                    <a href="https://boosty.to/neuro_art" target="_blank" rel="noopener noreferrer" className="rounded-lg bg-[#F15F2C] px-3 py-1.5 text-xs font-semibold text-white">Boosty</a>
                    <a href="https://dalink.to/nerual_dreming" target="_blank" rel="noopener noreferrer" className="rounded-lg border border-zinc-300 px-3 py-1.5 text-xs font-medium text-zinc-700 dark:border-zinc-600 dark:text-zinc-200">{t('allLinks')}</a>
                  </div>
                  <div className="space-y-1 rounded-lg bg-zinc-100 p-3 font-mono text-[11px] text-zinc-600 dark:bg-black/30 dark:text-zinc-400">
                    <div><span className="text-zinc-400">BTC</span> 1E7dHL22RpyhJGVpcvKdbyZgksSYkYeEBC</div>
                    <div><span className="text-zinc-400">ETH</span> 0xb5db65adf478983186d4897ba92fe2c25c594a0c</div>
                    <div><span className="text-zinc-400">USDT · TRC20</span> TQST9Lp2TjK6FiVkn4fwfGUee7NmkxEE7C</div>
                  </div>
                </div>

                <div className="space-y-2 border-t border-zinc-200 pt-4 dark:border-zinc-700/50">
                  <p className="font-medium text-zinc-900 dark:text-white">{t('thisStudio')}</p>
                  <div className="flex flex-wrap gap-2">
                    <a href={STUDIO_REPO_URL} target="_blank" rel="noopener noreferrer" className="inline-flex items-center gap-1.5 rounded-lg bg-zinc-800 px-3 py-1.5 text-xs font-medium text-white dark:bg-zinc-700">
                      <Github size={14} />{STUDIO.name}
                    </a>
                    <a href={`${STUDIO_REPO_URL}/issues`} target="_blank" rel="noopener noreferrer" className="rounded-lg border border-zinc-300 px-3 py-1.5 text-xs font-medium text-zinc-700 dark:border-zinc-600 dark:text-zinc-200">{t('reportIssues')}</a>
                  </div>
                </div>
              </div>
            )}
          </div>

          <div className="flex justify-end border-t border-zinc-200 px-6 py-4 dark:border-white/5">
            <button onClick={onClose} className="rounded-lg bg-zinc-900 px-6 py-2 font-semibold text-white transition-colors hover:bg-zinc-800 dark:bg-white dark:text-black dark:hover:bg-zinc-200">
              {t('done')}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};

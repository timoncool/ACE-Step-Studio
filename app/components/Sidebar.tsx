import React, { useEffect, useState } from 'react';
import { Disc, Layers, Library, Moon, Newspaper, Search, SlidersHorizontal, Sun } from 'lucide-react';
import { View } from '../types';
import { useI18n } from '../context/I18nContext';
import { ResourceMonitor } from './ResourceMonitor';
import { STUDIO } from '../studio';
import { profileLabel } from '../services/modelCatalog';

interface SidebarProps {
  currentView: View;
  onNavigate: (view: View) => void;
  theme: 'light' | 'dark';
  onToggleTheme: () => void;
  user?: { username: string; isAdmin?: boolean; avatar_url?: string } | null;
  onOpenSettings?: () => void;
  isOpen?: boolean;
  onToggle?: () => void;
}

type NativeSetupStatus = {
  ready: boolean;
  engine_ready: boolean;
  engine_id: string;
  selected_profile_id?: string | null;
  selected_component_ids?: string[] | null;
};

const StatusLine: React.FC<{ active: boolean; pending?: boolean; label: string }> = ({ active, pending, label }) => (
  <div className="flex min-w-0 items-center gap-1.5 text-zinc-500 dark:text-zinc-400" title={label}>
    <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${active ? 'bg-emerald-500' : pending ? 'animate-pulse bg-amber-400' : 'bg-zinc-400 dark:bg-zinc-600'}`} />
    <span className="truncate">{label}</span>
  </div>
);

const SystemWidget: React.FC<{ isOpen?: boolean }> = ({ isOpen }) => {
  const { t } = useI18n();
  const [setup, setSetup] = useState<NativeSetupStatus | null>(null);
  const [unreachable, setUnreachable] = useState(false);
  const [openRouter, setOpenRouter] = useState<{ configured?: boolean } | null>(null);

  useEffect(() => {
    const refresh = async () => {
      try {
        const response = await fetch('/setup/status');
        if (!response.ok) throw new Error(`Setup status ${response.status}`);
        setSetup(await response.json());
        setUnreachable(false);
      } catch {
        setSetup(null);
        setUnreachable(true);
      }
    };
    void refresh();
    const timer = window.setInterval(() => void refresh(), 5000);
    return () => window.clearInterval(timer);
  }, []);

  // Settings are intentionally local to the user. Polling causes this compact
  // status to reflect a configuration change made in Settings without reload.
  useEffect(() => {
    const refresh = () => void fetch('/v1/openrouter/settings')
      .then((response) => response.ok ? response.json() : null)
      .then(setOpenRouter)
      .catch(() => setOpenRouter(null));
    refresh();
    const timer = window.setInterval(refresh, 15000);
    return () => window.clearInterval(timer);
  }, []);

  // The credential lives in the native server, never in browser storage, so
  // the badge asks the server whether one is configured.
  const openRouterConfigured = openRouter?.configured === true;
  const engineReady = setup?.engine_ready === true;
  const modelReady = setup?.ready === true;
  const profile = setup?.selected_profile_id
    ? profileLabel(t, { id: setup.selected_profile_id, label: setup.selected_profile_id })
    : (setup?.selected_component_ids?.length ? t('customSet') : t('noProfile'));
  // Without a model set on disk the engine is not starting, it is waiting.
  const engineLabel = unreachable ? t('engineUnavailable') : engineReady ? t('engineReachable') : modelReady ? t('engineStarting') : t('engineWaitsForModels');
  const modelLabel = modelReady ? `${t('profileReady')}: ${profile}` : t('profileNotInstalled');

  if (!isOpen) {
    return (
      <div className="flex w-full justify-center py-2" title={`${engineLabel} · ${modelLabel}`}>
        <span className={`h-2 w-2 rounded-full ${engineReady && modelReady ? 'bg-emerald-500' : unreachable ? 'bg-rose-500' : 'animate-pulse bg-amber-400'}`} />
      </div>
    );
  }

  // With nobody answering, the model and provider lines would be guesses. Say
  // the one thing that is actually known: the service is not responding.
  if (unreachable) {
    return (
      <div className="space-y-1.5 rounded-xl border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-[10px]">
        <StatusLine active={false} label={t('serviceUnavailable')} />
      </div>
    );
  }

  return (
    <div className="space-y-1.5 rounded-xl border border-zinc-200/70 bg-zinc-50/80 px-3 py-2 text-[10px] dark:border-white/5 dark:bg-zinc-900/50">
      <StatusLine active={engineReady && modelReady} pending={!unreachable && !(engineReady && modelReady)} label={engineLabel} />
      <StatusLine active={modelReady} pending={!unreachable && !modelReady} label={modelLabel} />
      <StatusLine active={openRouterConfigured} label={openRouterConfigured ? t('openRouterConfigured') : t('openRouterNotConfigured')} />
    </div>
  );
};

export const Sidebar: React.FC<SidebarProps> = ({
  currentView,
  onNavigate,
  theme,
  onToggleTheme,
  user,
  onOpenSettings,
  isOpen = true,
  onToggle,
}) => {
  const { t } = useI18n();

  return (
    <>
      {isOpen && onToggle && (
        <div className="fixed inset-0 z-40 bg-black/60 backdrop-blur-sm md:hidden" onClick={onToggle} />
      )}

      <aside className={`fixed inset-y-0 left-0 z-50 flex h-full min-h-0 shrink-0 flex-col overflow-hidden border-r border-zinc-200 bg-white py-4 transition-[width,transform] duration-300 dark:border-white/5 dark:bg-suno-sidebar md:relative md:inset-auto ${isOpen ? 'w-[min(20rem,calc(100vw-2.5rem))] md:w-[200px]' : 'w-[72px]'}`}>
        <div className="mb-6 flex min-w-0 items-center justify-between gap-2 px-3">
          <div className="flex min-w-0 items-center gap-3">
            {/* The studio mark: three ascending bars in the accent ramp, the
                same shape as the application icon. */}
            <button type="button" className="flex h-10 w-10 shrink-0 items-center justify-center rounded-[11px] shadow-lg transition-transform hover:scale-105" onClick={() => onNavigate('create')} title={STUDIO.name}>
              {/* The application's own icon, so the window and the taskbar
                  show the same mark. */}
              <img src="/brand/studio.png" alt="" className="h-10 w-10 rounded-[11px]" />
            </button>
            {isOpen && (
              <span className="min-w-0 text-[13px] font-bold leading-[1.15] text-zinc-900 dark:text-white" title={STUDIO.name}>
                {STUDIO.name}
              </span>
            )}
          </div>
          {onToggle && (
            <button type="button" onClick={onToggle} className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg text-zinc-500 transition-colors hover:bg-zinc-100 hover:text-black dark:text-zinc-400 dark:hover:bg-white/10 dark:hover:text-white" title={isOpen ? t('collapseSidebar') : t('expandSidebar')}>
              <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                {isOpen ? <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 19l-7-7 7-7" /> : <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 5l7 7-7 7" />}
              </svg>
            </button>
          )}
        </div>

        <nav className="flex min-h-0 w-full flex-1 flex-col gap-2 overflow-y-auto px-3 scrollbar-hide">
          <NavItem icon={<Disc size={20} />} label={t('create')} active={currentView === 'create'} onClick={() => onNavigate('create')} isExpanded={isOpen} />
          <NavItem icon={<Library size={20} />} label={t('library')} active={currentView === 'library'} onClick={() => onNavigate('library')} isExpanded={isOpen} />
          <NavItem icon={<Search size={20} />} label={t('search')} active={currentView === 'search'} onClick={() => onNavigate('search')} isExpanded={isOpen} />
          <NavItem icon={<Newspaper size={20} />} label={t('news')} active={currentView === 'news'} onClick={() => onNavigate('news')} isExpanded={isOpen} />
          <NavItem icon={<Layers size={20} />} label={t('adaptersNav')} active={currentView === 'adapters'} onClick={() => onNavigate('adapters')} isExpanded={isOpen} />
          <NavItem icon={<SlidersHorizontal size={20} />} label={t('studioTools')} active={currentView === 'tools'} onClick={() => onNavigate('tools')} isExpanded={isOpen} />

          <div className="mt-auto flex flex-col gap-2">
            {/* the files panel docks its chip here */}
            <div id="dock-slot" className="flex flex-col" />
            <ResourceMonitor isOpen={isOpen} />
            <SystemWidget isOpen={isOpen} />
            <button type="button" onClick={onToggleTheme} className={`flex w-full items-center gap-3 rounded-xl text-zinc-500 transition-all duration-200 hover:bg-zinc-100 hover:text-black dark:text-zinc-400 dark:hover:bg-white/5 dark:hover:text-white ${isOpen ? 'justify-start px-3 py-2.5' : 'aspect-square justify-center'}`} title={theme === 'dark' ? t('lightMode') : t('darkMode')}>
              <span className="shrink-0">{theme === 'dark' ? <Sun size={20} /> : <Moon size={20} />}</span>
              {isOpen && <span className="truncate text-sm font-medium">{theme === 'dark' ? t('lightMode') : t('darkMode')}</span>}
            </button>

              <button type="button" onClick={onOpenSettings} className={`flex w-full items-center gap-3 rounded-xl text-zinc-500 transition-all duration-200 hover:bg-zinc-100 hover:text-black dark:text-zinc-400 dark:hover:bg-white/5 dark:hover:text-white ${isOpen ? 'justify-start px-3 py-2.5' : 'aspect-square justify-center'}`} title={`${user.username} - ${t('settings')}`}>
                <span className="flex h-6 w-6 shrink-0 items-center justify-center overflow-hidden rounded-full border border-white/20 bg-gradient-to-br from-pink-500 to-purple-600 text-xs font-bold text-white">{user.username.charAt(0).toUpperCase()}</span>
                {isOpen && <span className="min-w-0 flex-1 truncate text-left text-sm font-medium">{user.username}</span>}
              </button>
          </div>
        </nav>
      </aside>
    </>
  );
};

interface NavItemProps {
  icon: React.ReactNode;
  label: string;
  active?: boolean;
  onClick: () => void;
  isExpanded?: boolean;
}

const NavItem: React.FC<NavItemProps> = ({ icon, label, active, onClick, isExpanded }) => (
  <button type="button" onClick={onClick} className={`group relative flex w-full items-center gap-3 overflow-hidden rounded-xl transition-all duration-200 ${isExpanded ? 'justify-start px-3 py-2.5' : 'aspect-square justify-center'} ${active ? 'bg-zinc-100 text-black dark:bg-white/10 dark:text-white' : 'text-zinc-500 hover:bg-zinc-100 hover:text-black dark:hover:bg-white/5 dark:hover:text-white'}`} title={label}>
    {active && <span className="absolute left-0 top-1/2 h-8 w-1 -translate-y-1/2 rounded-r-full bg-pink-500" />}
    <span className="shrink-0">{icon}</span>
    {isExpanded && <span className="truncate text-sm font-medium">{label}</span>}
  </button>
);

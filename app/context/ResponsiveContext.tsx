import React, { createContext, useContext, useState, useEffect, ReactNode, useMemo } from 'react';

interface ResponsiveContextType {
  isMobile: boolean;
  isDesktop: boolean;
  /** Wide enough for the song details beside the list (Tailwind's xl). */
  hasSidePanel: boolean;
}

const MOBILE_BREAKPOINT = 768;
const SIDE_PANEL_QUERY = '(min-width: 80rem)';

const ResponsiveContext = createContext<ResponsiveContextType | undefined>(undefined);

function getInitialState(): boolean {
  if (typeof window === 'undefined') {
    return false;
  }
  return window.innerWidth < MOBILE_BREAKPOINT;
}

export function ResponsiveProvider({ children }: { children: ReactNode }): React.ReactElement {
  const [isMobile, setIsMobile] = useState<boolean>(getInitialState);
  const [hasSidePanel, setHasSidePanel] = useState<boolean>(() => typeof window !== 'undefined' && window.matchMedia(SIDE_PANEL_QUERY).matches);

  useEffect(() => {
    if (typeof window === 'undefined') {
      return;
    }

    const mediaQuery = window.matchMedia(`(max-width: ${MOBILE_BREAKPOINT - 1}px)`);

    const handleChange = (event: MediaQueryListEvent) => {
      setIsMobile(event.matches);
    };

    setIsMobile(mediaQuery.matches);

    mediaQuery.addEventListener('change', handleChange);
    const sidePanelQuery = window.matchMedia(SIDE_PANEL_QUERY);
    const handleSidePanel = (event: MediaQueryListEvent) => setHasSidePanel(event.matches);
    setHasSidePanel(sidePanelQuery.matches);
    sidePanelQuery.addEventListener('change', handleSidePanel);
    return () => {
      mediaQuery.removeEventListener('change', handleChange);
      sidePanelQuery.removeEventListener('change', handleSidePanel);
    };
  }, []);

  const value = useMemo<ResponsiveContextType>(() => ({
    isMobile,
    isDesktop: !isMobile,
    hasSidePanel,
  }), [isMobile, hasSidePanel]);

  return <ResponsiveContext.Provider value={value}>{children}</ResponsiveContext.Provider>;
}

export function useResponsive(): ResponsiveContextType {
  const context = useContext(ResponsiveContext);
  if (context === undefined) {
    throw new Error('useResponsive must be used within a ResponsiveProvider');
  }
  return context;
}

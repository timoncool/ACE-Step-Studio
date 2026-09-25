import React, { createContext, useCallback, useContext, useMemo, useState, ReactNode } from 'react';
import { LOCAL_USER_ID } from '../services/nativeLibrary';

/**
 * The studio is a single-user desktop application: the library, the
 * media files and the model weights all live on this machine. There is no
 * account service, so this context only carries a display name for the shell.
 * It performs no network calls — the previous ACE implementation asked a Node
 * service on :3001 for a session and left the whole UI stuck on "connecting"
 * whenever that retired service was absent.
 */
export interface StudioProfile {
  id: string;
  username: string;
}

interface AuthContextType {
  user: StudioProfile;
  isLoading: false;
  setDisplayName: (username: string) => void;
}

const DISPLAY_NAME_KEY = 'studio.displayName';
const DEFAULT_DISPLAY_NAME = 'Local Studio';

const AuthContext = createContext<AuthContextType | undefined>(undefined);

function storedDisplayName(): string {
  try {
    return localStorage.getItem(DISPLAY_NAME_KEY)?.trim() || DEFAULT_DISPLAY_NAME;
  } catch {
    return DEFAULT_DISPLAY_NAME;
  }
}

export function AuthProvider({ children }: { children: ReactNode }): React.ReactElement {
  const [username, setUsername] = useState(storedDisplayName);

  const setDisplayName = useCallback((next: string) => {
    const clean = next.trim() || DEFAULT_DISPLAY_NAME;
    setUsername(clean);
    try {
      localStorage.setItem(DISPLAY_NAME_KEY, clean);
    } catch {
      // A blocked storage quota must not break the studio shell.
    }
  }, []);

  const value = useMemo<AuthContextType>(
    () => ({ user: { id: LOCAL_USER_ID, username }, isLoading: false, setDisplayName }),
    [setDisplayName, username],
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthContextType {
  const context = useContext(AuthContext);
  if (context === undefined) {
    throw new Error('useAuth must be used within an AuthProvider');
  }
  return context;
}

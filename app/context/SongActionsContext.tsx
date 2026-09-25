import React, { createContext, useContext } from 'react';
import { Song } from '../types';

/**
 * What can be done to a song, handed down once from the app. Every song menu
 * and song button reads it from here, so a list, the library page, the song
 * details and the player all offer the same actions.
 */
export interface SongActions {
  reusePrompt?: (song: Song) => void;
  replay?: (song: Song) => void;
  exportVideo?: (song: Song) => void;
  addToPlaylist?: (song: Song) => void;
  remove?: (song: Song) => void;
  update?: (song: Song) => void;
}

const SongActionsContext = createContext<SongActions>({});

export const SongActionsProvider: React.FC<{ value: SongActions; children: React.ReactNode }> = ({ value, children }) => (
  <SongActionsContext.Provider value={value}>{children}</SongActionsContext.Provider>
);

export const useSongActions = () => useContext(SongActionsContext);

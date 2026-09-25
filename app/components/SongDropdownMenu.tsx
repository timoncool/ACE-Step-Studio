import React, { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { hasSungLines, karaokeReason } from '../services/karaoke';
import { Song } from '../types';
import { useI18n } from '../context/I18nContext';
import { openExternal } from '../services/externalLinks';
import { apiUrl } from '../services/apiBase';
import { downloadSongAudio } from '../services/songDownload';
import { openMidi, openStems } from '../services/openStems';
import { useAuth } from '../context/AuthContext';
import { useSongActions } from '../context/SongActionsContext';
import {
    Clapperboard,
    Edit3,
    Layers,
    Repeat,
    ListPlus,
    Download,
    Trash2,
    Loader2,
    Mic2,
    Scissors,
    Wand2,
    Piano,
} from 'lucide-react';

/** The one menu of a song. Its actions come from the app through
 *  SongActionsContext, so every place that opens it offers the same items. */
interface SongDropdownMenuProps {
    song: Song;
    isOpen: boolean;
    onClose: () => void;
    position?: 'left' | 'right' | 'center';
    direction?: 'up' | 'down';
}

/** The space between a menu and its button: mt-2 / mb-2. */
const MENU_GAP = 8;

interface MenuItemProps {
    icon: React.ReactNode;
    label: string;
    onClick?: () => void;
    danger?: boolean;
    disabled?: boolean;
}

const MenuItem: React.FC<MenuItemProps> = ({ icon, label, onClick, danger, disabled }) => (
    <button
        onClick={onClick}
        disabled={disabled}
        className={`w-full px-3 py-2 text-left text-sm flex items-center gap-3 transition-colors
            ${danger
                ? 'text-red-400 hover:bg-red-500/10 hover:text-red-300'
                : 'text-zinc-300 hover:bg-white/5 hover:text-white'}
            ${disabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'}
        `}
    >
        <span className="w-4 h-4 flex items-center justify-center opacity-70">{icon}</span>
        <span>{label}</span>
    </button>
);

const MenuDivider: React.FC = () => (
    <div className="h-px bg-white/10 my-1 mx-2" />
);

/// Karaoke timings for one track, made on demand. The menu asks the service
/// whether karaoke is configured at all; with it off nothing is shown.
function useKaraoke(song: Song, onSongUpdate?: (song: Song) => void) {
    const { t: translate } = useI18n();
    const [ready, setReady] = useState(false);
    const [busy, setBusy] = useState(false);

    useEffect(() => {
        void fetch('/v1/karaoke/status')
            .then((response) => (response.ok ? response.json() : Promise.reject(new Error())))
            .then((status: { ready?: boolean }) => setReady(status.ready === true))
            .catch(() => setReady(false));
    }, []);

    // A refusal used to be thrown out of an unawaited promise and land
    // nowhere: the menu item stopped spinning and nothing else happened, which
    // reads exactly like a button that does not work.
    const [failed, setFailed] = useState<string | null>(null);
    const make = async () => {
        setBusy(true);
        setFailed(null);
        try {
            const response = await fetch(`/v1/library/songs/${encodeURIComponent(song.id)}/karaoke`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({}),
            });
            const body = await response.json().catch(() => null);
            if (!response.ok) {
                setFailed(karaokeReason(translate, body?.error) || `${response.status}`);
                return;
            }
            onSongUpdate?.({ ...song, lrcContent: body.lrc as string });
        } catch (error) {
            setFailed(error instanceof Error ? error.message : String(error));
        } finally {
            setBusy(false);
        }
    };

    return { ready, busy, make, failed };
}

export const SongDropdownMenu: React.FC<SongDropdownMenuProps> = ({
    song,
    isOpen,
    onClose,
    position = 'right',
    direction = 'down',
}) => {
    const { t } = useI18n();
    const { user } = useAuth();
    const actions = useSongActions();
    const isOwner = Boolean(user && user.id === song.userId);
    const menuRef = useRef<HTMLDivElement>(null);
    // Near the bottom of its panel the menu opens upward; when it fits neither
    // way, the panel scrolls it into view instead of hiding it under the edge.
    const [up, setUp] = useState(direction === 'up');
    useLayoutEffect(() => {
        const node = menuRef.current;
        if (!isOpen) {
            setUp(direction === 'up');
            return;
        }
        if (direction === 'up' || !node || !node.parentElement) return;
        const anchor = node.parentElement;
        const place = () => {
            let panel = { top: 0, bottom: window.innerHeight };
            for (let element: HTMLElement | null = anchor; element; element = element.parentElement) {
                if (/(auto|scroll|hidden)/.test(getComputedStyle(element).overflowY)) {
                    const box = element.getBoundingClientRect();
                    panel = { top: Math.max(0, box.top), bottom: Math.min(window.innerHeight, box.bottom) };
                    break;
                }
            }
            const around = anchor.getBoundingClientRect();
            const height = node.getBoundingClientRect().height;
            if (around.bottom + MENU_GAP + height <= panel.bottom) setUp(false);
            else if (around.top - MENU_GAP - height >= panel.top) setUp(true);
            else node.scrollIntoView({ block: 'nearest' });
        };
        place();
        // items that appear later (karaoke) make the menu taller
        const watch = new ResizeObserver(place);
        watch.observe(node);
        return () => watch.disconnect();
    }, [isOpen, direction]);
    const { ready: karaokeReady, busy: karaokeBusy, make: makeKaraoke, failed: karaokeFailed } = useKaraoke(song, actions.update);

    useEffect(() => {
        const handleClickOutside = (e: MouseEvent) => {
            if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
                onClose();
            }
        };

        const handleEscape = (e: KeyboardEvent) => {
            if (e.key === 'Escape') {
                onClose();
            }
        };

        if (isOpen) {
            document.addEventListener('mousedown', handleClickOutside);
            document.addEventListener('keydown', handleEscape);
        }

        return () => {
            document.removeEventListener('mousedown', handleClickOutside);
            document.removeEventListener('keydown', handleEscape);
        };
    }, [isOpen, onClose]);

    if (!isOpen) return null;

    const handleAction = (action?: () => void) => {
        if (action) {
            action();
        }
        onClose();
    };

    const handleEditAudio = () => {
        if (!song.audioUrl) return;
        const audioUrl = song.audioUrl.startsWith('http')
            ? song.audioUrl
            : `${window.location.origin}${song.audioUrl}`;
        // AudioMass is a fully client-side editor shipped as static assets, so
        // it runs without any backend service.
        void openExternal(apiUrl(`/editor/index.html?audioUrl=${encodeURIComponent(audioUrl)}`));
        onClose();
    };


    const handleDownload = async () => {
        try {
            await downloadSongAudio(song);
        } catch (error) {
            console.error('Download failed:', error);
        }
        onClose();
    };

    const positionClasses = position === 'left' ? 'left-0' : position === 'center' ? 'left-1/2 -translate-x-1/2' : 'right-0';
    const directionClasses = up
        ? 'bottom-full mb-2'
        : 'top-full mt-2';
    const animationClasses = up
        ? 'animate-in fade-in slide-in-from-bottom-2'
        : 'animate-in fade-in slide-in-from-top-2';

    return (
        <div
            ref={menuRef}
            className={`absolute ${positionClasses} ${directionClasses} w-52
                bg-zinc-900 rounded-xl shadow-2xl border border-white/10 py-1.5 z-50
                ${animationClasses} duration-150`}
            onClick={(e) => e.stopPropagation()}
        >
            {/* Creative Actions */}
            {isOwner && (
                <MenuItem
                    icon={<Edit3 size={14} />}
                    label={t('editAudio')}
                    onClick={handleEditAudio}
                />
            )}
            {actions.exportVideo && (
                <MenuItem
                    icon={<Clapperboard size={14} />}
                    label={t('videoExport')}
                    onClick={() => handleAction(() => actions.exportVideo?.(song))}
                    disabled={!song.audioUrl}
                />
            )}
            {song.audioUrl && (
                <>
                    <MenuItem
                        icon={<Wand2 size={14} />}
                        label={t('processMenu')}
                        onClick={() => handleAction(() => window.dispatchEvent(new CustomEvent('studio:process-song', { detail: song })))}
                    />
                    <MenuItem
                        icon={<Piano size={14} />}
                        label={t('midiMenu')}
                        onClick={() => handleAction(() => openMidi(song))}
                    />
                </>
            )}
            <MenuItem
                icon={<Scissors size={14} />}
                label={t('stemsTitle')}
                onClick={() => handleAction(() => openStems(song))}
                disabled={!song.audioUrl}
            />
            {actions.reusePrompt && (
                <MenuItem
                    icon={<Repeat size={14} />}
                    label={t('reusePrompt')}
                    onClick={() => handleAction(() => actions.reusePrompt?.(song))}
                />
            )}
            {actions.replay && song.nativeReplayAvailable && (
                <MenuItem
                    icon={<Repeat size={14} />}
                    label={t('replayTitle')}
                    onClick={() => handleAction(() => actions.replay?.(song))}
                />
            )}

            {/* Karaoke: only offered once it is switched on in Settings, and
                only for a track that has both audio and written lyrics. */}
            {karaokeReady && song.audioUrl && hasSungLines(song.lyrics) && (
                <MenuItem
                    icon={karaokeBusy ? <Loader2 size={14} className="animate-spin" /> : <Mic2 size={14} />}
                    label={karaokeBusy ? t('karaokeMaking') : song.lrcContent ? t('karaokeReady') : t('karaokeMake')}
                    onClick={() => void makeKaraoke()}
                    disabled={karaokeBusy}
                />
            )}
            {karaokeFailed && (
                <p className="px-3 py-1.5 text-[11px] leading-4 text-rose-600 dark:text-rose-300">{karaokeFailed}</p>
            )}

            <MenuDivider />

            {/* Library Actions */}
            <MenuItem
                icon={<ListPlus size={14} />}
                label={t('addToPlaylist')}
                onClick={() => handleAction(() => actions.addToPlaylist?.(song))}
            />
            <MenuItem
                icon={<Download size={14} />}
                label={t('download')}
                onClick={handleDownload}
            />

            {/* Owner-only Actions */}
            {isOwner && (
                <>
                    <MenuDivider />
                    <MenuItem
                        icon={<Trash2 size={14} />}
                        label={t('deleteSong')}
                        onClick={() => handleAction(() => actions.remove?.(song))}
                        danger
                    />
                </>
            )}
        </div>
    );
};

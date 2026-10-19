import React, { useState } from 'react';
import { Loader2, Mic2 } from 'lucide-react';
import { useKaraokeStatus } from '../services/studioQueries';
import { useI18n } from '../context/I18nContext';
import { Song } from '../types';

export const KaraokeAction: React.FC<{ song: Song; onDone?: (lrc: string) => void; unavailable?: React.ReactNode }> = ({ song, onDone, unavailable = null }) => {
    const { t } = useI18n();
    const available = useKaraokeStatus().data?.ready === true;
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState<string | null>(null);

    if (!available) return <>{unavailable}</>;
    if (!song.audioUrl || !song.lyrics?.trim()) return null;

    const run = async () => {
        setBusy(true);
        setError(null);
        try {
            const response = await fetch(`/v1/library/songs/${encodeURIComponent(song.id)}/karaoke`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({}),
            });
            const body = await response.json().catch(() => null);
            if (!response.ok) throw new Error(body?.error || String(response.status));
            onDone?.(body.lrc as string);
        } catch (reason) {
            setError(reason instanceof Error ? reason.message : String(reason));
        } finally {
            setBusy(false);
        }
    };

    return (
        <div className="space-y-1">
            <button
                onClick={() => void run()}
                disabled={busy}
                className="w-full flex items-center justify-center gap-2 px-3 py-2 rounded-xl bg-zinc-100 dark:bg-white/5 hover:bg-zinc-200 dark:hover:bg-white/10 text-zinc-600 dark:text-zinc-400 text-xs font-medium transition-colors disabled:opacity-50"
            >
                {busy ? <Loader2 size={14} className="animate-spin" /> : <Mic2 size={14} />}
                {busy ? t('karaokeMaking') : song.lrcContent ? t('karaokeReady') : t('karaokeMake')}
            </button>
            {error && <p className="text-[11px] text-red-500">{error}</p>}
        </div>
    );
};

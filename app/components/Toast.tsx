import { useI18n } from '../context/I18nContext';
import React, { useEffect, useRef } from 'react';
import { CheckCircle, AlertCircle, X } from 'lucide-react';

export type ToastType = 'success' | 'error' | 'info';

interface ToastProps {
    message: string;
    type?: ToastType;
    isVisible: boolean;
    onClose: () => void;
    duration?: number;
}

export const Toast: React.FC<ToastProps> = ({
    message,
    type = 'success',
    isVisible,
    onClose,
    duration = 3000
}) => {
    const { t } = useI18n();
    const onCloseRef = useRef(onClose);
    useEffect(() => { onCloseRef.current = onClose; }, [onClose]);

    useEffect(() => {
        if (isVisible && duration > 0) {
            const timer = setTimeout(() => {
                onCloseRef.current();
            }, duration);
            return () => clearTimeout(timer);
        }
    }, [isVisible, duration, message]);

    if (!isVisible) return null;

    const bgColors = {
        success: 'bg-zinc-900 border-green-500/50 text-white',
        error: 'bg-zinc-900 border-red-500/50 text-white',
        info: 'bg-zinc-900 border-blue-500/50 text-white',
    };

    const icons = {
        success: <CheckCircle className="text-green-500" size={20} />,
        error: <AlertCircle className="text-red-500" size={20} />,
        info: <AlertCircle className="text-blue-500" size={20} />,
    };

    return (
        <div className="pointer-events-none fixed inset-x-3 top-3 z-[100] flex justify-center sm:top-6">
            <div role={type === 'error' ? 'alert' : 'status'} className={`pointer-events-auto flex max-w-full items-center gap-3 rounded-2xl border px-4 py-3 shadow-2xl ${bgColors[type]} animate-in slide-in-from-top-4 fade-in duration-300 sm:rounded-full sm:px-6 sm:py-4`}>
                {icons[type]}
                <span className="min-w-0 break-words text-sm font-medium">{message}</span>
                <button type="button" onClick={onClose} className="ml-1 shrink-0 rounded-full p-0.5 hover:opacity-70" aria-label={t('closeNotification')}>
                    <X size={16} />
                </button>
            </div>
        </div>
    );
};

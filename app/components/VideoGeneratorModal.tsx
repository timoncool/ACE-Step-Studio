import React, { useRef, useEffect, useState, useCallback } from 'react';
import { useI18n } from '../context/I18nContext';
import { saveFile } from '../services/saveFile';
import { Song } from '../types';
import { X, Play, Pause, Download, Wand2, Image as ImageIcon, Music, Video, Loader2, Palette, Layers, Zap, Type, Monitor, Aperture, Activity, Circle, Grid, Box, BarChart2, Waves, Disc, Upload, Plus, Trash2, Settings2, MousePointer2, Search, ExternalLink, Sun, Film, Minus } from 'lucide-react';
import { FFmpeg } from '@ffmpeg/ffmpeg';
import { fetchFile, toBlobURL } from '@ffmpeg/util';
import { createHardwareEncoder, type HardwareEncoder } from '../services/videoEncoder';
import { lineProgress } from '../services/lrc-parser';
import { useResponsive } from '../context/ResponsiveContext';
import { useBridgeCommand } from '../services/mcpBridge';
import { apiUrl } from '../services/apiBase';
import { STUDIO } from '../studio';

interface VideoGeneratorModalProps {
  isOpen: boolean;
  onClose: () => void;
  song: Song | null;
}

type PresetType = 
  | 'NCS Circle' | 'Linear Bars' | 'Dual Mirror' | 'Center Wave' 
  | 'Orbital' | 'Digital Rain' | 'Hexagon' | 'Shockwave' 
  | 'Oscilloscope' | 'Minimal';

type AspectRatio = '16:9' | '9:16' | '1:1';

const RESOLUTIONS: Record<AspectRatio, { width: number; height: number; label: string }> = {
  '16:9': { width: 1920, height: 1080, label: '1920×1080' },
  '9:16': { width: 1080, height: 1920, label: '1080×1920' },
  '1:1': { width: 1080, height: 1080, label: '1080×1080' },
};

interface VisualizerConfig {
  preset: PresetType;
  primaryColor: string;
  secondaryColor: string;
  bgDim: number;
  particleCount: number;
  aspectRatio: AspectRatio;
  visualizerX: number; // 0-100%
  visualizerY: number; // 0-100%
  visualizerScale: number; // 0.3-2.0
  lyricsX: number;     // 0-100%
  lyricsY: number;     // 0-100%
}

interface EffectConfig {
  shake: boolean;
  glitch: boolean;
  vhs: boolean;
  cctv: boolean;
  scanlines: boolean;
  chromatic: boolean;
  bloom: boolean;
  filmGrain: boolean;
  pixelate: boolean;
  strobe: boolean;
  vignette: boolean;
  hueShift: boolean;
  letterbox: boolean;
}

interface EffectIntensities {
  shake: number;
  glitch: number;
  vhs: number;
  cctv: number;
  scanlines: number;
  chromatic: number;
  bloom: number;
  filmGrain: number;
  pixelate: number;
  strobe: number;
  vignette: number;
  hueShift: number;
  letterbox: number;
}

interface TextLayer {
  id: string;
  text: string;
  x: number; // 0-100 percentage
  y: number; // 0-100 percentage
  size: number;
  color: string;
  font: string;
}

interface PexelsPhoto {
  id: number;
  src: { large: string; original: string };
  photographer: string;
}

interface PexelsVideo {
  id: number;
  image: string;
  video_files: { link: string; quality: string; width: number }[];
  user: { name: string };
}

const PRESETS: { id: PresetType; labelKey: string; icon: React.ReactNode }[] = [
  { id: 'NCS Circle', labelKey: 'presetClassicNcs', icon: <Circle size={16} /> },
  { id: 'Linear Bars', labelKey: 'presetSpectrum', icon: <BarChart2 size={16} /> },
  { id: 'Dual Mirror', labelKey: 'presetMirror', icon: <ColumnsIcon /> },
  { id: 'Center Wave', labelKey: 'presetShockwave', icon: <Waves size={16} /> },
  { id: 'Orbital', labelKey: 'presetOrbital', icon: <Disc size={16} /> },
  { id: 'Hexagon', labelKey: 'presetHexCore', icon: <Box size={16} /> },
  { id: 'Oscilloscope', labelKey: 'presetAnalog', icon: <Activity size={16} /> },
  { id: 'Digital Rain', labelKey: 'presetMatrix', icon: <Grid size={16} /> },
  { id: 'Shockwave', labelKey: 'presetPulse', icon: <Aperture size={16} /> },
  { id: 'Minimal', labelKey: 'presetClean', icon: <Type size={16} /> },
];

/** Draw image with cover-fit (no stretching, crops to fill) centered at (cx, cy) */
function drawImageCover(ctx: CanvasRenderingContext2D, img: CanvasImageSource, cx: number, cy: number, canvasW: number, canvasH: number) {
  const imgW = (img as HTMLImageElement).naturalWidth || (img as HTMLVideoElement).videoWidth || canvasW;
  const imgH = (img as HTMLImageElement).naturalHeight || (img as HTMLVideoElement).videoHeight || canvasH;
  const scale = Math.max(canvasW / imgW, canvasH / imgH);
  const drawW = imgW * scale;
  const drawH = imgH * scale;
  ctx.drawImage(img, cx - drawW / 2, cy - drawH / 2, drawW, drawH);
}

function ColumnsIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 3v18"/>
      <rect width="18" height="18" x="3" y="3" rx="2" />
    </svg>
  );
}

export const VideoGeneratorModal: React.FC<VideoGeneratorModalProps> = ({ isOpen, onClose, song }) => {
  const { t } = useI18n();
  const { isMobile } = useResponsive();

  // Lyrics overlay
  const lrcLinesRef = useRef<import('../services/lrc-parser').LrcLine[]>([]);
  const [lyricsEnabled, setLyricsEnabled] = useState(true);
  const [lyricsStyle, setLyricsStyle] = useState<'lines' | 'scroll' | 'karaoke'>('karaoke');
  const [lyricsPosition, setLyricsPosition] = useState<'bottom' | 'center' | 'top'>('bottom');
  const [lyricsFontSize, setLyricsFontSize] = useState(50);
  const [lyricsLines, setLyricsLines] = useState(2);
  const [lyricsShowSections, setLyricsShowSections] = useState(false);
  const [lyricsColor, setLyricsColor] = useState('#ffffff');
  const [lyricsBgColor, setLyricsBgColor] = useState('#000000');
  const [lyricsBgOpacity, setLyricsBgOpacity] = useState(50);
  const [lyricsHighlightColor, setLyricsHighlightColor] = useState('#ec4899');
  const [lyricsOffset, setLyricsOffset] = useState(0); // seconds, negative = later
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const animationRef = useRef<number>(0);
  const analyserRef = useRef<AnalyserNode | null>(null);
  const audioContextRef = useRef<AudioContext | null>(null);
  const bgImageRef = useRef<HTMLImageElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const videoFileInputRef = useRef<HTMLInputElement>(null);

  // FFmpeg Refs
  const ffmpegRef = useRef<FFmpeg | null>(null);

  // Tabs: 'presets' | 'style' | 'text' | 'effects'
  const [activeTab, setActiveTab] = useState('presets');

  // State
  const [isPlaying, setIsPlaying] = useState(false);
  const [playbackTime, setPlaybackTime] = useState(0);
  const [playbackDuration, setPlaybackDuration] = useState(0);
  const [volume, setVolume] = useState(1);
  const [backgroundType, setBackgroundType] = useState<'random' | 'custom' | 'video'>('random');
  const [backgroundSeed, setBackgroundSeed] = useState(Date.now());
  const [customImage, setCustomImage] = useState<string | null>(null);
  const [videoUrl, setVideoUrl] = useState<string>('');
  const bgVideoRef = useRef<HTMLVideoElement | null>(null);
  const [bgVideoState, setBgVideoState] = useState<'loading' | 'ready' | 'failed'>('loading');

  // Custom Album Art
  const [customAlbumArt, setCustomAlbumArt] = useState<string | null>(null);
  const albumArtInputRef = useRef<HTMLInputElement>(null);
  const albumArtImageRef = useRef<HTMLImageElement | null>(null);

  // Pexels Browser State
  const [showPexelsBrowser, setShowPexelsBrowser] = useState(false);
  const [pexelsTarget, setPexelsTarget] = useState<'background' | 'albumArt'>('background');
  const [pexelsTab, setPexelsTab] = useState<'photos' | 'videos'>('photos');
  const [pexelsQuery, setPexelsQuery] = useState('abstract');
  const [pexelsPhotos, setPexelsPhotos] = useState<PexelsPhoto[]>([]);
  const [pexelsVideos, setPexelsVideos] = useState<PexelsVideo[]>([]);
  const [pexelsLoading, setPexelsLoading] = useState(false);
  const [pexelsApiKey, setPexelsApiKey] = useState<string>(() => localStorage.getItem('pexels_api_key') || '');
  const [showPexelsApiKeyInput, setShowPexelsApiKeyInput] = useState(false);
  const [pexelsError, setPexelsError] = useState<string | null>(null);
  
  const [isExporting, setIsExporting] = useState(false);
  const [exportProgress, setExportProgress] = useState(0);
  const [exportStage, setExportStage] = useState<'idle' | 'capturing' | 'encoding'>('idle');
  const [ffmpegLoaded, setFfmpegLoaded] = useState(false);
  const [ffmpegLoading, setFfmpegLoading] = useState(false);
  // An export started by an agent goes to the studio's folder instead of a download
  const exportTargetRef = useRef<((blob: Blob) => Promise<void>) | null>(null);
  const [savedVideo, setSavedVideo] = useState<string | null>(null);
  const [agentExportError, setAgentExportError] = useState<string | null>(null);

  // Config State
  const [config, setConfig] = useState<VisualizerConfig>({
    preset: 'NCS Circle',
    primaryColor: '#ec4899', // Pink-500
    secondaryColor: '#3b82f6', // Blue-500
    bgDim: 0.6,
    particleCount: 50,
    aspectRatio: '1:1' as AspectRatio,
    visualizerX: 50,
    visualizerY: 50,
    visualizerScale: 1.0,
    lyricsX: 50,
    lyricsY: 10,
  });

  const [effects, setEffects] = useState<EffectConfig>({
    shake: true,
    glitch: false,
    vhs: false,
    cctv: false,
    scanlines: false,
    chromatic: false,
    bloom: false,
    filmGrain: false,
    pixelate: false,
    strobe: false,
    vignette: false,
    hueShift: false,
    letterbox: false
  });

  const [intensities, setIntensities] = useState<EffectIntensities>({
    shake: 0.05,
    glitch: 0.3,
    vhs: 0.5,
    cctv: 0.8,
    scanlines: 0.4,
    chromatic: 0.5,
    bloom: 0.5,
    filmGrain: 0.3,
    pixelate: 0.3,
    strobe: 0.5,
    vignette: 0.5,
    hueShift: 0.5,
    letterbox: 0.5
  });

  // Text Layers State
  const [textLayers, setTextLayers] = useState<TextLayer[]>([]);

  // Sync playback time state with audio element
  useEffect(() => {
    if (!isPlaying) return;
    const interval = setInterval(() => {
      if (audioRef.current) {
        setPlaybackTime(audioRef.current.currentTime);
        setPlaybackDuration(audioRef.current.duration || 0);
      }
    }, 250);
    return () => clearInterval(interval);
  }, [isPlaying]);

  // Init default text + parse LRC on load
  useEffect(() => {
    if (song) {
        setTextLayers([
            { id: '1', text: song.title, x: 50, y: 85, size: 48, color: '#ffffff', font: 'Inter' },
            { id: '2', text: song.creator || STUDIO.name, x: 50, y: 92, size: 24, color: '#a1a1aa', font: 'Inter' }
        ]);
        // Parse LRC
        if (song.lrcContent) {
            import('../services/lrc-parser').then(({ parseLrc }) => {
                lrcLinesRef.current = parseLrc(song.lrcContent!);
                setLyricsEnabled(true);
            });
        } else {
            lrcLinesRef.current = [];
            setLyricsEnabled(false);
        }
    }
  }, [song]);

  // Use refs for render loop to access latest state without re-binding
  const configRef = useRef(config);
  const effectsRef = useRef(effects);
  const intensitiesRef = useRef(intensities);
  const textLayersRef = useRef(textLayers);

  const lyricsEnabledRef = useRef(lyricsEnabled);
  const lyricsStyleRef = useRef(lyricsStyle);
  const lyricsPositionRef = useRef(lyricsPosition);
  const lyricsFontSizeRef = useRef(lyricsFontSize);
  const lyricsLinesRef = useRef(lyricsLines);
  const lyricsShowSectionsRef = useRef(lyricsShowSections);
  const lyricsColorRef = useRef(lyricsColor);
  const lyricsBgColorRef = useRef(lyricsBgColor);
  const lyricsBgOpacityRef = useRef(lyricsBgOpacity);
  const lyricsHighlightColorRef = useRef(lyricsHighlightColor);
  const lyricsOffsetRef = useRef(lyricsOffset);

  // WYSIWYG drag state
  const dragRef = useRef<{ layerId: string; startX: number; startY: number; origX: number; origY: number } | null>(null);
  const [hoveredLayer, setHoveredLayer] = useState<string | null>(null);

  const getCanvasCoords = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const canvas = canvasRef.current!;
    const rect = canvas.getBoundingClientRect();
    // Account for object-contain — canvas may have letterbox/pillarbox
    const canvasAspect = canvas.width / canvas.height;
    const rectAspect = rect.width / rect.height;
    let renderW: number, renderH: number, offsetX: number, offsetY: number;
    if (rectAspect > canvasAspect) {
      // Pillarbox (bars on sides)
      renderH = rect.height;
      renderW = rect.height * canvasAspect;
      offsetX = (rect.width - renderW) / 2;
      offsetY = 0;
    } else {
      // Letterbox (bars top/bottom)
      renderW = rect.width;
      renderH = rect.width / canvasAspect;
      offsetX = 0;
      offsetY = (rect.height - renderH) / 2;
    }
    return {
      x: Math.max(0, Math.min(100, ((e.clientX - rect.left - offsetX) / renderW) * 100)),
      y: Math.max(0, Math.min(100, ((e.clientY - rect.top - offsetY) / renderH) * 100)),
    };
  };

  const hitTestLayers = (px: number, py: number): string | null => {
    const cfg = configRef.current;

    // Hit test visualizer (center circle area ~20% radius)
    const vDist = Math.sqrt((px - cfg.visualizerX) ** 2 + (py - cfg.visualizerY) ** 2);
    if (vDist < 15) return '__visualizer__';

    // Hit test lyrics area
    if (lyricsEnabledRef.current && lrcLinesRef.current.length > 0) {
      const lDist = Math.abs(py - cfg.lyricsY);
      const lDistX = Math.abs(px - cfg.lyricsX);
      if (lDist < 8 && lDistX < 30) return '__lyrics__';
    }

    // Hit test text layers (reverse order = top layer first)
    const canvas = canvasRef.current;
    if (!canvas) return null;
    const ctx = canvas.getContext('2d');
    if (!ctx) return null;
    for (let i = textLayers.length - 1; i >= 0; i--) {
      const layer = textLayers[i];
      ctx.font = `bold ${layer.size * (canvas.width / 1920)}px ${layer.font}, sans-serif`;
      const metrics = ctx.measureText(layer.text);
      const textW = (metrics.width / canvas.width) * 100;
      const textH = (layer.size * (canvas.width / 1920) * 1.3 / canvas.height) * 100;
      const pad = 3;
      if (px >= layer.x - textW / 2 - pad && px <= layer.x + textW / 2 + pad &&
          py >= layer.y - textH - pad && py <= layer.y + pad) {
        return layer.id;
      }
    }
    return null;
  };

  const handleCanvasMouseDown = useCallback((e: React.MouseEvent<HTMLCanvasElement>) => {
    if (!canvasRef.current) return;
    const { x, y } = getCanvasCoords(e);
    const hitId = hitTestLayers(x, y);
    if (hitId) {
      let origX: number, origY: number;
      if (hitId === '__visualizer__') {
        origX = config.visualizerX; origY = config.visualizerY;
      } else if (hitId === '__lyrics__') {
        origX = config.lyricsX; origY = config.lyricsY;
      } else {
        const layer = textLayers.find(l => l.id === hitId);
        if (!layer) return;
        origX = layer.x; origY = layer.y;
      }
      dragRef.current = { layerId: hitId, startX: x, startY: y, origX, origY };
      e.preventDefault();
    }
  }, [textLayers, config]);

  const handleCanvasMouseMove = useCallback((e: React.MouseEvent<HTMLCanvasElement>) => {
    if (!canvasRef.current) return;
    const { x, y } = getCanvasCoords(e);

    if (dragRef.current) {
      const dx = x - dragRef.current.startX;
      const dy = y - dragRef.current.startY;
      const newX = Math.max(5, Math.min(95, dragRef.current.origX + dx));
      const newY = Math.max(5, Math.min(95, dragRef.current.origY + dy));
      const id = dragRef.current.layerId;
      if (id === '__visualizer__') {
        setConfig(prev => ({ ...prev, visualizerX: newX, visualizerY: newY }));
      } else if (id === '__lyrics__') {
        setConfig(prev => ({ ...prev, lyricsX: newX, lyricsY: newY }));
      } else {
        setTextLayers(prev => prev.map(l => l.id === id ? { ...l, x: newX, y: newY } : l));
      }
    } else {
      // Hover detection for cursor
      const hitId = hitTestLayers(x, y);
      setHoveredLayer(hitId);
    }
  }, [textLayers]);

  const handleCanvasMouseUp = useCallback(() => {
    dragRef.current = null;
  }, []);

  const handleCanvasWheel = useCallback((e: React.WheelEvent<HTMLCanvasElement>) => {
    if (!canvasRef.current) return;
    const { x, y } = getCanvasCoords(e as any);
    const hitId = hitTestLayers(x, y);
    if (!hitId) return;
    e.preventDefault();
    const delta = e.deltaY > 0 ? -2 : 2; // scroll down = smaller, up = bigger

    if (hitId === '__visualizer__') {
      const scaleDelta = e.deltaY > 0 ? -0.05 : 0.05;
      setConfig(prev => ({ ...prev, visualizerScale: Math.max(0.3, Math.min(2.0, prev.visualizerScale + scaleDelta)) }));
    } else if (hitId === '__lyrics__') {
      setLyricsFontSize(prev => Math.max(16, Math.min(96, prev + delta)));
    } else {
      setTextLayers(prev => prev.map(l =>
        l.id === hitId ? { ...l, size: Math.max(12, Math.min(120, l.size + delta)) } : l
      ));
    }
  }, [textLayers]);

  useEffect(() => { configRef.current = config; }, [config]);
  useEffect(() => { effectsRef.current = effects; }, [effects]);
  useEffect(() => { intensitiesRef.current = intensities; }, [intensities]);
  useEffect(() => { textLayersRef.current = textLayers; }, [textLayers]);
  useEffect(() => { lyricsEnabledRef.current = lyricsEnabled; }, [lyricsEnabled]);
  useEffect(() => { lyricsStyleRef.current = lyricsStyle; }, [lyricsStyle]);
  useEffect(() => { lyricsPositionRef.current = lyricsPosition; }, [lyricsPosition]);
  useEffect(() => { lyricsFontSizeRef.current = lyricsFontSize; }, [lyricsFontSize]);
  useEffect(() => { lyricsLinesRef.current = lyricsLines; }, [lyricsLines]);
  useEffect(() => { lyricsShowSectionsRef.current = lyricsShowSections; }, [lyricsShowSections]);
  useEffect(() => { lyricsColorRef.current = lyricsColor; }, [lyricsColor]);
  useEffect(() => { lyricsBgColorRef.current = lyricsBgColor; }, [lyricsBgColor]);
  useEffect(() => { lyricsBgOpacityRef.current = lyricsBgOpacity; }, [lyricsBgOpacity]);
  useEffect(() => { lyricsHighlightColorRef.current = lyricsHighlightColor; }, [lyricsHighlightColor]);
  useEffect(() => { lyricsOffsetRef.current = lyricsOffset; }, [lyricsOffset]);

  // Load FFmpeg
  const loadFFmpeg = useCallback(async () => {
    if (ffmpegRef.current || ffmpegLoading) return;

    setFfmpegLoading(true);
    try {
      const ffmpeg = new FFmpeg();

      ffmpeg.on('progress', ({ progress }) => {
        if (exportStage === 'encoding') {
          setExportProgress(Math.round(progress * 100));
        }
      });

      // Load FFmpeg from local files (no CDN dependency)
      await ffmpeg.load({
        coreURL: '/ffmpeg/ffmpeg-core.js',
        wasmURL: '/ffmpeg/ffmpeg-core.wasm',
      });

      ffmpegRef.current = ffmpeg;
      setFfmpegLoaded(true);
    } catch (error) {
      console.error('Failed to load FFmpeg:', error);
      alert('Failed to load video encoder. Check your internet connection and try again.');
    } finally {
      setFfmpegLoading(false);
    }
  }, [ffmpegLoading, exportStage]);

  // Load Background Image
  useEffect(() => {
    if (backgroundType === 'video') {
      bgImageRef.current = null;
      return;
    }

    const img = new Image();
    img.crossOrigin = "Anonymous";
    if (backgroundType === 'custom' && customImage) {
      img.src = customImage;
    } else {
      const bgRes = RESOLUTIONS[config.aspectRatio || '16:9'];
      img.src = `https://picsum.photos/seed/${backgroundSeed}/${bgRes.width}/${bgRes.height}?blur=4`;
    }
    img.onload = () => {
      bgImageRef.current = img;
    };
  }, [backgroundSeed, backgroundType, customImage]);

  // Load Background Video
  useEffect(() => {
    if (backgroundType !== 'video' || !videoUrl) {
      if (bgVideoRef.current) {
        bgVideoRef.current.pause();
        bgVideoRef.current = null;
      }
      return;
    }

    const video = document.createElement('video');
    video.crossOrigin = 'anonymous';
    video.src = videoUrl;
    video.loop = true;
    video.muted = true;
    video.playsInline = true;
    video.autoplay = true;
    setBgVideoState('loading');

    video.onloadeddata = () => {
      bgVideoRef.current = video;
      setBgVideoState('ready');
      video.play().catch(console.error);
    };

    video.onerror = () => {
      console.error('[ERROR] the background video did not load:', videoUrl);
      bgVideoRef.current = null;
      setBgVideoState('failed');
    };

    return () => {
      video.pause();
      video.src = '';
    };
  }, [backgroundType, videoUrl]);

  // The picture in the centre: the one chosen here, or the song's cover.
  // Loaded once and drawn from memory on every frame.
  const albumArtSource = customAlbumArt || song?.coverUrl || '';
  useEffect(() => {
    albumArtImageRef.current = null;
    if (!albumArtSource) return;
    const img = new Image();
    img.crossOrigin = 'anonymous';
    // External pictures come through the service, which the canvas may read.
    img.src = albumArtSource.startsWith('http') ? `/v1/proxy/image?url=${encodeURIComponent(albumArtSource)}` : albumArtSource;
    img.onload = () => {
      albumArtImageRef.current = img;
    };
    img.onerror = () => {
      console.error('[ERROR] the centre picture did not load:', albumArtSource);
    };
  }, [albumArtSource]);

  // Initialize Audio & Canvas
  useEffect(() => {
    if (!isOpen || !song) return;

    // Reset basics
    setIsPlaying(false);
    setIsExporting(false);
    setExportProgress(0);
    setExportStage('idle');

    // Audio Setup
    const audio = new Audio();
    audio.crossOrigin = "anonymous";
    audio.src = song.audioUrl || '';
    audioRef.current = audio;

    const AudioContextClass = window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
    const audioCtx = new AudioContextClass();
    audioContextRef.current = audioCtx;

    const analyser = audioCtx.createAnalyser();
    analyser.fftSize = 2048;
    analyserRef.current = analyser;

    const source = audioCtx.createMediaElementSource(audio);
    source.connect(analyser);
    analyser.connect(audioCtx.destination);

    audio.onended = () => {
      setIsPlaying(false);
    };

    // Start Loop
    cancelAnimationFrame(animationRef.current);
    renderLoop();

    return () => {
      audio.pause();
      if (audioContextRef.current?.state !== 'closed') {
        audioContextRef.current?.close();
      }
      cancelAnimationFrame(animationRef.current);
    };
  }, [isOpen, song]); 

  const togglePlay = async () => {
    if (!audioRef.current || !audioContextRef.current) return;
    if (audioContextRef.current.state === 'suspended') {
      await audioContextRef.current.resume();
    }
    if (isPlaying) {
      audioRef.current.pause();
    } else {
      await audioRef.current.play();
      setPlaybackTime(audioRef.current.currentTime);
      setPlaybackDuration(audioRef.current.duration || 0);
    }
    setIsPlaying(!isPlaying);
  };

  const startRecording = async () => {
    if (!canvasRef.current || !song) return;

    // Server-side ffmpeg — no WASM loading needed

    setIsExporting(true);
    setExportStage('capturing');
    setExportProgress(0);

    try {
      await renderOffline();
    } catch (error) {
      console.error('Rendering failed:', error);
      // an agent's render reports to the agent, not to a dialog nobody watches
      if (exportTargetRef.current) {
        exportTargetRef.current = null;
        setAgentExportError(error instanceof Error ? error.message : String(error));
      } else {
        alert('Video rendering failed. Please try again.');
      }
      setIsExporting(false);
      setExportStage('idle');
    }
  };

  const analyzeAudioOffline = async (audioBuffer: AudioBuffer, fps: number): Promise<Uint8Array[]> => {
    const duration = audioBuffer.duration;
    const totalFrames = Math.ceil(duration * fps);
    const samplesPerFrame = Math.floor(audioBuffer.sampleRate / fps);
    const fftSize = 2048;
    const frequencyBinCount = fftSize / 2;

    // Get raw audio data from first channel
    const channelData = audioBuffer.getChannelData(0);
    const frequencyDataFrames: Uint8Array[] = [];
    const smoothing = 0.8; // Match AnalyserNode default smoothingTimeConstant
    let prevFrame = new Float32Array(frequencyBinCount);

    for (let frame = 0; frame < totalFrames; frame++) {
      const startSample = frame * samplesPerFrame;
      const endSample = Math.min(startSample + fftSize, channelData.length);

      const frameData = new Uint8Array(frequencyBinCount);
      const rawFrame = new Float32Array(frequencyBinCount);

      for (let bin = 0; bin < frequencyBinCount; bin++) {
        let sum = 0;
        const binSize = Math.max(1, Math.floor((endSample - startSample) / frequencyBinCount));
        const binStart = startSample + bin * binSize;
        const binEnd = Math.min(binStart + binSize, endSample);

        for (let i = binStart; i < binEnd && i < channelData.length; i++) {
          sum += Math.abs(channelData[i]);
        }

        const avg = binSize > 0 ? sum / binSize : 0;
        // Apply exponential smoothing like AnalyserNode
        rawFrame[bin] = smoothing * prevFrame[bin] + (1 - smoothing) * avg;
        frameData[bin] = Math.min(255, Math.floor(rawFrame[bin] * 512));
      }

      prevFrame = rawFrame;
      frequencyDataFrames.push(frameData);
    }

    return frequencyDataFrames;
  };

  const loadImageAsDataUrl = async (url: string): Promise<string | null> => {
    try {
      // Use proxy for external URLs to avoid CORS issues
      const isExternal = url.startsWith('http') && !url.includes(window.location.host);
      const fetchUrl = isExternal ? `/v1/proxy/image?url=${encodeURIComponent(url)}` : url;

      const response = await fetch(fetchUrl);
      const blob = await response.blob();
      return new Promise((resolve) => {
        const reader = new FileReader();
        reader.onload = () => resolve(reader.result as string);
        reader.onerror = () => resolve(null);
        reader.readAsDataURL(blob);
      });
    } catch {
      return null;
    }
  };

  const renderOffline = async () => {
    if (!song) return;

    // Create a separate clean canvas to avoid tainted canvas issues
    const canvas = document.createElement('canvas');
    const res = RESOLUTIONS[configRef.current.aspectRatio || '16:9'];
    canvas.width = res.width;
    canvas.height = res.height;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const fps = 30;
    const width = canvas.width;
    const height = canvas.height;
    const currentConfig = configRef.current;
    const centerX = (currentConfig.visualizerX / 100) * width;
    const centerY = (currentConfig.visualizerY / 100) * height;

    setExportProgress(1);

    // Pre-load images via proxy to avoid CORS/tainted canvas issues
    let bgImage: HTMLImageElement | null = null;
    let bgVideo: HTMLVideoElement | null = null;
    let albumImage: HTMLImageElement | null = null;

    // Load background video or image
    if (backgroundType === 'video' && videoUrl) {
      bgVideo = document.createElement('video');
      bgVideo.crossOrigin = 'anonymous';
      bgVideo.src = videoUrl;
      bgVideo.muted = true;
      bgVideo.playsInline = true;
      await new Promise<void>((resolve) => {
        bgVideo!.onloadeddata = () => resolve();
        bgVideo!.onerror = () => {
          console.warn('Failed to load background video, falling back to image');
          bgVideo = null;
          resolve();
        };
        bgVideo!.load();
      });
    } else if (bgImageRef.current) {
      bgImage = bgImageRef.current;
    }

    // Load album art (use custom if set, otherwise song cover)
    const albumArtSource = customAlbumArt || song.coverUrl;
    if (albumArtSource) {
      // Custom album art might already be a data URL
      const albumDataUrl = albumArtSource.startsWith('data:')
        ? albumArtSource
        : await loadImageAsDataUrl(albumArtSource);
      if (albumDataUrl) {
        albumImage = new Image();
        albumImage.src = albumDataUrl;
        await new Promise<void>((resolve) => {
          albumImage!.onload = () => resolve();
          albumImage!.onerror = () => resolve();
        });
      }
    }

    // Fetch and decode audio
    setExportProgress(2);
    const audioUrl = song.audioUrl || '';
    const audioResponse = await fetch(audioUrl);
    const audioArrayBuffer = await audioResponse.arrayBuffer();

    // Keep a copy for FFmpeg
    const audioDataCopy = audioArrayBuffer.slice(0);

    setExportProgress(5);

    // Decode audio for analysis
    const audioCtx = new (window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext)();
    const audioBuffer = await audioCtx.decodeAudioData(audioArrayBuffer);
    const duration = audioBuffer.duration;
    const totalFrames = Math.ceil(duration * fps);

    setExportProgress(10);

    // Analyze audio to get frequency data for each frame
    const frequencyDataFrames = await analyzeAudioOffline(audioBuffer, fps);

    setExportProgress(15);

    // Render all frames
    const currentEffects = effectsRef.current;
    const currentIntensities = intensitiesRef.current;
    const currentTexts = textLayersRef.current;
    const capturedFrames: string[] = [];
    // One GPU encoder for the whole export, or none and the JPEG path below.
    const hardware: HardwareEncoder | null = await createHardwareEncoder({ width, height, fps }).catch(() => null);

    for (let frameIndex = 0; frameIndex < totalFrames; frameIndex++) {
      const time = frameIndex / fps;
      const dataArray = frequencyDataFrames[frameIndex] || new Uint8Array(1024);

      // Create time domain data (simple sine wave approximation based on bass)
      const timeDomain = new Uint8Array(1024);
      let bassSum = 0;
      for (let i = 0; i < 20; i++) bassSum += dataArray[i];
      const bassLevel = bassSum / 20 / 255;
      for (let i = 0; i < timeDomain.length; i++) {
        timeDomain[i] = 128 + Math.sin(i * 0.1 + time * 10) * 64 * bassLevel;
      }

      // Calculate bass and pulse
      let bass = 0;
      for (let i = 0; i < 20; i++) bass += dataArray[i];
      bass = bass / 20;
      const normBass = bass / 255;
      const pulse = 1 + normBass * 0.15;

      // Clear canvas
      ctx.globalCompositeOperation = 'source-over';
      ctx.fillStyle = '#000';
      ctx.fillRect(0, 0, width, height);

      // Draw background (video or image)
      let bgSource: HTMLImageElement | HTMLVideoElement | null = bgImage;

      if (bgVideo) {
        // Seek video to current frame time (loop if video is shorter)
        const videoTime = time % (bgVideo.duration || 1);
        bgVideo.currentTime = videoTime;
        // Wait for seek to complete
        await new Promise<void>((resolve) => {
          const onSeeked = () => {
            bgVideo!.removeEventListener('seeked', onSeeked);
            resolve();
          };
          bgVideo!.addEventListener('seeked', onSeeked);
          // Fallback timeout in case seeked never fires
          setTimeout(resolve, 50);
        });
        bgSource = bgVideo;
      }

      if (bgSource) {
        ctx.save();
        ctx.globalAlpha = 1 - currentConfig.bgDim;

        if (currentEffects.shake && normBass > (0.6 - (currentIntensities.shake * 0.3))) {
          const magnitude = currentIntensities.shake * 50;
          const shakeX = (Math.random() - 0.5) * magnitude * normBass;
          const shakeY = (Math.random() - 0.5) * magnitude * normBass;
          ctx.translate(shakeX, shakeY);
        }

        const zoom = 1.05 + (Math.sin(time * 0.5) * 0.05);
        ctx.translate(width / 2, height / 2); // Background always zooms from screen center
        ctx.scale(zoom, zoom);
        drawImageCover(ctx, bgSource, 0, 0, width, height);
        ctx.restore();
      }

      // Draw preset (scaled)
      ctx.save();
      if (currentEffects.shake && normBass > 0.6) {
        const magnitude = currentIntensities.shake * 30;
        const shakeX = (Math.random() - 0.5) * magnitude * normBass;
        const shakeY = (Math.random() - 0.5) * magnitude * normBass;
        ctx.translate(shakeX, shakeY);
      }
      // Apply visualizer scale around its center
      const vScale = currentConfig.visualizerScale || 1.0;
      if (vScale !== 1.0) {
        ctx.translate(centerX, centerY);
        ctx.scale(vScale, vScale);
        ctx.translate(-centerX, -centerY);
      }

      switch(currentConfig.preset) {
        case 'NCS Circle':
          drawNCSCircle(ctx, centerX, centerY, dataArray, pulse, time, currentConfig.primaryColor, currentConfig.secondaryColor);
          break;
        case 'Linear Bars':
          drawLinearBars(ctx, width, height, dataArray, currentConfig.primaryColor, currentConfig.secondaryColor);
          break;
        case 'Dual Mirror':
          drawDualMirror(ctx, width, height, dataArray, currentConfig.primaryColor);
          break;
        case 'Center Wave':
          drawCenterWave(ctx, centerX, centerY, dataArray, time, currentConfig.primaryColor);
          break;
        case 'Orbital':
          drawOrbital(ctx, centerX, centerY, dataArray, time, currentConfig.primaryColor, currentConfig.secondaryColor);
          break;
        case 'Hexagon':
          drawHexagon(ctx, centerX, centerY, dataArray, pulse, time, currentConfig.primaryColor);
          break;
        case 'Oscilloscope':
          drawOscilloscope(ctx, width, height, timeDomain, currentConfig.primaryColor);
          break;
        case 'Digital Rain':
          drawDigitalRain(ctx, width, height, dataArray, time, currentConfig.primaryColor);
          break;
        case 'Shockwave':
          drawShockwave(ctx, centerX, centerY, bass, time, currentConfig.primaryColor);
          break;
      }

      drawParticles(ctx, width, height, time, bass, currentConfig.particleCount, currentConfig.primaryColor);

      if (['NCS Circle', 'Hexagon', 'Orbital', 'Shockwave'].includes(currentConfig.preset) && albumImage) {
        // Draw album art inline with pre-loaded image
        ctx.save();
        ctx.translate(centerX, centerY);
        ctx.scale(pulse, pulse);
        ctx.shadowBlur = 40;
        ctx.shadowColor = currentConfig.primaryColor;
        ctx.beginPath();
        ctx.arc(0, 0, 150, 0, Math.PI * 2);
        ctx.closePath();
        ctx.lineWidth = 5;
        ctx.strokeStyle = 'white';
        ctx.stroke();
        ctx.clip();
        ctx.drawImage(albumImage, -150, -150, 300, 300);
        ctx.restore();
      }

      // Pixelate effect (applied before text so text stays sharp)
      if (currentEffects.pixelate) {
        const pixelSize = Math.max(4, Math.floor(16 * currentIntensities.pixelate));
        ctx.imageSmoothingEnabled = false;
        const tempCanvas2 = document.createElement('canvas');
        const smallW = Math.floor(width / pixelSize);
        const smallH = Math.floor(height / pixelSize);
        tempCanvas2.width = smallW;
        tempCanvas2.height = smallH;
        const tempCtx2 = tempCanvas2.getContext('2d')!;
        tempCtx2.drawImage(canvas, 0, 0, smallW, smallH);
        ctx.clearRect(0, 0, width, height);
        ctx.drawImage(tempCanvas2, 0, 0, smallW, smallH, 0, 0, width, height);
        ctx.imageSmoothingEnabled = true;
      }

      ctx.restore(); // End visualizer scale context

      // --- 3. CUSTOM TEXT LAYERS ---
      ctx.save();
      ctx.shadowBlur = 10;
      ctx.shadowColor = 'black';
      ctx.textAlign = 'center';

      currentTexts.forEach(layer => {
        ctx.fillStyle = layer.color;
        const dynamicSize = layer.id === '1' && currentConfig.preset === 'Minimal' ? layer.size * pulse : layer.size;
        ctx.font = `bold ${dynamicSize}px ${layer.font}, sans-serif`;
        const xPos = (layer.x / 100) * width;
        const yPos = (layer.y / 100) * height;
        ctx.fillText(layer.text, xPos, yPos);
      });

      ctx.restore();

      // --- SYNCED LYRICS OVERLAY (export) ---
      if (lyricsEnabledRef.current && lrcLinesRef.current.length > 0) {
        const lrcTime = time + lyricsOffsetRef.current;
        const lrcLines = lrcLinesRef.current;
        const lrcStyle = lyricsStyleRef.current;
        const lrcFontSize = lyricsFontSizeRef.current * (width / 1920);
        const lrcMaxLines = lyricsLinesRef.current;
        const lrcShowSections = lyricsShowSectionsRef.current;
        const lrcX = (currentConfig.lyricsX / 100) * width;
        const lrcY = (currentConfig.lyricsY / 100) * height;

        let lrcIdx = -1;
        for (let li = lrcLines.length - 1; li >= 0; li--) {
          if (lrcTime >= lrcLines[li].time) { lrcIdx = li; break; }
        }

        if (lrcIdx >= 0) {
          ctx.save();
          ctx.font = `bold ${lrcFontSize}px Inter, sans-serif`;
          ctx.textAlign = 'center';
          ctx.textBaseline = 'top';
          const lrcLine = lrcLines[lrcIdx];

          if (lrcStyle === 'karaoke' && !(!lrcShowSections && lrcLine.isSection)) {
            const nextT = Math.min(lrcIdx + 1 < lrcLines.length ? lrcLines[lrcIdx + 1].time : lrcLine.time + 5, lrcLine.time + 5);
            // Word times drive the sweep when the file has them; a line time
            // alone can only be swept linearly, and that is what drifts.
            const prog = lineProgress(lrcLine, lrcTime, nextT);
            // Hide line if fully sung (prog=1) for more than 1s
            if (prog >= 1 && lrcTime > nextT + 1) { /* skip — line already sung */ } else {
            const m = ctx.measureText(lrcLine.text);
            const bx = lrcX - m.width / 2;
            const pw = m.width + lrcFontSize * 0.8;
            const ph = lrcFontSize * 1.3;
            ctx.fillStyle = `rgba(${parseInt(lyricsBgColorRef.current.slice(1,3),16)},${parseInt(lyricsBgColorRef.current.slice(3,5),16)},${parseInt(lyricsBgColorRef.current.slice(5,7),16)}, ${lyricsBgOpacityRef.current/100})`;
            ctx.beginPath(); ctx.roundRect(lrcX - pw/2, lrcY - lrcFontSize*0.15, pw, ph, ph/2); ctx.fill();
            ctx.textAlign = 'left';
            ctx.fillStyle = `${lyricsColorRef.current}4d`;
            ctx.strokeStyle = 'rgba(0,0,0,0.6)'; ctx.lineWidth = 2;
            ctx.strokeText(lrcLine.text, bx, lrcY); ctx.fillText(lrcLine.text, bx, lrcY);
            ctx.save();
            ctx.beginPath(); ctx.rect(bx, lrcY - lrcFontSize*0.2, m.width * prog, lrcFontSize*1.4); ctx.clip();
            ctx.fillStyle = lyricsHighlightColorRef.current;
            ctx.strokeText(lrcLine.text, bx, lrcY); ctx.fillText(lrcLine.text, bx, lrcY);
            ctx.restore();
            }
          } else if (lrcStyle === 'scroll' && !(!lrcShowSections && lrcLine.isSection)) {
            const nextT = Math.min(lrcIdx + 1 < lrcLines.length ? lrcLines[lrcIdx + 1].time : lrcLine.time + 5, lrcLine.time + 5);
            // Word times drive the sweep when the file has them; a line time
            // alone can only be swept linearly, and that is what drifts.
            const prog = lineProgress(lrcLine, lrcTime, nextT);
            const m = ctx.measureText(lrcLine.text);
            const scrollOff = prog * (m.width + width * 0.5);
            ctx.fillStyle = `rgba(${parseInt(lyricsBgColorRef.current.slice(1,3),16)},${parseInt(lyricsBgColorRef.current.slice(3,5),16)},${parseInt(lyricsBgColorRef.current.slice(5,7),16)}, ${lyricsBgOpacityRef.current/100})`;
            ctx.fillRect(0, lrcY - lrcFontSize*0.15, width, lrcFontSize*1.3);
            ctx.textAlign = 'left';
            ctx.fillStyle = lyricsColorRef.current;
            ctx.strokeStyle = 'rgba(0,0,0,0.8)'; ctx.lineWidth = 2;
            ctx.strokeText(lrcLine.text, width - scrollOff, lrcY);
            ctx.fillText(lrcLine.text, width - scrollOff, lrcY);
          } else {
            const lineH = lrcFontSize * 1.6;
            const vis: {text:string; isCur:boolean}[] = [];
            for (let o = 0; o < lrcMaxLines && lrcIdx + o < lrcLines.length; o++) {
              const l = lrcLines[lrcIdx + o];
              if (!lrcShowSections && l.isSection) continue;
              vis.push({text: l.text, isCur: o === 0});
            }
            const bY = lrcY - (lineH * vis.length) / 2;
            vis.forEach((v, vi) => {
              const y = bY + vi * lineH;
              const a = v.isCur ? 1.0 : 0.4;
              const m = ctx.measureText(v.text);
              ctx.fillStyle = `rgba(${parseInt(lyricsBgColorRef.current.slice(1,3),16)},${parseInt(lyricsBgColorRef.current.slice(3,5),16)},${parseInt(lyricsBgColorRef.current.slice(5,7),16)}, ${(lyricsBgOpacityRef.current/100)*a})`;
              ctx.beginPath(); ctx.roundRect(lrcX - (m.width+lrcFontSize*0.8)/2, y-lrcFontSize*0.15, m.width+lrcFontSize*0.8, lrcFontSize*1.3, lrcFontSize*0.65); ctx.fill();
              ctx.fillStyle = `rgba(${parseInt(lyricsColorRef.current.slice(1,3),16)},${parseInt(lyricsColorRef.current.slice(3,5),16)},${parseInt(lyricsColorRef.current.slice(5,7),16)}, ${a})`;
              ctx.strokeStyle = `rgba(0,0,0,${0.8*a})`; ctx.lineWidth = 3;
              ctx.strokeText(v.text, lrcX, y); ctx.fillText(v.text, lrcX, y);
            });
          }
          ctx.restore();
        }
      }

      // Apply post-processing effects
      if (currentEffects.scanlines || currentEffects.cctv) {
        ctx.fillStyle = `rgba(0,0,0,${currentIntensities.scanlines * 0.8})`;
        for (let i = 0; i < height; i += 4) {
          ctx.fillRect(0, i, width, 2);
        }
      }

      if (currentEffects.vhs || currentEffects.chromatic || (currentEffects.glitch && Math.random() > (1 - currentIntensities.glitch))) {
        const intensity = currentEffects.vhs ? currentIntensities.vhs : currentIntensities.chromatic;
        const offset = (10 * intensity) * normBass;
        ctx.globalCompositeOperation = 'screen';
        ctx.fillStyle = `rgba(255,0,0,${0.2 * intensity})`;
        ctx.fillRect(-offset, 0, width, height);
        ctx.fillStyle = `rgba(0,0,255,${0.2 * intensity})`;
        ctx.fillRect(offset, 0, width, height);
        ctx.globalCompositeOperation = 'source-over';
      }

      if (currentEffects.glitch && Math.random() > (1 - currentIntensities.glitch)) {
        ctx.fillStyle = Math.random() > 0.5 ? currentConfig.primaryColor : '#fff';
        ctx.fillRect(Math.random() * width, Math.random() * height, Math.random() * 200, 4);
      }

      if (currentEffects.cctv) {
        const intensity = currentIntensities.cctv;
        ctx.globalCompositeOperation = 'overlay';
        ctx.fillStyle = `rgba(0, 50, 0, ${0.4 * intensity})`;
        ctx.fillRect(0, 0, width, height);

        const grad = ctx.createRadialGradient(centerX, centerY, height * 0.4, centerX, centerY, height * 0.9);
        grad.addColorStop(0, 'transparent');
        grad.addColorStop(1, 'black');
        ctx.globalCompositeOperation = 'multiply';
        ctx.fillStyle = grad;
        ctx.fillRect(0, 0, width, height);

        // Date stamp + REC
        ctx.globalCompositeOperation = 'source-over';
        const cctvFs = Math.round(width * 0.022);
        ctx.font = `bold ${cctvFs}px monospace`;
        ctx.fillStyle = 'white';
        ctx.shadowColor = 'black';
        ctx.shadowBlur = 4;
        const frameDate = new Date(Date.now() + time * 1000);
        ctx.fillText(frameDate.toLocaleString().toUpperCase(), cctvFs * 2, cctvFs * 2);
        ctx.fillText('REC \u25cf', width - cctvFs * 6, cctvFs * 2);
        ctx.shadowBlur = 0;
        ctx.globalCompositeOperation = 'source-over';
      }

      // Bloom / Glow effect
      if (currentEffects.bloom) {
        const intensity = currentIntensities.bloom;
        ctx.globalCompositeOperation = 'screen';
        ctx.filter = `blur(${15 * intensity}px)`;
        ctx.globalAlpha = 0.4 * intensity;
        ctx.drawImage(canvas, 0, 0);
        ctx.filter = 'none';
        ctx.globalAlpha = 1;
        ctx.globalCompositeOperation = 'source-over';
      }

      // Film Grain
      if (currentEffects.filmGrain) {
        const intensity = currentIntensities.filmGrain;
        const imageData = ctx.getImageData(0, 0, width, height);
        const data = imageData.data;
        const grainAmount = intensity * 50;
        for (let i = 0; i < data.length; i += 16) {
          const noise = (Math.random() - 0.5) * grainAmount;
          data[i] += noise;
          data[i + 1] += noise;
          data[i + 2] += noise;
        }
        ctx.putImageData(imageData, 0, 0);
      }

      // Strobe effect
      if (currentEffects.strobe && normBass > (0.7 - currentIntensities.strobe * 0.3)) {
        ctx.globalCompositeOperation = 'screen';
        ctx.fillStyle = `rgba(255, 255, 255, ${currentIntensities.strobe * normBass * 0.8})`;
        ctx.fillRect(0, 0, width, height);
        ctx.globalCompositeOperation = 'source-over';
      }

      // Vignette effect
      if (currentEffects.vignette) {
        const intensity = currentIntensities.vignette;
        const grad = ctx.createRadialGradient(centerX, centerY, height * 0.3, centerX, centerY, height * 0.8);
        grad.addColorStop(0, 'transparent');
        grad.addColorStop(1, `rgba(0, 0, 0, ${0.8 * intensity})`);
        ctx.fillStyle = grad;
        ctx.fillRect(0, 0, width, height);
      }

      // Hue Shift effect
      if (currentEffects.hueShift) {
        const hueRotation = currentIntensities.hueShift * 360 * (1 + normBass * 0.5);
        ctx.filter = `hue-rotate(${hueRotation}deg)`;
        ctx.drawImage(canvas, 0, 0);
        ctx.filter = 'none';
      }

      // Letterbox effect
      if (currentEffects.letterbox) {
        const barHeight = height * 0.12 * currentIntensities.letterbox;
        ctx.fillStyle = 'black';
        ctx.fillRect(0, 0, width, barHeight);
        ctx.fillRect(0, height - barHeight, width, barHeight);
      }

      // Straight to the GPU encoder when the machine has one; the JPEG round
      // trip below is the fallback, and it is what made this slow.
      if (hardware) {
        await hardware.encode(canvas, frameIndex);
      } else {
        const frameData = canvas.toDataURL('image/jpeg', 0.85);
        capturedFrames.push(frameData.split(',')[1]);
      }

      // Update progress + yield to prevent Chrome "page not responding" alert
      if (frameIndex % 30 === 0) {
        setExportProgress(15 + Math.round((frameIndex / totalFrames) * 55));
        await new Promise(r => setTimeout(r, 0));
      }
    }

    setExportStage('encoding');
    setExportProgress(70);

    /// Hands the finished file to the user and tears the export down. Both the
    /// hardware and the WebAssembly path end here.
    const finishExport = async (blob: Blob) => {
      const target = exportTargetRef.current;
      if (target) {
        exportTargetRef.current = null;
        try {
          await target(blob);
        } catch (problem) {
          setAgentExportError(problem instanceof Error ? problem.message : String(problem));
        }
        await audioCtx.close().catch(() => undefined);
        setExportProgress(100);
        setIsExporting(false);
        setExportStage('idle');
        return;
      }
      void saveFile(`${song.title || STUDIO.slug}.mp4`, { blob });
      await audioCtx.close().catch(() => undefined);
      setExportProgress(100);
      setTimeout(() => {
        setIsExporting(false);
        setExportStage('idle');
      }, 500);
    };

    // The hardware path has already encoded the video; all that is left is
    // putting the song next to it, which is a mux and a short audio encode
    // rather than re-encoding every frame.
    if (hardware) {
      const silent = await hardware.finish();
      const audioSource = song.audioUrl || (song as unknown as { audio_url?: string }).audio_url || '';
      if (!audioSource) {
        await finishExport(new Blob([silent], { type: 'video/mp4' }));
        return;
      }
      const ffmpeg = new FFmpeg();
      ffmpeg.on('progress', ({ progress: value }) => {
        setExportProgress(80 + Math.round(Math.max(0, Math.min(1, value)) * 18));
      });
      await ffmpeg.load({
        coreURL: await toBlobURL('/ffmpeg/ffmpeg-core.js', 'text/javascript'),
        wasmURL: await toBlobURL('/ffmpeg/ffmpeg-core.wasm', 'application/wasm'),
      });
      const audioExtension = audioSource.toLowerCase().includes('.wav') ? 'wav' : 'mp3';
      await ffmpeg.writeFile('video.mp4', silent);
      await ffmpeg.writeFile(`audio.${audioExtension}`, new Uint8Array(await (await fetch(audioSource)).arrayBuffer()));
      await ffmpeg.exec([
        '-i', 'video.mp4',
        '-i', `audio.${audioExtension}`,
        // The picture is already H.264: copy it rather than encode it twice.
        '-c:v', 'copy', '-c:a', 'aac', '-b:a', '192k', '-shortest',
        '-movflags', '+faststart',
        'out.mp4',
      ]);
      const merged = (await ffmpeg.readFile('out.mp4')) as Uint8Array;
      if (merged.length === 0) throw new Error('The encoder produced an empty file.');
      await finishExport(new Blob([merged], { type: 'video/mp4' }));
      return;
    }

    // Assemble the captured frames locally. ffmpeg is compiled to WebAssembly
    // and served from the application itself, so no service and no network is
    // involved in producing the file.
    const ffmpeg = new FFmpeg();
    ffmpeg.on('progress', ({ progress: value }) => {
      setExportProgress(70 + Math.round(Math.max(0, Math.min(1, value)) * 28));
    });
    await ffmpeg.load({
      coreURL: await toBlobURL('/ffmpeg/ffmpeg-core.js', 'text/javascript'),
      wasmURL: await toBlobURL('/ffmpeg/ffmpeg-core.wasm', 'application/wasm'),
    });

    for (let index = 0; index < capturedFrames.length; index++) {
      const binary = atob(capturedFrames[index]);
      const bytes = new Uint8Array(binary.length);
      for (let byte = 0; byte < binary.length; byte++) bytes[byte] = binary.charCodeAt(byte);
      await ffmpeg.writeFile(`frame${String(index).padStart(6, '0')}.jpg`, bytes);
    }

    const audioSource = song.audioUrl || (song as unknown as { audio_url?: string }).audio_url || '';
    const audioExtension = audioSource.toLowerCase().includes('.wav') ? 'wav' : 'mp3';
    if (audioSource) {
      const audioBytes = new Uint8Array(await (await fetch(audioSource)).arrayBuffer());
      await ffmpeg.writeFile(`audio.${audioExtension}`, audioBytes);
    }

    const encodeArguments = [
      '-framerate', String(fps),
      '-i', 'frame%06d.jpg',
      ...(audioSource ? ['-i', `audio.${audioExtension}`] : []),
      '-c:v', 'libx264', '-preset', 'veryfast', '-pix_fmt', 'yuv420p',
      ...(audioSource ? ['-c:a', 'aac', '-b:a', '192k', '-shortest'] : []),
      '-movflags', '+faststart',
      'out.mp4',
    ];
    await ffmpeg.exec(encodeArguments);

    const outputData = (await ffmpeg.readFile('out.mp4')) as Uint8Array;
    if (outputData.length === 0) {
      throw new Error('The encoder produced an empty file.');
    }
    setExportProgress(98);
    await finishExport(new Blob([outputData], { type: 'video/mp4' }));
  };

  const stopRecording = () => {
    // For offline rendering, we can't really stop mid-process
    // This is kept for compatibility but offline render runs to completion
  };

  const handleFileUpload = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (file) {
        const reader = new FileReader();
        reader.onload = (ev) => {
            const result = ev.target?.result as string;
            setCustomImage(result);
            setBackgroundType('custom');
        };
        reader.readAsDataURL(file);
    }
  };

  const handleVideoFileUpload = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (file) {
      const url = URL.createObjectURL(file);
      setVideoUrl(url);
      setBackgroundType('video');
    }
  };

  const searchPexels = async (query: string, type: 'photos' | 'videos') => {
    setPexelsLoading(true);
    setPexelsError(null);
    try {
      // Pexels is called directly. The old server route existed only to attach
      // the key; the key belongs to the user and is already stored locally.
      if (!pexelsApiKey) {
        setPexelsError('API key required');
        setShowPexelsApiKeyInput(true);
        return;
      }
      const endpoint = type === 'photos'
        ? `https://api.pexels.com/v1/search?per_page=24&query=${encodeURIComponent(query)}`
        : `https://api.pexels.com/videos/search?per_page=24&query=${encodeURIComponent(query)}`;

      const response = await fetch(endpoint, { headers: { Authorization: pexelsApiKey } });
      const data = await response.json();

      if (!response.ok) {
        if (response.status === 400 || response.status === 401) {
          setPexelsError(data.error || 'API key required');
          setShowPexelsApiKeyInput(true);
        } else {
          setPexelsError(data.error || 'Search failed');
        }
        return;
      }

      if (type === 'photos') {
        setPexelsPhotos(data.photos || []);
      } else {
        setPexelsVideos(data.videos || []);
      }
    } catch (error) {
      console.error('Pexels search failed:', error);
      setPexelsError('Search failed. Please try again.');
    } finally {
      setPexelsLoading(false);
    }
  };

  const savePexelsApiKey = (key: string) => {
    setPexelsApiKey(key);
    localStorage.setItem('pexels_api_key', key);
    setShowPexelsApiKeyInput(false);
    setPexelsError(null);
    // Retry search with new key
    if (key) {
      searchPexels(pexelsQuery, pexelsTab);
    }
  };

  const selectPexelsPhoto = (photo: PexelsPhoto) => {
    if (pexelsTarget === 'albumArt') {
      setCustomAlbumArt(photo.src.large);
    } else {
      setCustomImage(photo.src.large);
      setBackgroundType('custom');
    }
    setShowPexelsBrowser(false);
  };

  const selectPexelsVideo = (video: PexelsVideo) => {
    // Get best quality video file (prefer HD)
    const hdFile = video.video_files.find(f => f.quality === 'hd' && f.width >= 1280);
    const sdFile = video.video_files.find(f => f.quality === 'sd');
    const videoFile = hdFile || sdFile || video.video_files[0];
    if (videoFile) {
      setVideoUrl(videoFile.link);
      setBackgroundType('video');
      setShowPexelsBrowser(false);
    }
  };

  const openPexelsBrowser = (target: 'background' | 'albumArt' = 'background', tab: 'photos' | 'videos' = 'photos') => {
    setPexelsTarget(target);
    setPexelsTab(target === 'albumArt' ? 'photos' : tab); // Album art is always photos
    setShowPexelsBrowser(true);
    const searchTab = target === 'albumArt' ? 'photos' : tab;
    if ((searchTab === 'photos' && pexelsPhotos.length === 0) || (searchTab === 'videos' && pexelsVideos.length === 0)) {
      searchPexels(pexelsQuery, searchTab);
    }
  };

  const handleAlbumArtUpload = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (file) {
      const reader = new FileReader();
      reader.onload = (ev) => {
        setCustomAlbumArt(ev.target?.result as string);
      };
      reader.readAsDataURL(file);
    }
  };

  // --- RENDER ENGINE ---
  const renderLoop = () => {
    if (!canvasRef.current || !analyserRef.current || !song) {
        animationRef.current = requestAnimationFrame(renderLoop);
        return;
    }

    const canvas = canvasRef.current;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    // Read current state
    const currentConfig = configRef.current;
    const currentEffects = effectsRef.current;
    const currentIntensities = intensitiesRef.current;
    const currentTexts = textLayersRef.current;

    const width = canvas.width;
    const height = canvas.height;
    const centerX = (currentConfig.visualizerX / 100) * width;
    const centerY = (currentConfig.visualizerY / 100) * height;
    const time = Date.now() / 1000;

    // Data
    const bufferLength = analyserRef.current.frequencyBinCount;
    const dataArray = new Uint8Array(bufferLength);
    const timeDomain = new Uint8Array(bufferLength);
    analyserRef.current.getByteFrequencyData(dataArray);
    analyserRef.current.getByteTimeDomainData(timeDomain);


    // Bass Calc
    let bass = 0;
    for (let i = 0; i < 20; i++) bass += dataArray[i];
    bass = bass / 20;
    const normBass = bass / 255;
    const pulse = 1 + normBass * 0.15;

    // --- 1. CLEAR & BACKGROUND ---
    ctx.globalCompositeOperation = 'source-over';
    ctx.fillStyle = '#000';
    ctx.fillRect(0, 0, width, height);

    // Draw video or image background
    const bgSource = bgVideoRef.current && bgVideoRef.current.readyState >= 2
        ? bgVideoRef.current
        : bgImageRef.current;

    if (bgSource) {
        ctx.save();
        ctx.globalAlpha = 1 - currentConfig.bgDim;

        // Shake Effect (Camera)
        if (currentEffects.shake && normBass > (0.6 - (currentIntensities.shake * 0.3))) {
             const magnitude = currentIntensities.shake * 50;
             const shakeX = (Math.random() - 0.5) * magnitude * normBass;
             const shakeY = (Math.random() - 0.5) * magnitude * normBass;
             ctx.translate(shakeX, shakeY);
        }

        const zoom = 1.05 + (Math.sin(time * 0.5) * 0.05);
        ctx.translate(width / 2, height / 2); // Background always zooms from screen center
        ctx.scale(zoom, zoom);
        drawImageCover(ctx, bgSource, 0, 0, width, height);
        ctx.restore();
    }

    // --- 2. PRESET DRAWING ---
    ctx.save();
    
    // Apply Shake to visual elements
    if (currentEffects.shake && normBass > 0.6) {
         const magnitude = currentIntensities.shake * 30;
         const shakeX = (Math.random() - 0.5) * magnitude * normBass;
         const shakeY = (Math.random() - 0.5) * magnitude * normBass;
         ctx.translate(shakeX, shakeY);
    }
    const vScale = currentConfig.visualizerScale || 1.0;
    if (vScale !== 1.0) {
        ctx.translate(centerX, centerY);
        ctx.scale(vScale, vScale);
        ctx.translate(-centerX, -centerY);
    }

    switch(currentConfig.preset) {
        case 'NCS Circle':
            drawNCSCircle(ctx, centerX, centerY, dataArray, pulse, time, currentConfig.primaryColor, currentConfig.secondaryColor);
            break;
        case 'Linear Bars':
            drawLinearBars(ctx, width, height, dataArray, currentConfig.primaryColor, currentConfig.secondaryColor);
            break;
        case 'Dual Mirror':
            drawDualMirror(ctx, width, height, dataArray, currentConfig.primaryColor);
            break;
        case 'Center Wave':
            drawCenterWave(ctx, centerX, centerY, dataArray, time, currentConfig.primaryColor);
            break;
        case 'Orbital':
            drawOrbital(ctx, centerX, centerY, dataArray, time, currentConfig.primaryColor, currentConfig.secondaryColor);
            break;
        case 'Hexagon':
            drawHexagon(ctx, centerX, centerY, dataArray, pulse, time, currentConfig.primaryColor);
            break;
        case 'Oscilloscope':
            drawOscilloscope(ctx, width, height, timeDomain, currentConfig.primaryColor);
            break;
        case 'Digital Rain':
            drawDigitalRain(ctx, width, height, dataArray, time, currentConfig.primaryColor);
            break;
        case 'Shockwave':
             drawShockwave(ctx, centerX, centerY, bass, time, currentConfig.primaryColor);
             break;
    }
    
    drawParticles(ctx, width, height, time, bass, currentConfig.particleCount, currentConfig.primaryColor);

    if (['NCS Circle', 'Hexagon', 'Orbital', 'Shockwave'].includes(currentConfig.preset)) {
        drawAlbumArt(ctx, centerX, centerY, pulse, currentConfig.primaryColor, albumArtImageRef.current);
    }

    // Pixelate effect (applied before text so text stays sharp)
    if (currentEffects.pixelate) {
        const pixelSize = Math.max(4, Math.floor(16 * currentIntensities.pixelate));
        ctx.imageSmoothingEnabled = false;
        const tempCanvas = document.createElement('canvas');
        const smallW = Math.floor(width / pixelSize);
        const smallH = Math.floor(height / pixelSize);
        tempCanvas.width = smallW;
        tempCanvas.height = smallH;
        const tempCtx = tempCanvas.getContext('2d')!;
        tempCtx.drawImage(canvas, 0, 0, smallW, smallH);
        ctx.clearRect(0, 0, width, height);
        ctx.drawImage(tempCanvas, 0, 0, smallW, smallH, 0, 0, width, height);
        ctx.imageSmoothingEnabled = true;
    }

    ctx.restore(); // End visualizer scale context

    // --- 3. CUSTOM TEXT LAYERS ---
    ctx.save();
    ctx.shadowBlur = 10;
    ctx.shadowColor = 'black';
    ctx.textAlign = 'center';

    const activeLayerId = dragRef.current?.layerId || hoveredLayer;

    currentTexts.forEach(layer => {
        ctx.fillStyle = layer.color;
        const dynamicSize = layer.id === '1' && currentConfig.preset === 'Minimal' ? layer.size * pulse : layer.size;
        ctx.font = `bold ${dynamicSize}px ${layer.font}, sans-serif`;

        const xPos = (layer.x / 100) * width;
        const yPos = (layer.y / 100) * height;

        ctx.fillText(layer.text, xPos, yPos);

        // Selection frame
        if (activeLayerId === layer.id) {
            const metrics = ctx.measureText(layer.text);
            const pad = 8;
            ctx.strokeStyle = '#ec4899';
            ctx.lineWidth = 2;
            ctx.setLineDash([6, 4]);
            ctx.strokeRect(xPos - metrics.width / 2 - pad, yPos - dynamicSize - pad, metrics.width + pad * 2, dynamicSize * 1.3 + pad * 2);
            ctx.setLineDash([]);
        }
    });

    // Visualizer selection frame
    if (activeLayerId === '__visualizer__') {
        const vRadius = Math.min(width, height) * 0.18;
        ctx.strokeStyle = '#ec4899';
        ctx.lineWidth = 2;
        ctx.setLineDash([6, 4]);
        ctx.strokeRect(centerX - vRadius, centerY - vRadius, vRadius * 2, vRadius * 2);
        ctx.setLineDash([]);
    }

    ctx.restore();

    // --- 3.5 SYNCED LYRICS OVERLAY ---
    if (lyricsEnabledRef.current && lrcLinesRef.current.length > 0) {
      const currentTime = (audioRef.current?.currentTime ?? 0) + lyricsOffsetRef.current;
      const lines = lrcLinesRef.current;
      const showSections = lyricsShowSectionsRef.current;
      const fontSize = lyricsFontSizeRef.current * (width / 1920);
      const maxLines = lyricsLinesRef.current;
      const style = lyricsStyleRef.current;
      const position = lyricsPositionRef.current;

      // Find current line index
      let currentIdx = -1;
      for (let i = lines.length - 1; i >= 0; i--) {
        if (currentTime >= lines[i].time) { currentIdx = i; break; }
      }

      if (currentIdx >= 0) {
        const lineHeight = fontSize * 1.6;
        const lyricsXPos = (currentConfig.lyricsX / 100) * width;
        const lyricsYPos = (currentConfig.lyricsY / 100) * height;

        ctx.save();
        ctx.font = `bold ${fontSize}px Inter, sans-serif`;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'top';

        if (style === 'scroll') {
          // --- SCROLL: horizontal scrolling text ---
          const line = lines[currentIdx];
          if (!showSections && line.isSection) { /* skip */ } else {
            const metrics = ctx.measureText(line.text);
            const nextTime = Math.min(currentIdx + 1 < lines.length ? lines[currentIdx + 1].time : line.time + 5, line.time + 5);
            const progress = Math.min(1, (currentTime - line.time) / (nextTime - line.time));
            const scrollOffset = progress * (metrics.width + width * 0.5);
            const x = width - scrollOffset;

            ctx.fillStyle = `rgba(${parseInt(lyricsBgColorRef.current.slice(1,3),16)},${parseInt(lyricsBgColorRef.current.slice(3,5),16)},${parseInt(lyricsBgColorRef.current.slice(5,7),16)}, ${lyricsBgOpacityRef.current/100})`;
            const pillH = fontSize * 1.3;
            ctx.fillRect(0, lyricsYPos - fontSize * 0.15, width, pillH);

            ctx.fillStyle = lyricsColorRef.current;
            ctx.strokeStyle = 'rgba(0,0,0,0.8)';
            ctx.lineWidth = 2;
            ctx.textAlign = 'left';
            ctx.strokeText(line.text, x, lyricsYPos);
            ctx.fillText(line.text, x, lyricsYPos);
          }

        } else if (style === 'karaoke') {
          // --- KARAOKE: progressive fill left-to-right ---
          const line = lines[currentIdx];
          if (!showSections && line.isSection) { /* skip */ } else {
            const nextTime = Math.min(currentIdx + 1 < lines.length ? lines[currentIdx + 1].time : line.time + 5, line.time + 5);
            const progress = Math.min(1, (currentTime - line.time) / (nextTime - line.time));
            if (progress >= 1 && currentTime > nextTime + 1) { /* skip — already sung */ } else {
            const metrics = ctx.measureText(line.text);
            const textW = metrics.width;
            const baseX = lyricsXPos - textW / 2;

            // Background pill
            const pillW = textW + fontSize * 0.8;
            const pillH = fontSize * 1.3;
            ctx.fillStyle = `rgba(${parseInt(lyricsBgColorRef.current.slice(1,3),16)},${parseInt(lyricsBgColorRef.current.slice(3,5),16)},${parseInt(lyricsBgColorRef.current.slice(5,7),16)}, ${lyricsBgOpacityRef.current/100})`;
            ctx.beginPath();
            ctx.roundRect(lyricsXPos - pillW / 2, lyricsYPos - fontSize * 0.15, pillW, pillH, pillH / 2);
            ctx.fill();

            // Dim text (full line)
            ctx.textAlign = 'left';
            ctx.fillStyle = `${lyricsColorRef.current}4d`;
            ctx.strokeStyle = 'rgba(0,0,0,0.6)';
            ctx.lineWidth = 2;
            ctx.strokeText(line.text, baseX, lyricsYPos);
            ctx.fillText(line.text, baseX, lyricsYPos);

            // Highlighted portion (clip to progress)
            ctx.save();
            ctx.beginPath();
            ctx.rect(baseX, lyricsYPos - fontSize * 0.2, textW * progress, fontSize * 1.4);
            ctx.clip();
            ctx.fillStyle = lyricsHighlightColorRef.current;
            ctx.strokeStyle = 'rgba(0,0,0,0.8)';
            ctx.lineWidth = 2;
            ctx.strokeText(line.text, baseX, lyricsYPos);
            ctx.fillText(line.text, baseX, lyricsYPos);
            ctx.restore();
            }
          }

        } else {
          // --- LINES: default style (multi-line with fade) ---
          const visibleLines: { text: string; isCurrent: boolean }[] = [];
          for (let offset = 0; offset < maxLines && currentIdx + offset < lines.length; offset++) {
            const line = lines[currentIdx + offset];
            if (!showSections && line.isSection) continue;
            visibleLines.push({ text: line.text, isCurrent: offset === 0 });
          }

          const baseY = lyricsYPos - (lineHeight * visibleLines.length) / 2;

          visibleLines.forEach((line, i) => {
            const y = baseY + i * lineHeight;
            const alpha = line.isCurrent ? 1.0 : 0.4;

            const metrics = ctx.measureText(line.text);
            const pillWidth = metrics.width + fontSize * 0.8;
            const pillHeight = fontSize * 1.3;
            ctx.fillStyle = `rgba(0, 0, 0, ${0.5 * alpha})`;
            ctx.beginPath();
            ctx.roundRect(lyricsXPos - pillWidth / 2, y - fontSize * 0.15, pillWidth, pillHeight, pillHeight / 2);
            ctx.fill();

            ctx.fillStyle = `rgba(255, 255, 255, ${alpha})`;
            ctx.strokeStyle = `rgba(0, 0, 0, ${0.8 * alpha})`;
            ctx.lineWidth = 3;
            ctx.strokeText(line.text, lyricsXPos, y);
            ctx.fillText(line.text, lyricsXPos, y);
          });
        }

        // Selection frame
        if (activeLayerId === '__lyrics__') {
            ctx.strokeStyle = '#ec4899';
            ctx.lineWidth = 2;
            ctx.setLineDash([6, 4]);
            const frameW = width * 0.5;
            const frameH = lineHeight * 2;
            ctx.strokeRect(lyricsXPos - frameW / 2, lyricsYPos - frameH / 2, frameW, frameH);
            ctx.setLineDash([]);
        }

        ctx.restore();
      }
    }

    // --- 4. POST-PROCESSING EFFECTS ---
    
    // Scanlines
    if (currentEffects.scanlines || currentEffects.cctv) {
        ctx.fillStyle = `rgba(0,0,0,${currentIntensities.scanlines * 0.8})`;
        for (let i = 0; i < height; i+=4) {
            ctx.fillRect(0, i, width, 2);
        }
    }

    // VHS Color Shift / Chromatic Aberration
    if (currentEffects.vhs || currentEffects.chromatic || (currentEffects.glitch && Math.random() > (1 - currentIntensities.glitch))) {
        const intensity = currentEffects.vhs ? currentIntensities.vhs : currentIntensities.chromatic;
        const offset = (10 * intensity) * normBass;
        ctx.globalCompositeOperation = 'screen';

        // Red Shift - draw colored rectangle offset left
        ctx.fillStyle = `rgba(255,0,0,${0.2 * intensity})`;
        ctx.fillRect(-offset, 0, width, height);

        // Blue Shift - draw colored rectangle offset right
        ctx.fillStyle = `rgba(0,0,255,${0.2 * intensity})`;
        ctx.fillRect(offset, 0, width, height);

        ctx.globalCompositeOperation = 'source-over';
    }

    // Glitch Slices
    if (currentEffects.glitch && Math.random() > (1 - currentIntensities.glitch)) {
        const sliceHeight = Math.random() * 50;
        const sliceY = Math.random() * height;
        const offset = (Math.random() - 0.5) * 40 * currentIntensities.glitch;
        
        ctx.drawImage(canvas, 0, sliceY, width, sliceHeight, offset, sliceY, width, sliceHeight);
        
        // Random colored block
        ctx.fillStyle = Math.random() > 0.5 ? currentConfig.primaryColor : '#fff';
        ctx.fillRect(Math.random()*width, Math.random()*height, Math.random()*200, 4);
    }

    // CCTV Vignette & Grain
    if (currentEffects.cctv) {
        const intensity = currentIntensities.cctv;
        // Green tint
        ctx.globalCompositeOperation = 'overlay';
        ctx.fillStyle = `rgba(0, 50, 0, ${0.4 * intensity})`;
        ctx.fillRect(0, 0, width, height);

        // Vignette
        const grad = ctx.createRadialGradient(centerX, centerY, height * 0.4, centerX, centerY, height * 0.9);
        grad.addColorStop(0, 'transparent');
        grad.addColorStop(1, 'black');
        ctx.globalCompositeOperation = 'multiply';
        ctx.fillStyle = grad;
        ctx.fillRect(0, 0, width, height);

        // Date Stamp
        ctx.globalCompositeOperation = 'source-over';
        const cctvFontSize = Math.round(width * 0.022);
        ctx.font = `bold ${cctvFontSize}px monospace`;
        ctx.fillStyle = 'white';
        ctx.shadowColor = 'black';
        ctx.shadowBlur = 4;
        ctx.fillText(new Date().toLocaleString().toUpperCase(), cctvFontSize * 2, cctvFontSize * 2);
        ctx.fillText("REC \u25cf", width - cctvFontSize * 6, cctvFontSize * 2);
        ctx.shadowBlur = 0;
    }

    // Bloom / Glow effect
    if (currentEffects.bloom) {
        const intensity = currentIntensities.bloom;
        ctx.globalCompositeOperation = 'screen';
        ctx.filter = `blur(${15 * intensity}px)`;
        ctx.globalAlpha = 0.4 * intensity;
        ctx.drawImage(canvas, 0, 0);
        ctx.filter = 'none';
        ctx.globalAlpha = 1;
        ctx.globalCompositeOperation = 'source-over';
    }

    // Film Grain
    if (currentEffects.filmGrain) {
        const intensity = currentIntensities.filmGrain;
        const imageData = ctx.getImageData(0, 0, width, height);
        const data = imageData.data;
        const grainAmount = intensity * 50;
        for (let i = 0; i < data.length; i += 4) {
            const noise = (Math.random() - 0.5) * grainAmount;
            data[i] += noise;
            data[i + 1] += noise;
            data[i + 2] += noise;
        }
        ctx.putImageData(imageData, 0, 0);
    }

    // Strobe effect
    if (currentEffects.strobe && normBass > (0.7 - currentIntensities.strobe * 0.3)) {
        ctx.globalCompositeOperation = 'screen';
        ctx.fillStyle = `rgba(255, 255, 255, ${currentIntensities.strobe * normBass * 0.8})`;
        ctx.fillRect(0, 0, width, height);
        ctx.globalCompositeOperation = 'source-over';
    }

    // Vignette effect
    if (currentEffects.vignette) {
        const intensity = currentIntensities.vignette;
        const grad = ctx.createRadialGradient(centerX, centerY, height * 0.3, centerX, centerY, height * 0.8);
        grad.addColorStop(0, 'transparent');
        grad.addColorStop(1, `rgba(0, 0, 0, ${0.8 * intensity})`);
        ctx.fillStyle = grad;
        ctx.fillRect(0, 0, width, height);
    }

    // Hue Shift effect
    if (currentEffects.hueShift) {
        const hueRotation = currentIntensities.hueShift * 360 * (1 + normBass * 0.5);
        ctx.filter = `hue-rotate(${hueRotation}deg)`;
        ctx.drawImage(canvas, 0, 0);
        ctx.filter = 'none';
    }

    // Letterbox effect
    if (currentEffects.letterbox) {
        const barHeight = height * 0.12 * currentIntensities.letterbox;
        ctx.fillStyle = 'black';
        ctx.fillRect(0, 0, width, barHeight);
        ctx.fillRect(0, height - barHeight, width, barHeight);
    }

    animationRef.current = requestAnimationFrame(renderLoop);
  };

  // --- DRAWING FUNCTIONS ---
  // (Reusing existing drawing functions from previous step, ensuring they use updated args)
  const drawNCSCircle = (ctx: CanvasRenderingContext2D, cx: number, cy: number, data: Uint8Array, pulse: number, time: number, c1: string, c2: string) => {
    const radius = 150 + (pulse - 1) * 50;
    const bars = 80;
    const step = (Math.PI * 2) / bars;
    ctx.save();
    ctx.translate(cx, cy);
    ctx.rotate(time * 0.15);
    for (let i = 0; i < bars; i++) {
        const val = data[i + 10];
        const normalized = val / 255;
        const h = 8 + Math.pow(normalized, 1.5) * 120;
        ctx.save();
        ctx.rotate(i * step);
        const grad = ctx.createLinearGradient(0, radius, 0, radius + h);
        grad.addColorStop(0, c1);
        grad.addColorStop(1, c2);
        ctx.fillStyle = grad;
        ctx.beginPath();
        ctx.roundRect(-3, radius + 10, 6, h, 3);
        ctx.fill();
        ctx.fillStyle = 'rgba(255,255,255,0.15)';
        ctx.beginPath();
        ctx.roundRect(-3, radius + 10 + h + 2, 6, 3, 2);
        ctx.fill();
        ctx.restore();
    }
    ctx.beginPath();
    ctx.arc(0, 0, radius + 150, 0, Math.PI * 2);
    ctx.strokeStyle = 'rgba(255,255,255,0.08)';
    ctx.lineWidth = 1;
    ctx.stroke();
    ctx.restore();
  };

  const drawLinearBars = (ctx: CanvasRenderingContext2D, w: number, h: number, data: Uint8Array, c1: string, c2: string) => {
      const bars = 64;
      const barW = w / bars;
      const gap = 2;
      for(let i=0; i<bars; i++) {
          const val = data[i * 2];
          const normalized = val / 255;
          const barH = 10 + Math.pow(normalized, 1.3) * (h * 0.35);
          const grad = ctx.createLinearGradient(0, h/2, 0, h/2 - barH);
          grad.addColorStop(0, c1);
          grad.addColorStop(1, c2);
          ctx.fillStyle = grad;
          ctx.fillRect(i * barW + gap/2, h/2 - barH, barW - gap, barH);
          ctx.fillStyle = 'rgba(255,255,255,0.2)';
          ctx.fillRect(i * barW + gap/2, h/2, barW - gap, barH * 0.3);
      }
      ctx.fillStyle = 'rgba(255,255,255,0.5)';
      ctx.fillRect(0, h/2, w, 1);
  };

  const drawDualMirror = (ctx: CanvasRenderingContext2D, w: number, h: number, data: Uint8Array, color: string) => {
      const bars = 40;
      const barH = h / bars;
      const cy = h/2;
      for(let i=0; i<bars; i++) {
          const val = data[i*3];
          const normalized = val / 255;
          const len = 20 + Math.pow(normalized, 1.4) * (w * 0.3);
          const alpha = 0.4 + normalized * 0.6;
          ctx.fillStyle = color;
          ctx.globalAlpha = alpha;
          ctx.fillRect(0, cy - (i*barH), len, barH-2);
          ctx.fillRect(0, cy + (i*barH), len, barH-2);
          ctx.fillRect(w - len, cy - (i*barH), len, barH-2);
          ctx.fillRect(w - len, cy + (i*barH), len, barH-2);
      }
      ctx.globalAlpha = 1;
  };

  const drawOrbital = (ctx: CanvasRenderingContext2D, cx: number, cy: number, data: Uint8Array, time: number, c1: string, c2: string) => {
      for(let i=0; i<5; i++) {
          const r = 100 + (i * 55);
          const val = data[i*10];
          const normalized = val / 255;
          const width = 4 + normalized * 6;
          ctx.beginPath();
          ctx.strokeStyle = i % 2 === 0 ? c1 : c2;
          ctx.lineWidth = width;
          ctx.shadowBlur = 20;
          ctx.shadowColor = ctx.strokeStyle;
          const direction = i % 2 === 0 ? 1 : -1;
          const speed = direction * (0.5 + i * 0.1);
          const start = time * speed;
          const arcLength = Math.PI * 1.2 + normalized * Math.PI * 0.3;
          ctx.arc(cx, cy, r, start, start + arcLength);
          ctx.stroke();
      }
      ctx.shadowBlur = 0;
  };

  const drawHexagon = (ctx: CanvasRenderingContext2D, cx: number, cy: number, data: Uint8Array, pulse: number, time: number, color: string) => {
      const sides = 6;
      const r = 180 * pulse;
      ctx.save();
      ctx.translate(cx, cy);
      ctx.rotate(time * 0.4);
      ctx.beginPath();
      ctx.lineWidth = 12;
      ctx.strokeStyle = color;
      ctx.lineJoin = 'round';
      ctx.shadowBlur = 25;
      ctx.shadowColor = color;
      for(let i=0; i<=sides; i++) {
          const angle = i * 2 * Math.PI / sides;
          const x = r * Math.cos(angle);
          const y = r * Math.sin(angle);
          if (i === 0) ctx.moveTo(x, y);
          else ctx.lineTo(x, y);
      }
      ctx.stroke();
      ctx.restore();
      ctx.shadowBlur = 0;
  };

  const drawOscilloscope = (ctx: CanvasRenderingContext2D, w: number, h: number, data: Uint8Array, color: string) => {
      ctx.lineWidth = 3;
      ctx.strokeStyle = color;
      ctx.shadowBlur = 15;
      ctx.shadowColor = color;
      ctx.beginPath();
      const sliceWidth = w / data.length;
      let x = 0;
      for(let i = 0; i < data.length; i++) {
          const normalized = (data[i] - 128) / 128.0;
          const dampened = normalized * 0.6;
          const yPos = (h/2) + (dampened * h/2);
          if(i === 0) ctx.moveTo(x, yPos);
          else ctx.lineTo(x, yPos);
          x += sliceWidth;
      }
      ctx.stroke();

      ctx.strokeStyle = 'rgba(255,255,255,0.1)';
      ctx.shadowBlur = 0;
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(0, h/2);
      ctx.lineTo(w, h/2);
      ctx.stroke();
  };
  
  const drawCenterWave = (ctx: CanvasRenderingContext2D, cx: number, cy: number, data: Uint8Array, time: number, color: string) => {
      ctx.strokeStyle = color;
      ctx.lineWidth = 2;
      ctx.shadowBlur = 8;
      ctx.shadowColor = color;
      for(let i=0; i<12; i++) {
          ctx.beginPath();
          const baseR = 60 + (i * 35);
          const val = data[i*4];
          const normalized = val / 255;
          const r = baseR + Math.pow(normalized, 1.5) * 25;
          ctx.globalAlpha = 0.8 - (i/15);
          ctx.ellipse(cx, cy, r, r * 0.75, time * 0.5 + i * 0.3, 0, Math.PI * 2);
          ctx.stroke();
      }
      ctx.globalAlpha = 1;
      ctx.shadowBlur = 0;
  };

  const drawDigitalRain = (ctx: CanvasRenderingContext2D, w: number, h: number, data: Uint8Array, time: number, color: string) => {
      const cols = 50;
      const colW = w / cols;
      ctx.fillStyle = color;
      ctx.font = 'bold 14px monospace';
      ctx.shadowBlur = 8;
      ctx.shadowColor = color;
      for(let i=0; i<cols; i++) {
          const val = data[i*2];
          const normalized = val / 255;
          const len = 8 + Math.floor(Math.pow(normalized, 1.3) * 15);
          const baseSpeed = 40 + (i % 5) * 10;
          const speedOffset = (time * baseSpeed) % h;
          for(let j=0; j<len; j++) {
              const char = String.fromCharCode(0x30A0 + Math.random() * 96);
              const y = (speedOffset + (j * 18)) % h;
              ctx.globalAlpha = (1 - (j/len)) * 0.8;
              ctx.fillText(char, i * colW, y);
          }
      }
      ctx.globalAlpha = 1;
      ctx.shadowBlur = 0;
  };

  const drawShockwave = (ctx: CanvasRenderingContext2D, cx: number, cy: number, bass: number, time: number, color: string) => {
      const normBass = bass / 255;
      const maxRadius = 500;
      const rings = 6;

      ctx.shadowColor = color;

      for (let i = 0; i < rings; i++) {
          const phase = (time * 0.8 + (i * 0.4)) % 2;
          const progress = phase / 2;
          const radius = 50 + progress * maxRadius;
          const alpha = (1 - progress) * (0.5 + normBass * 0.5);
          const lineWidth = (1 - progress) * (8 + normBass * 12);

          if (alpha > 0.05) {
              ctx.beginPath();
              ctx.strokeStyle = color;
              ctx.lineWidth = lineWidth;
              ctx.globalAlpha = alpha;
              ctx.shadowBlur = 20 + normBass * 30;
              ctx.arc(cx, cy, radius, 0, Math.PI * 2);
              ctx.stroke();
          }
      }

      const coreSize = 30 + normBass * 40;
      const coreGrad = ctx.createRadialGradient(cx, cy, 0, cx, cy, coreSize);
      coreGrad.addColorStop(0, color);
      coreGrad.addColorStop(0.5, color);
      coreGrad.addColorStop(1, 'transparent');
      ctx.globalAlpha = 0.6 + normBass * 0.4;
      ctx.fillStyle = coreGrad;
      ctx.beginPath();
      ctx.arc(cx, cy, coreSize, 0, Math.PI * 2);
      ctx.fill();

      ctx.globalAlpha = 1;
      ctx.shadowBlur = 0;
  };

  const drawParticles = (ctx: CanvasRenderingContext2D, w: number, h: number, time: number, bass: number, count: number, color: string) => {
      const normBass = bass / 255;
      const cx = w / 2;
      const cy = h / 2;

      // Rising particles - float upward with drift
      const risingCount = Math.floor(count * 0.4);
      for (let i = 0; i < risingCount; i++) {
          const seed = i * 127.1;
          const xBase = ((Math.sin(seed) * 10000) % w + w) % w;
          const drift = Math.sin(time * 2 + seed) * 30;
          const x = xBase + drift;
          const speed = 20 + (i % 7) * 15;
          const y = h - ((time * speed + seed * 10) % (h + 100));
          const size = 2 + (i % 4) + normBass * 3;
          const twinkle = 0.5 + Math.sin(time * 8 + seed) * 0.3;

          ctx.beginPath();
          ctx.fillStyle = color;
          ctx.shadowBlur = 15 + normBass * 10;
          ctx.shadowColor = color;
          ctx.globalAlpha = twinkle * (0.4 + normBass * 0.4);
          ctx.arc(x, y, size, 0, Math.PI * 2);
          ctx.fill();
      }

      // Burst particles - explode from center on bass
      const burstCount = Math.floor(count * 0.35);
      for (let i = 0; i < burstCount; i++) {
          const angle = (i / burstCount) * Math.PI * 2 + time * 0.3;
          const seed = i * 234.5;
          const burstPhase = (time * 1.5 + seed * 0.01) % 3;
          const burstProgress = burstPhase / 3;
          const maxDist = 300 + normBass * 200;
          const dist = burstProgress * maxDist;
          const x = cx + Math.cos(angle) * dist;
          const y = cy + Math.sin(angle) * dist;
          const size = (1 - burstProgress) * (3 + normBass * 4);
          const alpha = (1 - burstProgress) * (0.6 + normBass * 0.4);

          if (size > 0.5 && alpha > 0.1) {
              ctx.beginPath();
              ctx.fillStyle = color;
              ctx.shadowBlur = 10;
              ctx.shadowColor = color;
              ctx.globalAlpha = alpha;
              ctx.arc(x, y, size, 0, Math.PI * 2);
              ctx.fill();
          }
      }

      // Orbital sparkles - circle around center
      const orbitalCount = Math.floor(count * 0.15);
      for (let i = 0; i < orbitalCount; i++) {
          const orbitRadius = 150 + (i % 4) * 80 + normBass * 50;
          const speed = (i % 2 === 0 ? 1 : -1) * (0.8 + (i % 3) * 0.3);
          const angle = time * speed + (i / orbitalCount) * Math.PI * 2;
          const x = cx + Math.cos(angle) * orbitRadius;
          const y = cy + Math.sin(angle) * orbitRadius;
          const sparkle = 0.5 + Math.sin(time * 12 + i * 5) * 0.5;
          const size = 2 + sparkle * 2 + normBass * 2;

          ctx.beginPath();
          ctx.fillStyle = '#fff';
          ctx.shadowBlur = 20;
          ctx.shadowColor = color;
          ctx.globalAlpha = sparkle * 0.8;
          ctx.arc(x, y, size, 0, Math.PI * 2);
          ctx.fill();
      }

      // Floating dust - subtle background particles
      const dustCount = Math.floor(count * 0.1);
      for (let i = 0; i < dustCount; i++) {
          const seed = i * 567.8;
          const x = ((Math.sin(seed) * 10000) % w + w) % w;
          const y = ((Math.cos(seed) * 10000) % h + h) % h;
          const drift = Math.sin(time + seed) * 2;
          const size = 1 + Math.sin(time * 3 + seed) * 0.5;

          ctx.beginPath();
          ctx.fillStyle = '#fff';
          ctx.shadowBlur = 5;
          ctx.shadowColor = '#fff';
          ctx.globalAlpha = 0.2 + normBass * 0.2;
          ctx.arc(x + drift, y, size, 0, Math.PI * 2);
          ctx.fill();
      }

      ctx.globalAlpha = 1;
      ctx.shadowBlur = 0;
  };

  const drawAlbumArt = (ctx: CanvasRenderingContext2D, cx: number, cy: number, pulse: number, borderColor: string, picture: HTMLImageElement | null) => {
    ctx.save();
    ctx.translate(cx, cy);
    ctx.scale(pulse, pulse);
    ctx.shadowBlur = 40;
    ctx.shadowColor = borderColor;
    ctx.beginPath();
    ctx.arc(0, 0, 150, 0, Math.PI * 2);
    ctx.closePath();
    ctx.lineWidth = 5;
    ctx.strokeStyle = 'white';
    ctx.stroke();
    ctx.clip();

    // Until the picture has arrived the circle is empty.
    if (picture) {
        ctx.drawImage(picture, -150, -150, 300, 300);
    } else {
        ctx.fillStyle = '#111';
        ctx.fillRect(-150, -150, 300, 300);
    }
    ctx.restore();
  };

  const addTextLayer = () => {
      const newLayer: TextLayer = {
          id: Date.now().toString(),
          text: t('newText'),
          x: 50,
          y: 50,
          size: 40,
          color: '#ffffff',
          font: 'Inter'
      };
      setTextLayers([...textLayers, newLayer]);
  };

  const updateTextLayer = (id: string, updates: Partial<TextLayer>) => {
      setTextLayers(textLayers.map(l => l.id === id ? { ...l, ...updates } : l));
  };

  const removeTextLayer = (id: string) => {
      setTextLayers(textLayers.filter(l => l.id !== id));
  };

  // What an agent connected over MCP reads and sets in the editor
  const editorState = () => ({
    song: song ? { id: song.id, title: song.title } : null,
    presets: PRESETS.map(preset => preset.id),
    aspect_ratios: Object.keys(RESOLUTIONS),
    config,
    effects,
    intensities,
    text_layers: textLayers,
    lyrics: {
      available: lrcLinesRef.current.length > 0,
      enabled: lyricsEnabled, style: lyricsStyle, position: lyricsPosition, font_size: lyricsFontSize, lines: lyricsLines,
      show_sections: lyricsShowSections, color: lyricsColor, background_color: lyricsBgColor, background_opacity: lyricsBgOpacity,
      highlight_color: lyricsHighlightColor, offset_seconds: lyricsOffset,
    },
    background: { type: backgroundType, image: customImage ? 'set' : null, video: videoUrl || null },
    album_art: customAlbumArt ? 'set' : 'the song cover',
    playback: { playing: isPlaying, position_seconds: playbackTime, duration_seconds: playbackDuration || audioRef.current?.duration || 0 },
    export: { running: isExporting, progress: exportProgress, stage: exportStage, saved: savedVideo, error: agentExportError },
  });
  const requireOpen = () => {
    if (!isOpen || !song) throw new Error('The video editor is closed; video_open opens it for a song.');
  };
  useBridgeCommand('video_get', () => {
    requireOpen();
    return editorState();
  });
  useBridgeCommand('video_set', (args) => {
    requireOpen();
    const object = (value: unknown) => (value && typeof value === 'object' ? value as Record<string, unknown> : null);
    // a name the editor does not have is refused before anything changes
    const known = (given: Record<string, unknown> | null, current: object, what: string) => {
      const unknown = Object.keys(given ?? {}).filter(key => !(key in current));
      if (unknown.length) throw new Error(`Unknown ${what}: ${unknown.join(', ')}. The ${what} are: ${Object.keys(current).join(', ')}.`);
    };
    const nextConfig = object(args.config);
    const nextEffects = object(args.effects);
    const nextIntensities = object(args.intensities);
    known(nextConfig, config, 'config fields');
    known(nextEffects, effects, 'effects');
    known(nextIntensities, intensities, 'intensities');
    if (nextConfig?.preset !== undefined && !PRESETS.some(preset => preset.id === nextConfig.preset)) throw new Error(`Presets: ${PRESETS.map(preset => preset.id).join(', ')}.`);
    if (nextConfig?.aspectRatio !== undefined && !(String(nextConfig.aspectRatio) in RESOLUTIONS)) throw new Error(`Aspect ratios: ${Object.keys(RESOLUTIONS).join(', ')}.`);
    if (nextConfig) setConfig(current => ({ ...current, ...nextConfig }));
    if (nextEffects) setEffects(current => ({ ...current, ...nextEffects }));
    if (nextIntensities) setIntensities(current => ({ ...current, ...nextIntensities }));
    if (Array.isArray(args.text_layers)) {
      setTextLayers((args.text_layers as Partial<TextLayer>[]).map((layer, index) => ({ id: String(layer.id ?? index + 1), text: String(layer.text ?? ''), x: Number(layer.x ?? 50), y: Number(layer.y ?? 50), size: Number(layer.size ?? 36), color: String(layer.color ?? '#ffffff'), font: String(layer.font ?? 'Inter') })));
    }
    const lyrics = object(args.lyrics);
    if (lyrics) {
      if (lyrics.enabled !== undefined) setLyricsEnabled(Boolean(lyrics.enabled));
      if (lyrics.style === 'lines' || lyrics.style === 'scroll' || lyrics.style === 'karaoke') setLyricsStyle(lyrics.style);
      if (lyrics.position === 'bottom' || lyrics.position === 'center' || lyrics.position === 'top') setLyricsPosition(lyrics.position);
      if (lyrics.font_size !== undefined) setLyricsFontSize(Number(lyrics.font_size));
      if (lyrics.lines !== undefined) setLyricsLines(Number(lyrics.lines));
      if (lyrics.show_sections !== undefined) setLyricsShowSections(Boolean(lyrics.show_sections));
      if (typeof lyrics.color === 'string') setLyricsColor(lyrics.color);
      if (typeof lyrics.background_color === 'string') setLyricsBgColor(lyrics.background_color);
      if (lyrics.background_opacity !== undefined) setLyricsBgOpacity(Number(lyrics.background_opacity));
      if (typeof lyrics.highlight_color === 'string') setLyricsHighlightColor(lyrics.highlight_color);
      if (lyrics.offset_seconds !== undefined) setLyricsOffset(Number(lyrics.offset_seconds));
    }
    const background = object(args.background);
    if (background) {
      if (typeof background.image === 'string') { setCustomImage(background.image); setBackgroundType('custom'); }
      else if (typeof background.video === 'string') { setVideoUrl(background.video); setBackgroundType('video'); }
      else if (background.type === 'random') { setBackgroundType('random'); setBackgroundSeed(Date.now()); }
    }
    if (args.album_art === null) setCustomAlbumArt(null);
    else if (typeof args.album_art === 'string') setCustomAlbumArt(args.album_art);
    return { text: 'Set; video_get shows the result, ui_screenshot shows the frame.' };
  });
  useBridgeCommand('video_render', ({ name }) => {
    requireOpen();
    if (isExporting) throw new Error('A render is already running; video_get shows its progress.');
    if (!canvasRef.current) throw new Error('The editor is still opening; call video_render again in a moment.');
    setSavedVideo(null);
    setAgentExportError(null);
    exportTargetRef.current = async (blob: Blob) => {
      const file = `${String(name || song?.title || 'clip').replace(/[\\/:*?"<>|]+/g, '_')}.mp4`;
      const response = await fetch(apiUrl(`/v1/videos?name=${encodeURIComponent(file)}`), { method: 'POST', body: blob });
      if (!response.ok) throw new Error(`The studio could not keep the video: HTTP ${response.status}`);
      const saved = await response.json() as { path: string };
      setSavedVideo(saved.path);
    };
    void startRecording();
    return { text: 'Rendering; video_get shows the progress and, when done, the saved file under export.saved.' };
  });
  useBridgeCommand('video_play', async () => {
    requireOpen();
    if (!isPlaying) await togglePlay();
    return { text: 'Playing.' };
  });
  useBridgeCommand('video_pause', async () => {
    requireOpen();
    if (isPlaying) await togglePlay();
    return { text: 'Paused.' };
  });
  useBridgeCommand('video_seek', ({ seconds }) => {
    requireOpen();
    if (audioRef.current) audioRef.current.currentTime = Number(seconds) || 0;
    setPlaybackTime(Number(seconds) || 0);
    return { text: `At ${Number(seconds) || 0} s.` };
  });

  if (!isOpen || !song) return null;

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/90 backdrop-blur-md p-0 md:p-4 animate-in fade-in duration-200">

      <div className={`bg-suno-card w-full h-full md:max-w-7xl md:h-[90vh] md:rounded-2xl border-0 md:border border-white/10 overflow-hidden shadow-2xl relative ${isMobile ? 'flex flex-col' : 'flex'}`}>

        {/* Close Button */}
        <button onClick={onClose} className="absolute top-3 right-3 md:top-4 md:right-4 z-50 p-2 bg-black/50 hover:bg-white/20 rounded-full text-white transition-colors">
            <X size={isMobile ? 20 : 24} />
        </button>

        {/* Mobile: Preview at top */}
        {isMobile && (
          <div className="relative bg-black flex-shrink-0">
            {/* Header */}
            <div className="absolute top-0 left-0 right-0 z-10 p-3 bg-gradient-to-b from-black/80 to-transparent">
              <h2 className="text-base font-bold text-white flex items-center gap-2">
                <Video className="text-pink-500" size={18} />
                {t('videoStudio')}
              </h2>
            </div>

            {/* WYSIWYG Hint — above video */}
            <div className="flex items-center justify-center gap-3 py-1 text-[10px] text-zinc-500 bg-black/30">
              <span>🖱 {t('dragToMove')}</span>
              <span>⚙ {t('scrollToResize')}</span>
            </div>

            {/* Canvas Preview */}
            <div className={`relative w-full ${config.aspectRatio === '1:1' ? 'aspect-square' : config.aspectRatio === '9:16' ? 'aspect-[9/16] max-h-[60vh]' : 'aspect-video'}`}>
              <canvas
                ref={canvasRef}
                width={RESOLUTIONS[config.aspectRatio].width}
                height={RESOLUTIONS[config.aspectRatio].height}
                className="w-full h-full object-contain bg-[#0a0a0a] cursor-crosshair"
                onMouseDown={handleCanvasMouseDown}
                onMouseMove={handleCanvasMouseMove}
                onMouseUp={handleCanvasMouseUp}
                onMouseLeave={handleCanvasMouseUp}
                onWheel={handleCanvasWheel}
              />
            </div>

            {/* Playback Controls */}
            <div className="absolute bottom-0 left-0 right-0 bg-gradient-to-t from-black/90 to-transparent p-3">
              {/* Timeline */}
              <div className="flex items-center gap-2 mb-2 px-2">
                <span className="text-[10px] text-zinc-400 font-mono w-10 text-right">
                  {`${Math.floor(playbackTime / 60)}:${String(Math.floor(playbackTime % 60)).padStart(2, '0')}`}
                </span>
                <input
                  type="range"
                  min={0}
                  max={playbackDuration || 100}
                  value={playbackTime}
                  onChange={(e) => { const v = Number(e.target.value); setPlaybackTime(v); if (audioRef.current) audioRef.current.currentTime = v; }}
                  className="flex-1 h-1 accent-pink-500 cursor-pointer"
                  step={0.1}
                />
                <span className="text-[10px] text-zinc-400 font-mono w-10">
                  {`${Math.floor(playbackDuration / 60)}:${String(Math.floor(playbackDuration % 60)).padStart(2, '0')}`}
                </span>
              </div>
              {/* Play + Volume */}
              <div className="flex items-center justify-center gap-4">
                <button
                  onClick={togglePlay}
                  disabled={isExporting}
                  className="w-10 h-10 rounded-full bg-white text-black flex items-center justify-center shadow-lg tap-highlight-none disabled:opacity-50"
                >
                  {isPlaying ? <Pause fill="black" size={18} /> : <Play fill="black" className="ml-0.5" size={18} />}
                </button>
                <div className="flex items-center gap-1.5">
                  <Music size={12} className="text-zinc-400" />
                  <input
                    type="range"
                    min={0}
                    max={1}
                    step={0.01}
                    value={volume}
                    onChange={(e) => { const v = Number(e.target.value); setVolume(v); if (audioRef.current) audioRef.current.volume = v; }}
                    className="w-20 h-1 accent-pink-500 cursor-pointer"
                  />
                </div>
              </div>
            </div>
          </div>
        )}

        {/* Sidebar Controls */}
        <div className={`${isMobile ? 'flex-1 overflow-hidden' : 'w-96'} bg-suno-panel ${isMobile ? '' : 'border-r border-white/5'} flex flex-col z-20`}>
            {/* Header - Desktop only */}
            {!isMobile && (
              <div className="p-6 border-b border-white/5">
                  <h2 className="text-xl font-bold text-white mb-1 flex items-center gap-2">
                      <Video className="text-pink-500" size={20} />
                      Video Studio
                  </h2>
                  <p className="text-zinc-500 text-xs">{t('createProfessionalVisualizers')}</p>
              </div>
            )}

            {/* Tabs */}
            <div className="flex border-b border-white/5">
                {[
                    { id: 'presets', label: t('tabPresets'), icon: <Grid size={14} /> },
                    { id: 'style', label: t('tabStyle'), icon: <Palette size={14} /> },
                    { id: 'text', label: t('tabText'), icon: <Type size={14} /> },
                    { id: 'effects', label: t('tabFx'), icon: <Zap size={14} /> }
                ].map(tab => (
                    <button 
                        key={tab.id}
                        onClick={() => setActiveTab(tab.id)}
                        className={`flex-1 flex items-center justify-center gap-2 py-3 text-xs font-bold uppercase tracking-wider transition-colors ${activeTab === tab.id ? 'text-white border-b-2 border-pink-500 bg-white/5' : 'text-zinc-500 hover:text-zinc-300'}`}
                    >
                        {tab.icon} {tab.label}
                    </button>
                ))}
            </div>

            {/* Content Area */}
            <div className="flex-1 overflow-y-auto custom-scrollbar p-4 md:p-6 space-y-4 md:space-y-6">
                
                {/* PRESETS TAB */}
                {activeTab === 'presets' && (
                    <div className="grid grid-cols-2 gap-3">
                        {PRESETS.map(preset => (
                            <button
                                key={preset.id}
                                onClick={() => setConfig({ ...config, preset: preset.id })}
                                className={`flex flex-col items-center gap-2 p-4 rounded-xl border transition-all ${config.preset === preset.id ? 'bg-pink-600/20 border-pink-500 text-white' : 'bg-black/20 border-white/5 text-zinc-400 hover:bg-white/5 hover:border-white/10'}`}
                            >
                                <div className={`p-2 rounded-full ${config.preset === preset.id ? 'bg-pink-500 text-white' : 'bg-black/40 text-zinc-500'}`}>
                                    {preset.icon}
                                </div>
                                <span className="text-xs font-medium">{t(preset.labelKey)}</span>
                            </button>
                        ))}
                    </div>
                )}

                {/* STYLE TAB */}
                {activeTab === 'style' && (
                    <div className="space-y-6">
                         {/* Resolution / Aspect Ratio */}
                         <div className="space-y-3">
                            <label className="text-xs font-bold text-zinc-500 uppercase">{t('aspectRatio') || 'Aspect Ratio'}</label>
                            <div className="grid grid-cols-3 gap-2">
                                {(Object.keys(RESOLUTIONS) as AspectRatio[]).map(ratio => (
                                    <button
                                        key={ratio}
                                        onClick={() => setConfig({...config, aspectRatio: ratio})}
                                        className={`px-3 py-2 rounded-lg text-xs font-medium border transition-colors ${config.aspectRatio === ratio ? 'bg-pink-600 border-pink-600 text-white' : 'border-white/10 text-zinc-400 hover:border-white/20'}`}
                                    >
                                        <div className="font-bold">{ratio}</div>
                                        <div className="text-[10px] opacity-60">{RESOLUTIONS[ratio].label}</div>
                                    </button>
                                ))}
                            </div>
                         </div>

                         {/* Background */}
                         <div className="space-y-3">
                            <label className="text-xs font-bold text-zinc-500 uppercase flex justify-between">
                                {t('videoBackground')}
                            </label>
                            <div className="bg-black/20 p-3 rounded-lg border border-white/5 space-y-3">
                                {/* Type Selection */}
                                <div className="grid grid-cols-3 gap-2">
                                     <button
                                        onClick={() => { setBackgroundType('random'); setBackgroundSeed(Date.now()); }}
                                        className={`py-2 rounded text-xs font-bold flex items-center justify-center gap-1 ${backgroundType === 'random' ? 'bg-pink-600 text-white' : 'bg-zinc-800 text-zinc-400'}`}
                                     >
                                         <Wand2 size={12}/> {t('bgRandom')}
                                     </button>
                                     <button
                                        onClick={() => setBackgroundType('custom')}
                                        className={`py-2 rounded text-xs font-bold flex items-center justify-center gap-1 ${backgroundType === 'custom' ? 'bg-pink-600 text-white' : 'bg-zinc-800 text-zinc-400'}`}
                                     >
                                         <ImageIcon size={12}/> {t('bgImage')}
                                     </button>
                                     <button
                                        onClick={() => setBackgroundType('video')}
                                        className={`py-2 rounded text-xs font-bold flex items-center justify-center gap-1 ${backgroundType === 'video' ? 'bg-pink-600 text-white' : 'bg-zinc-800 text-zinc-400'}`}
                                     >
                                         <Video size={12}/> {t('bgVideo')}
                                     </button>
                                </div>

                                {/* Image Options */}
                                {backgroundType === 'custom' && (
                                    <div className="space-y-2">
                                        <div className="grid grid-cols-2 gap-2">
                                            <button
                                                onClick={() => fileInputRef.current?.click()}
                                                className="py-2 px-3 bg-zinc-700 hover:bg-zinc-600 rounded text-xs text-white flex items-center justify-center gap-1"
                                            >
                                                <Upload size={12}/> {t('upload')}
                                            </button>
                                            <button
                                                onClick={() => openPexelsBrowser('background', 'photos')}
                                                className="py-2 px-3 bg-emerald-600 hover:bg-emerald-700 rounded text-xs text-white flex items-center justify-center gap-1"
                                            >
                                                <Search size={12}/> Pexels
                                            </button>
                                        </div>
                                        {customImage && (
                                            <div className="relative rounded overflow-hidden h-20">
                                                <img src={customImage} alt="Background" className="w-full h-full object-cover" />
                                            </div>
                                        )}
                                    </div>
                                )}

                                {/* Video Options */}
                                {backgroundType === 'video' && (
                                    <div className="space-y-2">
                                        <div className="grid grid-cols-2 gap-2">
                                            <button
                                                onClick={() => videoFileInputRef.current?.click()}
                                                className="py-2 px-3 bg-zinc-700 hover:bg-zinc-600 rounded text-xs text-white flex items-center justify-center gap-1"
                                            >
                                                <Upload size={12}/> {t('upload')}
                                            </button>
                                            <button
                                                onClick={() => openPexelsBrowser('background', 'videos')}
                                                className="py-2 px-3 bg-emerald-600 hover:bg-emerald-700 rounded text-xs text-white flex items-center justify-center gap-1"
                                            >
                                                <Search size={12}/> Pexels
                                            </button>
                                        </div>
                                        <input
                                            type="text"
                                            placeholder={t('orPasteVideoUrl')}
                                            value={videoUrl}
                                            onChange={(e) => setVideoUrl(e.target.value)}
                                            className="w-full bg-zinc-800 rounded px-3 py-2 text-xs text-white border border-white/10 placeholder-zinc-500"
                                        />
                                        {videoUrl && bgVideoState === 'loading' && (
                                            <p className="text-[10px] text-zinc-400">{t('videoLoading')}</p>
                                        )}
                                        {videoUrl && bgVideoState === 'ready' && (
                                            <p className="text-[10px] text-emerald-400 truncate">✓ {t('videoReady')}</p>
                                        )}
                                        {videoUrl && bgVideoState === 'failed' && (
                                            <p className="text-[10px] text-rose-400">{t('videoFailed')}</p>
                                        )}
                                    </div>
                                )}

                                {/* Hidden File Inputs */}
                                <input
                                    type="file"
                                    ref={fileInputRef}
                                    onChange={handleFileUpload}
                                    className="hidden"
                                    accept="image/*"
                                />
                                <input
                                    type="file"
                                    ref={videoFileInputRef}
                                    onChange={handleVideoFileUpload}
                                    className="hidden"
                                    accept="video/*"
                                />

                                <div>
                                    <div className="flex justify-between text-sm text-zinc-300 mb-2">
                                        <span>{t('dimming')}</span>
                                        <span>{Math.round(config.bgDim * 100)}%</span>
                                    </div>
                                    <input
                                        type="range" min="0" max="1" step="0.1"
                                        value={config.bgDim}
                                        onChange={(e) => setConfig({...config, bgDim: parseFloat(e.target.value)})}
                                        className="w-full accent-pink-500 h-1 bg-zinc-700 rounded-lg appearance-none cursor-pointer"
                                    />
                                </div>
                            </div>
                        </div>

                         {/* Colors */}
                         <div className="space-y-3">
                             <label className="text-xs font-bold text-zinc-500 uppercase">{t('colorPresets')}</label>
                             <div className="grid grid-cols-5 gap-2">
                                 {[
                                     { name: 'Neon Pink', primary: '#ec4899', secondary: '#8b5cf6' },
                                     { name: 'Cyber Blue', primary: '#06b6d4', secondary: '#3b82f6' },
                                     { name: 'Sunset', primary: '#f97316', secondary: '#eab308' },
                                     { name: 'Matrix', primary: '#22c55e', secondary: '#10b981' },
                                     { name: 'Fire', primary: '#ef4444', secondary: '#f97316' },
                                     { name: 'Ocean', primary: '#0ea5e9', secondary: '#06b6d4' },
                                     { name: 'Violet', primary: '#a855f7', secondary: '#ec4899' },
                                     { name: 'Gold', primary: '#eab308', secondary: '#f59e0b' },
                                     { name: 'Ice', primary: '#67e8f9', secondary: '#a5f3fc' },
                                     { name: 'Mono', primary: '#ffffff', secondary: '#a1a1aa' },
                                 ].map((preset) => (
                                     <button
                                         key={preset.name}
                                         onClick={() => setConfig({...config, primaryColor: preset.primary, secondaryColor: preset.secondary})}
                                         className={`group relative h-8 rounded-lg overflow-hidden border-2 transition-all ${
                                             config.primaryColor === preset.primary && config.secondaryColor === preset.secondary
                                                 ? 'border-white scale-110 shadow-lg'
                                                 : 'border-transparent hover:border-white/30 hover:scale-105'
                                         }`}
                                         title={preset.name}
                                     >
                                         <div className="absolute inset-0 flex">
                                             <div className="flex-1" style={{ backgroundColor: preset.primary }} />
                                             <div className="flex-1" style={{ backgroundColor: preset.secondary }} />
                                         </div>
                                     </button>
                                 ))}
                             </div>
                         </div>

                         <div className="space-y-3">
                             <label className="text-xs font-bold text-zinc-500 uppercase">{t('customColors')}</label>
                             <div className="grid grid-cols-2 gap-4">
                                 <div>
                                     <span className="text-[10px] text-zinc-400 mb-1 block">{t('primary')}</span>
                                     <div className="flex items-center gap-2 bg-black/20 p-2 rounded border border-white/5">
                                         <input type="color" value={config.primaryColor} onChange={(e) => setConfig({...config, primaryColor: e.target.value})} className="w-6 h-6 rounded cursor-pointer border-none bg-transparent" />
                                         <span className="text-xs text-zinc-300 font-mono">{config.primaryColor}</span>
                                     </div>
                                 </div>
                                 <div>
                                     <span className="text-[10px] text-zinc-400 mb-1 block">{t('secondary')}</span>
                                      <div className="flex items-center gap-2 bg-black/20 p-2 rounded border border-white/5">
                                         <input type="color" value={config.secondaryColor} onChange={(e) => setConfig({...config, secondaryColor: e.target.value})} className="w-6 h-6 rounded cursor-pointer border-none bg-transparent" />
                                         <span className="text-xs text-zinc-300 font-mono">{config.secondaryColor}</span>
                                     </div>
                                 </div>
                             </div>
                         </div>
                         
                         {/* Visualizer Scale */}
                         <div className="space-y-3">
                            <div className="flex justify-between text-xs font-bold text-zinc-500 uppercase">
                                <span>{t('size')}</span>
                                <span>{Math.round(config.visualizerScale * 100)}%</span>
                            </div>
                            <input
                                type="range" min="30" max="200" step="5"
                                value={Math.round(config.visualizerScale * 100)}
                                onChange={(e) => setConfig({...config, visualizerScale: parseInt(e.target.value) / 100})}
                                className="w-full accent-pink-500 h-1 bg-zinc-700 rounded-lg appearance-none cursor-pointer"
                            />
                         </div>

                         {/* Particles */}
                         <div className="space-y-3">
                            <div className="flex justify-between text-xs font-bold text-zinc-500 uppercase">
                                <span>{t('particles')}</span>
                                <span>{config.particleCount}</span>
                            </div>
                            <input
                                type="range" min="0" max="200" step="10"
                                value={config.particleCount}
                                onChange={(e) => setConfig({...config, particleCount: parseInt(e.target.value)})}
                                className="w-full accent-pink-500 h-1 bg-zinc-700 rounded-lg appearance-none cursor-pointer"
                            />
                        </div>

                        {/* {t('centerImage')} (Album Art) */}
                        <div className="space-y-3">
                            <label className="text-xs font-bold text-zinc-500 uppercase">{t('centerImage')}</label>
                            <div className="bg-black/20 p-3 rounded-lg border border-white/5 space-y-3">
                                <div className="flex items-center gap-3">
                                    {/* Preview */}
                                    <div className="w-16 h-16 rounded-lg overflow-hidden bg-zinc-800 flex-shrink-0">
                                        <img
                                            src={customAlbumArt || song?.coverUrl || ''}
                                            alt="Center"
                                            className="w-full h-full object-cover"
                                        />
                                    </div>
                                    <div className="flex-1 space-y-2">
                                        <div className="grid grid-cols-2 gap-2">
                                            <button
                                                onClick={() => albumArtInputRef.current?.click()}
                                                className="py-1.5 px-2 bg-zinc-700 hover:bg-zinc-600 rounded text-[10px] text-white flex items-center justify-center gap-1"
                                            >
                                                <Upload size={10}/> {t('upload')}
                                            </button>
                                            <button
                                                onClick={() => openPexelsBrowser('albumArt')}
                                                className="py-1.5 px-2 bg-emerald-600 hover:bg-emerald-700 rounded text-[10px] text-white flex items-center justify-center gap-1"
                                            >
                                                <Search size={10}/> Pexels
                                            </button>
                                        </div>
                                        {customAlbumArt && (
                                            <button
                                                onClick={() => setCustomAlbumArt(null)}
                                                className="w-full py-1 text-[10px] text-zinc-500 hover:text-red-400"
                                            >
                                                {t('resetToDefault')}
                                            </button>
                                        )}
                                    </div>
                                </div>
                                <input
                                    type="file"
                                    ref={albumArtInputRef}
                                    onChange={handleAlbumArtUpload}
                                    className="hidden"
                                    accept="image/*"
                                />
                            </div>
                        </div>
                    </div>
                )}

                {/* TEXT TAB */}
                {activeTab === 'text' && (
                    <div className="space-y-4">
                        {/* Lyrics Overlay Settings */}
                        {lrcLinesRef.current.length > 0 && (
                            <div className="bg-black/20 p-3 rounded-lg border border-white/5 space-y-3">
                                <div className="flex items-center justify-between">
                                    <label className="text-xs font-bold text-zinc-500 uppercase">{t('lyricsOverlay') || 'Lyrics Overlay'}</label>
                                    <button
                                        onClick={() => setLyricsEnabled(!lyricsEnabled)}
                                        className={`w-10 h-5 rounded-full transition-colors ${lyricsEnabled ? 'bg-pink-500' : 'bg-zinc-600'}`}
                                    >
                                        <div className={`w-4 h-4 rounded-full bg-white shadow-sm transition-transform ${lyricsEnabled ? 'translate-x-5' : 'translate-x-0.5'}`} />
                                    </button>
                                </div>

                                {lyricsEnabled && (
                                    <div className="space-y-3">
                                        {/* Position */}
                                        <div>
                                        {/* Style */}
                                        <div>
                                            <label className="text-[10px] text-zinc-500 block mb-1">{t('lyricsStyleLabel') || 'Style'}</label>
                                            <div className="grid grid-cols-3 gap-1">
                                                {([
                                                    { id: 'lines' as const, label: t('lyricsStyleLines') || 'Lines' },
                                                    { id: 'scroll' as const, label: t('lyricsStyleScroll') || 'Scroll' },
                                                    { id: 'karaoke' as const, label: t('lyricsStyleKaraoke') || 'Karaoke' },
                                                ]).map(s => (
                                                    <button
                                                        key={s.id}
                                                        onClick={() => setLyricsStyle(s.id)}
                                                        className={`px-2 py-1 rounded text-[10px] font-medium ${lyricsStyle === s.id ? 'bg-pink-600 text-white' : 'bg-white/5 text-zinc-400'}`}
                                                    >
                                                        {s.label}
                                                    </button>
                                                ))}
                                            </div>
                                        </div>

                                        </div>

                                        {/* Lines visible — only for 'lines' style */}
                                        {lyricsStyle === 'lines' && <div>
                                            <label className="text-[10px] text-zinc-500 block mb-1">{t('visibleLines') || 'Visible Lines'}</label>
                                            <div className="grid grid-cols-3 gap-1">
                                                {[1, 2, 3].map(n => (
                                                    <button
                                                        key={n}
                                                        onClick={() => setLyricsLines(n)}
                                                        className={`px-2 py-1 rounded text-[10px] font-medium ${lyricsLines === n ? 'bg-pink-600 text-white' : 'bg-white/5 text-zinc-400'}`}
                                                    >
                                                        {n}
                                                    </button>
                                                ))}
                                            </div>
                                        </div>}

                                        {/* Font size */}
                                        <div>
                                            <div className="flex justify-between">
                                                <label className="text-[10px] text-zinc-500">{t('fontSize') || 'Font Size'}</label>
                                                <span className="text-[10px] text-zinc-400">{lyricsFontSize}px</span>
                                            </div>
                                            <input type="range" min={24} max={72} value={lyricsFontSize} onChange={e => setLyricsFontSize(Number(e.target.value))} className="w-full accent-pink-500 h-1 bg-zinc-700 rounded-lg appearance-none cursor-pointer" />
                                        </div>

                                        {/* Colors — all in one row */}
                                        <div className="flex items-center gap-2 flex-wrap">
                                            <span className="text-[10px] text-zinc-500">{t('color')}</span>
                                            <input type="color" value={lyricsColor} onChange={e => setLyricsColor(e.target.value)} className="w-5 h-5 rounded cursor-pointer border-none bg-transparent" />
                                            <input type="color" value={lyricsHighlightColor} onChange={e => setLyricsHighlightColor(e.target.value)} className="w-5 h-5 rounded cursor-pointer border-none bg-transparent" />
                                            <input type="color" value={lyricsBgColor} onChange={e => setLyricsBgColor(e.target.value)} className="w-5 h-5 rounded cursor-pointer border-none bg-transparent" />
                                            <span className="text-[10px] text-zinc-500">{t('opacity') || 'Opacity'}</span>
                                            <input type="range" min={0} max={100} value={lyricsBgOpacity} onChange={e => setLyricsBgOpacity(Number(e.target.value))} className="flex-1 accent-pink-500 h-1 bg-zinc-700 rounded-lg appearance-none cursor-pointer min-w-[60px]" />
                                            <span className="text-[10px] text-zinc-400">{lyricsBgOpacity}%</span>
                                        </div>

                                        {/* Show sections */}
                                        {/* Timing offset */}
                                        <div className="flex items-center gap-2">
                                            <span className="text-[10px] text-zinc-500">{t('lyricsOffset') || 'Offset'}</span>
                                            <input type="range" min={-3} max={3} step={0.1} value={lyricsOffset} onChange={e => setLyricsOffset(Number(e.target.value))} className="flex-1 accent-pink-500 h-1 bg-zinc-700 rounded-lg appearance-none cursor-pointer" />
                                            <span className="text-[10px] text-zinc-400 w-10 text-right">{lyricsOffset > 0 ? '+' : ''}{lyricsOffset.toFixed(1)}s</span>
                                        </div>

                                        <label className="flex items-center gap-2 text-[10px] text-zinc-400 cursor-pointer">
                                            <input type="checkbox" checked={lyricsShowSections} onChange={() => setLyricsShowSections(!lyricsShowSections)} className="accent-pink-500" />
                                            {t('showSectionMarkers') || 'Show section markers ([Verse], [Chorus])'}
                                        </label>
                                    </div>
                                )}
                            </div>
                        )}

                        {/* Text Layers */}
                        <button
                            onClick={addTextLayer}
                            className="w-full py-2 bg-pink-600 text-white rounded-lg flex items-center justify-center gap-2 text-xs font-bold hover:bg-pink-700"
                        >
                            <Plus size={14} /> {t('addTextLayer')}
                        </button>
                        
                        <div className="space-y-3">
                            {textLayers.map((layer, index) => (
                                <div key={layer.id} className="bg-black/20 p-3 rounded-lg border border-white/5 space-y-3">
                                    <div className="flex items-center justify-between">
                                        <span className="text-xs font-bold text-zinc-500">{t('layer')} {index + 1}</span>
                                        <button onClick={() => removeTextLayer(layer.id)} className="text-zinc-500 hover:text-red-500">
                                            <Trash2 size={14} />
                                        </button>
                                    </div>
                                    <input 
                                        type="text" 
                                        value={layer.text} 
                                        onChange={(e) => updateTextLayer(layer.id, { text: e.target.value })}
                                        className="w-full bg-zinc-800 rounded px-2 py-1 text-xs text-white border border-white/5"
                                        placeholder={t('textContent')}
                                    />
                                    <div className="flex items-center gap-2">
                                        <span className="text-[10px] text-zinc-500">{t('size')}</span>
                                        <input type="range" min="12" max="120" value={layer.size} onChange={(e) => updateTextLayer(layer.id, { size: parseInt(e.target.value) })} className="flex-1 accent-pink-500 h-1 bg-zinc-700 rounded-lg appearance-none cursor-pointer" />
                                        <span className="text-[10px] text-zinc-400 w-8 text-right">{layer.size}px</span>
                                        <input type="color" value={layer.color} onChange={(e) => updateTextLayer(layer.id, { color: e.target.value })} className="w-5 h-5 rounded cursor-pointer border-none bg-transparent" />
                                    </div>
                                </div>
                            ))}
                        </div>
                    </div>
                )}

                {/* EFFECTS TAB */}
                {activeTab === 'effects' && (
                    <div className="space-y-2">
                        {[
                            { id: 'shake', label: t('fx_shake'), desc: t('fxDesc_shake'), icon: <Activity size={16}/> },
                            { id: 'glitch', label: t('fx_glitch'), desc: t('fxDesc_glitch'), icon: <Zap size={16}/> },
                            { id: 'vhs', label: t('fx_vhs'), desc: t('fxDesc_vhs'), icon: <Disc size={16}/> },
                            { id: 'cctv', label: t('fx_cctv'), desc: t('fxDesc_cctv'), icon: <Monitor size={16}/> },
                            { id: 'scanlines', label: t('fx_scanlines'), desc: t('fxDesc_scanlines'), icon: <Grid size={16}/> },
                            { id: 'chromatic', label: t('fx_chromatic'), desc: t('fxDesc_chromatic'), icon: <Layers size={16}/> },
                            { id: 'bloom', label: t('fx_bloom'), desc: t('fxDesc_bloom'), icon: <Sun size={16}/> },
                            { id: 'filmGrain', label: t('fx_filmGrain'), desc: t('fxDesc_filmGrain'), icon: <Film size={16}/> },
                            { id: 'pixelate', label: t('fx_pixelate'), desc: t('fxDesc_pixelate'), icon: <Grid size={16}/> },
                            { id: 'strobe', label: t('fx_strobe'), desc: t('fxDesc_strobe'), icon: <Zap size={16}/> },
                            { id: 'vignette', label: t('fx_vignette'), desc: t('fxDesc_vignette'), icon: <Circle size={16}/> },
                            { id: 'hueShift', label: t('fx_hueShift'), desc: t('fxDesc_hueShift'), icon: <Palette size={16}/> },
                            { id: 'letterbox', label: t('fx_letterbox'), desc: t('fxDesc_letterbox'), icon: <Minus size={16}/> },
                        ].map((effect) => {
                             const effectId = effect.id as keyof EffectConfig;
                             const isActive = effects[effectId];
                             const intensity = intensities[effectId as keyof EffectIntensities];

                             return (
                                <div key={effect.id} className={`rounded-lg border transition-all ${isActive ? 'bg-pink-600/10 border-pink-500/30' : 'bg-black/20 border-white/5'}`}>
                                     <button 
                                        onClick={() => setEffects(prev => ({ ...prev, [effectId]: !prev[effectId] }))}
                                        className="w-full flex items-center justify-between p-3"
                                    >
                                        <div className="flex items-center gap-3">
                                            <div className={`p-1.5 rounded-md ${isActive ? 'bg-pink-500 text-white' : 'bg-zinc-800 text-zinc-500'}`}>
                                                {effect.icon}
                                            </div>
                                            <div className="text-left">
                                                <div className={`text-sm font-bold ${isActive ? 'text-white' : 'text-zinc-400'}`}>{effect.label}</div>
                                                <div className="text-[10px] text-zinc-500">{effect.desc}</div>
                                            </div>
                                        </div>
                                        <div className={`w-3 h-3 rounded-full ${isActive ? 'bg-pink-500 shadow-[0_0_8px_rgba(236,72,153,0.8)]' : 'bg-zinc-700'}`}></div>
                                    </button>
                                    
                                    {/* Intensity Slider */}
                                    {isActive && (
                                        <div className="px-3 pb-3 pt-0 animate-in fade-in slide-in-from-top-2">
                                            <div className="flex justify-between text-[10px] text-zinc-400 mb-1">
                                                <span>{t('intensity')}</span>
                                                <span>{Math.round(intensity * 100)}%</span>
                                            </div>
                                            <input 
                                                type="range" min="0" max="1" step="0.05" 
                                                value={intensity}
                                                onChange={(e) => setIntensities({...intensities, [effectId]: parseFloat(e.target.value)})}
                                                className="w-full accent-pink-500 h-1 bg-zinc-700 rounded-lg appearance-none cursor-pointer"
                                            />
                                        </div>
                                    )}
                                </div>
                             );
                        })}
                    </div>
                )}

            </div>

            {/* Footer */}
            <div className="p-4 md:p-6 border-t border-white/5 bg-black/20 space-y-3 safe-area-inset-bottom">
                 {ffmpegLoading ? (
                     <div className="w-full bg-zinc-800 rounded-xl h-12 flex items-center justify-center px-4">
                         <div className="flex items-center gap-2 text-white font-bold text-sm">
                             <Loader2 className="animate-spin" size={18} />
                             Loading video encoder...
                         </div>
                     </div>
                 ) : isExporting ? (
                     <div className="w-full bg-zinc-800 rounded-xl h-12 flex items-center justify-center px-4 relative overflow-hidden">
                         <div
                           className={`absolute left-0 top-0 bottom-0 transition-all duration-100 ${exportStage === 'capturing' ? 'bg-pink-600/20' : 'bg-blue-600/20'}`}
                           style={{ width: `${exportProgress}%` }}
                         />
                         <div className="flex items-center gap-2 z-10 text-white font-bold text-sm">
                             {exportStage === 'capturing' ? (
                               <>
                                 <Loader2 className="animate-spin text-pink-400" size={16} />
                                 Rendering frames {Math.round(exportProgress)}%
                               </>
                             ) : (
                               <>
                                 <Loader2 className="animate-spin text-blue-400" size={16} />
                                 {exportProgress < 95 ? 'Encoding (be patient)...' : `Encoding MP4 ${Math.round(exportProgress)}%`}
                               </>
                             )}
                         </div>
                     </div>
                 ) : (
                    <button
                        onClick={startRecording}
                        disabled={ffmpegLoading}
                        className="w-full h-12 bg-white text-black font-bold rounded-xl flex items-center justify-center gap-2 hover:scale-105 transition-transform disabled:opacity-50"
                    >
                        <Download size={18} />
                        {t('renderVideo')}
                    </button>
                 )}
                 <p className="text-[10px] text-zinc-600 text-center">
                   {ffmpegLoaded ? `${t('encoderReady')} • ` : ''}{t('offlineRendering')}
                 </p>
            </div>
        </div>

        {/* Preview Area - Desktop only */}
        {!isMobile && (
          <div className="flex-1 bg-[#0a0a0a] flex flex-col">
               <canvas
                  ref={canvasRef}
                  width={RESOLUTIONS[config.aspectRatio].width}
                  height={RESOLUTIONS[config.aspectRatio].height}
                  className="w-full flex-1 min-h-0 object-contain cursor-crosshair"
                  onMouseDown={handleCanvasMouseDown}
                  onMouseMove={handleCanvasMouseMove}
                  onMouseUp={handleCanvasMouseUp}
                  onMouseLeave={handleCanvasMouseUp}
                  onWheel={handleCanvasWheel}
               />

               {/* Hints */}
               <div className="text-center py-1 text-[10px] text-zinc-500 flex-shrink-0">
                 🖱 {t('dragToMove')}  ⚙ {t('scrollToResize')}
               </div>

               {/* Playback Controls */}
               <div className="flex-shrink-0 p-3">
                 {/* Timeline */}
                 <div className="flex items-center gap-3 mb-3 px-2">
                   <span className="text-[11px] text-zinc-400 font-mono w-12 text-right">
                     {`${Math.floor(playbackTime / 60)}:${String(Math.floor(playbackTime % 60)).padStart(2, '0')}`}
                   </span>
                   <input
                     type="range"
                     min={0}
                     max={playbackDuration || 100}
                     value={playbackTime}
                     onChange={(e) => { const v = Number(e.target.value); setPlaybackTime(v); if (audioRef.current) audioRef.current.currentTime = v; }}
                     className="flex-1 h-1.5 accent-pink-500 cursor-pointer"
                     step={0.1}
                   />
                   <span className="text-[11px] text-zinc-400 font-mono w-12">
                     {`${Math.floor(playbackDuration / 60)}:${String(Math.floor(playbackDuration % 60)).padStart(2, '0')}`}
                   </span>
                 </div>
                 {/* Play + Volume */}
                 <div className="flex items-center justify-center gap-6">
                   <button
                     onClick={togglePlay}
                     disabled={isExporting}
                     className="w-12 h-12 rounded-full bg-white text-black flex items-center justify-center hover:scale-105 transition-transform shadow-lg disabled:opacity-50"
                   >
                     {isPlaying ? <Pause fill="black" size={20} /> : <Play fill="black" className="ml-0.5" size={20} />}
                   </button>
                   <div className="flex items-center gap-2">
                     <Music size={14} className="text-zinc-400" />
                     <input
                       type="range"
                       min={0}
                       max={1}
                       step={0.01}
                       value={volume}
                       onChange={(e) => { const v = Number(e.target.value); setVolume(v); if (audioRef.current) audioRef.current.volume = v; }}
                       className="w-24 h-1.5 accent-pink-500 cursor-pointer"
                     />
                   </div>
                 </div>
               </div>
          </div>
        )}

      </div>

      {/* Pexels Browser Modal */}
      {showPexelsBrowser && (
        <div className="fixed inset-0 z-[70] flex items-center justify-center bg-black/80 backdrop-blur-sm p-4">
          <div className="bg-zinc-900 w-full max-w-4xl max-h-[80vh] rounded-2xl border border-white/10 flex flex-col overflow-hidden">
            {/* Header */}
            <div className="p-4 border-b border-white/10 flex items-center justify-between">
              <div className="flex items-center gap-3">
                <div className="p-2 bg-emerald-600 rounded-lg">
                  <ExternalLink size={18} className="text-white" />
                </div>
                <div>
                  <h3 className="text-white font-bold">
                    {pexelsTarget === 'albumArt' ? t('selectCenterImage') : t('selectBackground')}
                  </h3>
                  <p className="text-zinc-500 text-xs">
                    {pexelsTarget === 'albumArt' ? 'Choose an image for the center circle' : 'Free stock photos & videos'}
                  </p>
                </div>
              </div>
              <div className="flex items-center gap-2">
                <button
                  onClick={() => setShowPexelsApiKeyInput(!showPexelsApiKeyInput)}
                  className={`p-2 hover:bg-white/10 rounded-lg ${pexelsApiKey ? 'text-emerald-400' : 'text-amber-400'}`}
                  title={pexelsApiKey ? 'API key configured' : 'Set API key'}
                >
                  <Settings2 size={20} />
                </button>
                <button onClick={() => setShowPexelsBrowser(false)} className="p-2 hover:bg-white/10 rounded-lg text-zinc-400">
                  <X size={20} />
                </button>
              </div>
            </div>

            {/* API Key Input */}
            {showPexelsApiKeyInput && (
              <div className="p-4 bg-zinc-800/50 border-b border-white/10 space-y-2">
                <div className="flex items-center justify-between">
                  <label className="text-sm font-medium text-zinc-300">{t('pexelsApiKey')}</label>
                  <a
                    href="https://www.pexels.com/api/new/"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-xs text-emerald-400 hover:underline flex items-center gap-1"
                  >
                    Get free API key <ExternalLink size={10} />
                  </a>
                </div>
                <div className="flex gap-2">
                  <input
                    type="password"
                    value={pexelsApiKey}
                    onChange={(e) => setPexelsApiKey(e.target.value)}
                    placeholder={t('enterPexelsApiKey')}
                    className="flex-1 bg-zinc-900 rounded-lg px-4 py-2 text-sm text-white border border-white/10 placeholder-zinc-500"
                  />
                  <button
                    onClick={() => savePexelsApiKey(pexelsApiKey)}
                    className="px-4 py-2 bg-emerald-600 hover:bg-emerald-700 rounded-lg text-white font-bold text-sm"
                  >
                    {t('save')}
                  </button>
                </div>
                <p className="text-xs text-zinc-500">{t('apiKeyStoredLocally')}</p>
              </div>
            )}

            {/* Error Message */}
            {pexelsError && (
              <div className="px-4 py-2 bg-red-500/10 border-b border-red-500/20 text-red-400 text-sm flex items-center gap-2">
                <span>{pexelsError}</span>
                {!pexelsApiKey && (
                  <button
                    onClick={() => setShowPexelsApiKeyInput(true)}
                    className="text-red-300 underline hover:text-red-200"
                  >
                    {t('setApiKey')}
                  </button>
                )}
              </div>
            )}

            {/* Tabs & Search */}
            <div className="p-4 border-b border-white/10 space-y-3">
              {pexelsTarget !== 'albumArt' && (
              <div className="flex gap-2">
                <button
                  onClick={() => { setPexelsTab('photos'); searchPexels(pexelsQuery, 'photos'); }}
                  className={`px-4 py-2 rounded-lg text-sm font-bold flex items-center gap-2 ${pexelsTab === 'photos' ? 'bg-emerald-600 text-white' : 'bg-zinc-800 text-zinc-400'}`}
                >
                  <ImageIcon size={14} /> {t('pexelsPhotos')}
                </button>
                <button
                  onClick={() => { setPexelsTab('videos'); searchPexels(pexelsQuery, 'videos'); }}
                  className={`px-4 py-2 rounded-lg text-sm font-bold flex items-center gap-2 ${pexelsTab === 'videos' ? 'bg-emerald-600 text-white' : 'bg-zinc-800 text-zinc-400'}`}
                >
                  <Video size={14} /> {t('pexelsVideos')}
                </button>
              </div>
              )}
              <div className="flex gap-2">
                <input
                  type="text"
                  value={pexelsQuery}
                  onChange={(e) => setPexelsQuery(e.target.value)}
                  onKeyDown={(e) => e.key === 'Enter' && searchPexels(pexelsQuery, pexelsTab)}
                  placeholder={t('searchBackgrounds')}
                  className="flex-1 bg-zinc-800 rounded-lg px-4 py-2 text-sm text-white border border-white/10 placeholder-zinc-500"
                />
                <button
                  onClick={() => searchPexels(pexelsQuery, pexelsTab)}
                  disabled={pexelsLoading}
                  className="px-4 py-2 bg-emerald-600 hover:bg-emerald-700 rounded-lg text-white font-bold text-sm flex items-center gap-2 disabled:opacity-50"
                >
                  {pexelsLoading ? <Loader2 size={14} className="animate-spin" /> : <Search size={14} />}
                  {t('search')}
                </button>
              </div>
              {/* Quick Tags */}
              <div className="flex flex-wrap gap-2">
                {['abstract', 'nature', 'city', 'space', 'neon', 'particles', 'smoke', 'fire', 'water', 'technology'].map(tag => (
                  <button
                    key={tag}
                    onClick={() => { setPexelsQuery(tag); searchPexels(tag, pexelsTab); }}
                    className="px-3 py-1 bg-zinc-800 hover:bg-zinc-700 rounded-full text-xs text-zinc-400 hover:text-white capitalize"
                  >
                    {tag}
                  </button>
                ))}
              </div>
            </div>

            {/* Results Grid */}
            <div className="flex-1 overflow-y-auto p-4">
              {pexelsLoading ? (
                <div className="flex items-center justify-center h-48">
                  <Loader2 size={32} className="animate-spin text-emerald-500" />
                </div>
              ) : pexelsTab === 'photos' ? (
                <div className="grid grid-cols-3 gap-3">
                  {pexelsPhotos.map(photo => (
                    <button
                      key={photo.id}
                      onClick={() => selectPexelsPhoto(photo)}
                      className="relative group rounded-lg overflow-hidden aspect-video bg-zinc-800"
                    >
                      <img src={photo.src.large} alt="" className="w-full h-full object-cover" />
                      <div className="absolute inset-0 bg-black/60 opacity-0 group-hover:opacity-100 transition-opacity flex items-center justify-center">
                        <span className="text-white text-xs font-bold bg-emerald-600 px-3 py-1 rounded-full">{t('select')}</span>
                      </div>
                      <div className="absolute bottom-0 left-0 right-0 p-2 bg-gradient-to-t from-black/80 to-transparent">
                        <p className="text-[10px] text-zinc-300 truncate">by {photo.photographer}</p>
                      </div>
                    </button>
                  ))}
                </div>
              ) : (
                <div className="grid grid-cols-3 gap-3">
                  {pexelsVideos.map(video => (
                    <button
                      key={video.id}
                      onClick={() => selectPexelsVideo(video)}
                      className="relative group rounded-lg overflow-hidden aspect-video bg-zinc-800"
                    >
                      <img src={video.image} alt="" className="w-full h-full object-cover" />
                      <div className="absolute top-2 right-2 bg-black/60 px-2 py-0.5 rounded text-[10px] text-white font-bold">
                        <Video size={10} className="inline mr-1" />{t('videoBadge')}
                      </div>
                      <div className="absolute inset-0 bg-black/60 opacity-0 group-hover:opacity-100 transition-opacity flex items-center justify-center">
                        <span className="text-white text-xs font-bold bg-emerald-600 px-3 py-1 rounded-full">{t('select')}</span>
                      </div>
                      <div className="absolute bottom-0 left-0 right-0 p-2 bg-gradient-to-t from-black/80 to-transparent">
                        <p className="text-[10px] text-zinc-300 truncate">by {video.user.name}</p>
                      </div>
                    </button>
                  ))}
                </div>
              )}

              {!pexelsLoading && pexelsPhotos.length === 0 && pexelsTab === 'photos' && (
                <p className="text-center text-zinc-500 py-8">{t('noPhotosFound')}</p>
              )}
              {!pexelsLoading && pexelsVideos.length === 0 && pexelsTab === 'videos' && (
                <p className="text-center text-zinc-500 py-8">{t('noVideosFound')}</p>
              )}
            </div>

            {/* Footer */}
            <div className="p-3 border-t border-white/10 bg-zinc-800/50">
              <p className="text-[10px] text-zinc-500 text-center">
                {t('pexelsAttribution')}
              </p>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

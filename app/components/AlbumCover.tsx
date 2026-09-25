import React, { useMemo } from 'react';

interface AlbumCoverProps {
  seed: string;
  size?: 'xs' | 'sm' | 'md' | 'lg' | 'xl' | 'full';
  className?: string;
  children?: React.ReactNode;
  /**
   * A stored cover image for this track. When absent — or when the file cannot
   * be loaded — the deterministic generated artwork is used, so a track always
   * has a cover and never shows a broken image.
   */
  coverUrl?: string;
}

// Seeded random number generator for consistent results
class SeededRandom {
  private seed: number;

  constructor(seed: string) {
    this.seed = this.hashString(seed);
  }

  private hashString(str: string): number {
    let hash = 0;
    for (let i = 0; i < str.length; i++) {
      const char = str.charCodeAt(i);
      hash = ((hash << 5) - hash) + char;
      hash = hash & hash;
    }
    return Math.abs(hash) || 1;
  }

  next(): number {
    this.seed = (this.seed * 1103515245 + 12345) & 0x7fffffff;
    return this.seed / 0x7fffffff;
  }

  range(min: number, max: number): number {
    return min + this.next() * (max - min);
  }

  int(min: number, max: number): number {
    return Math.floor(this.range(min, max));
  }

  pick<T>(arr: T[]): T {
    return arr[this.int(0, arr.length)];
  }
}

// Curated color palettes - music-themed combinations
const palettes = [
  // Sunset Vibes
  { colors: ['#FF6B6B', '#FEC89A', '#FFD93D', '#C9184A'], bg: '#1a1a2e' },
  // Ocean Depths
  { colors: ['#0077B6', '#00B4D8', '#90E0EF', '#CAF0F8'], bg: '#03045E' },
  // Forest Night
  { colors: ['#2D6A4F', '#40916C', '#52B788', '#95D5B2'], bg: '#1B4332' },
  // Neon Dreams
  { colors: ['#F72585', '#7209B7', '#3A0CA3', '#4CC9F0'], bg: '#10002B' },
  // Golden Hour
  { colors: ['#FF9500', '#FF5400', '#FFBD00', '#FFE066'], bg: '#2D1B00' },
  // Arctic Aurora
  { colors: ['#48CAE4', '#00F5D4', '#9B5DE5', '#F15BB5'], bg: '#0A0A1A' },
  // Lavender Haze
  { colors: ['#E0AAFF', '#C77DFF', '#9D4EDD', '#7B2CBF'], bg: '#240046' },
  // Cherry Blossom
  { colors: ['#FFCCD5', '#FFB3C1', '#FF758F', '#C9184A'], bg: '#2B0A14' },
  // Cyber Punk
  { colors: ['#00FF87', '#60EFFF', '#FF00E5', '#FFE500'], bg: '#0D0D0D' },
  // Deep Space
  { colors: ['#7400B8', '#5E60CE', '#4EA8DE', '#56CFE1'], bg: '#03071E' },
  // Warm Ember
  { colors: ['#FFBA08', '#FAA307', '#F48C06', '#E85D04'], bg: '#370617' },
  // Cool Mint
  { colors: ['#64DFDF', '#72EFDD', '#80FFDB', '#5EEAD4'], bg: '#0D3B3B' },
  // Velvet Rose
  { colors: ['#9D174D', '#BE185D', '#DB2777', '#EC4899'], bg: '#1C0A14' },
  // Electric Blue
  { colors: ['#0EA5E9', '#38BDF8', '#7DD3FC', '#E0F2FE'], bg: '#0C1929' },
  // Jungle Fever
  { colors: ['#84CC16', '#A3E635', '#BEF264', '#ECFCCB'], bg: '#1A2E05' },
];

type PatternType = 'aurora' | 'mesh' | 'orbs' | 'rays' | 'waves' | 'geometric' | 'nebula' | 'gradient' | 'rings' | 'crystal';

const generatePattern = (rng: SeededRandom, palette: typeof palettes[0]): React.CSSProperties => {
  const patterns: PatternType[] = ['aurora', 'mesh', 'orbs', 'rays', 'waves', 'geometric', 'nebula', 'gradient', 'rings', 'crystal'];
  const pattern = rng.pick(patterns);
  const colors = palette.colors;
  const bg = palette.bg;

  switch (pattern) {
    case 'aurora': {
      const angle1 = rng.int(0, 360);
      const angle2 = rng.int(0, 360);
      return {
        background: `
          linear-gradient(${angle1}deg, ${colors[0]}00 0%, ${colors[0]}88 25%, ${colors[1]}88 50%, ${colors[2]}88 75%, ${colors[3]}00 100%),
          linear-gradient(${angle2}deg, ${colors[2]}00 0%, ${colors[3]}66 30%, ${colors[0]}66 70%, ${colors[1]}00 100%),
          radial-gradient(ellipse at ${rng.int(20, 80)}% ${rng.int(60, 100)}%, ${colors[1]}44 0%, transparent 50%),
          linear-gradient(180deg, ${bg} 0%, ${colors[3]}22 100%)
        `,
        backgroundColor: bg,
      };
    }

    case 'mesh': {
      const points = [
        { x: rng.int(0, 40), y: rng.int(0, 40) },
        { x: rng.int(60, 100), y: rng.int(0, 40) },
        { x: rng.int(0, 40), y: rng.int(60, 100) },
        { x: rng.int(60, 100), y: rng.int(60, 100) },
      ];
      return {
        background: `
          radial-gradient(at ${points[0].x}% ${points[0].y}%, ${colors[0]} 0%, transparent 50%),
          radial-gradient(at ${points[1].x}% ${points[1].y}%, ${colors[1]} 0%, transparent 50%),
          radial-gradient(at ${points[2].x}% ${points[2].y}%, ${colors[2]} 0%, transparent 50%),
          radial-gradient(at ${points[3].x}% ${points[3].y}%, ${colors[3]} 0%, transparent 50%)
        `,
        backgroundColor: bg,
      };
    }

    case 'orbs': {
      const orbCount = rng.int(3, 6);
      const orbs = Array.from({ length: orbCount }, (_, i) => {
        const size = rng.int(30, 70);
        const x = rng.int(10, 90);
        const y = rng.int(10, 90);
        const color = colors[i % colors.length];
        const blur = rng.int(20, 40);
        return `radial-gradient(circle ${size}% at ${x}% ${y}%, ${color}99 0%, ${color}44 ${blur}%, transparent 70%)`;
      });
      return {
        background: [...orbs, `linear-gradient(135deg, ${bg} 0%, ${colors[0]}11 100%)`].join(', '),
        backgroundColor: bg,
      };
    }

    case 'rays': {
      const centerX = rng.int(30, 70);
      const centerY = rng.int(30, 70);
      const rayCount = rng.int(6, 12);
      const rays = Array.from({ length: rayCount }, (_, i) => {
        const angle = (360 / rayCount) * i + rng.int(-10, 10);
        const color = colors[i % colors.length];
        return `linear-gradient(${angle}deg, transparent 0%, transparent 45%, ${color}66 48%, ${color}66 52%, transparent 55%, transparent 100%)`;
      });
      return {
        background: [
          `radial-gradient(circle at ${centerX}% ${centerY}%, ${colors[0]} 0%, transparent 30%)`,
          ...rays,
        ].join(', '),
        backgroundColor: bg,
      };
    }

    case 'waves': {
      const waveAngle = rng.int(0, 180);
      const waveSize = rng.int(8, 20);
      return {
        background: `
          repeating-linear-gradient(
            ${waveAngle}deg,
            ${colors[0]}44 0px,
            ${colors[1]}44 ${waveSize}px,
            ${colors[2]}44 ${waveSize * 2}px,
            ${colors[3]}44 ${waveSize * 3}px,
            ${colors[0]}44 ${waveSize * 4}px
          ),
          radial-gradient(ellipse at 50% 0%, ${colors[0]}66 0%, transparent 70%),
          radial-gradient(ellipse at 50% 100%, ${colors[2]}66 0%, transparent 70%)
        `,
        backgroundColor: bg,
      };
    }

    case 'geometric': {
      const angle = rng.int(0, 90);
      return {
        background: `
          conic-gradient(from ${angle}deg at 50% 50%, ${colors[0]}, ${colors[1]}, ${colors[2]}, ${colors[3]}, ${colors[0]}),
          repeating-conic-gradient(from 0deg at 50% 50%, ${bg}00 0deg, ${bg}88 ${90/rng.int(2,6)}deg)
        `,
        backgroundBlendMode: 'overlay',
        backgroundColor: bg,
      };
    }

    case 'nebula': {
      const x1 = rng.int(20, 80);
      const y1 = rng.int(20, 80);
      const x2 = rng.int(20, 80);
      const y2 = rng.int(20, 80);
      return {
        background: `
          radial-gradient(ellipse ${rng.int(60, 100)}% ${rng.int(40, 80)}% at ${x1}% ${y1}%, ${colors[0]}88 0%, transparent 50%),
          radial-gradient(ellipse ${rng.int(40, 80)}% ${rng.int(60, 100)}% at ${x2}% ${y2}%, ${colors[1]}88 0%, transparent 50%),
          radial-gradient(ellipse ${rng.int(50, 90)}% ${rng.int(50, 90)}% at ${100-x1}% ${100-y1}%, ${colors[2]}66 0%, transparent 60%),
          radial-gradient(ellipse ${rng.int(30, 60)}% ${rng.int(30, 60)}% at ${100-x2}% ${100-y2}%, ${colors[3]}44 0%, transparent 70%),
          linear-gradient(${rng.int(0, 360)}deg, ${bg} 0%, ${colors[0]}22 50%, ${bg} 100%)
        `,
        backgroundColor: bg,
      };
    }

    case 'gradient': {
      const angle = rng.int(0, 360);
      const type = rng.int(0, 3);
      if (type === 0) {
        return {
          background: `linear-gradient(${angle}deg, ${colors[0]} 0%, ${colors[1]} 33%, ${colors[2]} 66%, ${colors[3]} 100%)`,
        };
      } else if (type === 1) {
        return {
          background: `
            radial-gradient(circle at ${rng.int(30, 70)}% ${rng.int(30, 70)}%, ${colors[0]} 0%, ${colors[1]} 30%, ${colors[2]} 60%, ${colors[3]} 100%)
          `,
        };
      } else {
        return {
          background: `
            linear-gradient(${angle}deg, ${colors[0]} 0%, ${colors[0]} 25%, transparent 25%, transparent 75%, ${colors[2]} 75%),
            linear-gradient(${angle + 90}deg, ${colors[1]} 0%, ${colors[1]} 25%, transparent 25%, transparent 75%, ${colors[3]} 75%),
            linear-gradient(${angle}deg, ${colors[2]} 0%, ${colors[3]} 100%)
          `,
          backgroundBlendMode: 'multiply, screen, normal',
        };
      }
    }

    case 'rings': {
      const centerX = rng.int(30, 70);
      const centerY = rng.int(30, 70);
      return {
        background: `
          repeating-radial-gradient(circle at ${centerX}% ${centerY}%,
            ${colors[0]}66 0px, ${colors[0]}66 2px,
            transparent 2px, transparent ${rng.int(15, 25)}px,
            ${colors[1]}66 ${rng.int(15, 25)}px, ${colors[1]}66 ${rng.int(17, 27)}px,
            transparent ${rng.int(17, 27)}px, transparent ${rng.int(35, 50)}px
          ),
          radial-gradient(circle at ${centerX}% ${centerY}%, ${colors[2]}88 0%, transparent 60%),
          linear-gradient(${rng.int(0, 180)}deg, ${colors[3]}44, ${colors[0]}44)
        `,
        backgroundColor: bg,
      };
    }

    case 'crystal': {
      const facets = rng.int(4, 8);
      const gradients = Array.from({ length: facets }, (_, i) => {
        const startAngle = (360 / facets) * i;
        const color = colors[i % colors.length];
        return `conic-gradient(from ${startAngle}deg at ${50 + rng.int(-20, 20)}% ${50 + rng.int(-20, 20)}%, ${color}88 0deg, transparent ${360/facets}deg)`;
      });
      return {
        background: [
          ...gradients,
          `radial-gradient(circle at 50% 50%, ${colors[0]}44 0%, transparent 70%)`,
        ].join(', '),
        backgroundColor: bg,
      };
    }

    default:
      return {
        background: `linear-gradient(135deg, ${colors[0]}, ${colors[1]})`,
      };
  }
};

export const AlbumCover: React.FC<AlbumCoverProps> = ({ seed, size = 'md', className = '', children, coverUrl }) => {
  const [imageFailed, setImageFailed] = React.useState(false);
  React.useEffect(() => { setImageFailed(false); }, [coverUrl]);
  const coverStyle = useMemo(() => {
    const rng = new SeededRandom(seed);
    const palette = rng.pick(palettes);
    return generatePattern(rng, palette);
  }, [seed]);

  const sizeClasses: Record<string, string> = {
    xs: 'w-8 h-8',
    sm: 'w-10 h-10',
    md: 'w-12 h-12',
    lg: 'w-14 h-14',
    xl: 'w-48 h-48',
    full: 'w-full h-full',
  };

  return (
    <div
      className={`${sizeClasses[size]} rounded-md shadow-lg flex-shrink-0 overflow-hidden relative ${className}`}
      style={coverStyle}
    >
      {coverUrl && !imageFailed && (
        <img
          src={coverUrl}
          alt=""
          loading="lazy"
          onError={() => setImageFailed(true)}
          className="absolute inset-0 h-full w-full object-cover"
        />
      )}
      {children}
    </div>
  );
};

export default AlbumCover;

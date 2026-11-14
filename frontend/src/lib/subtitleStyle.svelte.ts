// Gerenciamento e persistência de estilização das legendas

export type SubtitleStyle = {
  fontSizePercent: number; // 75, 100, 125, 150, 175, 200 (Default: 100 = 24px/1.5rem)
  textColor: string; // hex
  bgColor: string; // hex
  bgOpacity: number; // 0 to 100
  bottomOffset: number; // 20 to 120 px (Default: 48)
  fontFamily: string; // e.g. 'Inter', 'Roboto', 'Montserrat'
  textShadow: 'outline' | 'shadow' | 'none';
};

export const DEFAULT_SUBTITLE_STYLE: SubtitleStyle = {
  fontSizePercent: 100,
  textColor: '#ffffff',
  bgColor: '#000000',
  bgOpacity: 75,
  bottomOffset: 48,
  fontFamily: 'Inter',
  textShadow: 'outline'
};

const STORAGE_KEY = 'mymedia_subtitle_style';

export const POPULAR_GOOGLE_FONTS = [
  { name: 'Inter', category: 'Sans-Serif Moderna' },
  { name: 'Roboto', category: 'Padrão YouTube' },
  { name: 'Montserrat', category: 'Geométrica' },
  { name: 'Open Sans', category: 'Neutra & Legível' },
  { name: 'Oswald', category: 'Condensada Cinema' },
  { name: 'Poppins', category: 'Arredondada' },
  { name: 'Ubuntu', category: 'Distinta' },
  { name: 'Fira Code', category: 'Monoespaçada' },
  { name: 'Playfair Display', category: 'Serifada Clássica' }
];

export function hexToRgba(hex: string, opacityPercent: number): string {
  let clean = hex.replace('#', '');
  if (clean.length === 3) {
    clean = clean
      .split('')
      .map((c) => c + c)
      .join('');
  }
  const num = parseInt(clean, 16);
  if (isNaN(num)) return `rgba(0, 0, 0, ${opacityPercent / 100})`;
  const r = (num >> 16) & 255;
  const g = (num >> 8) & 255;
  const b = num & 255;
  return `rgba(${r}, ${g}, ${b}, ${Math.max(0, Math.min(100, opacityPercent)) / 100})`;
}

export function loadGoogleFont(fontName: string) {
  if (typeof document === 'undefined' || !fontName) return;
  const standardFonts = ['sans-serif', 'serif', 'monospace', 'system-ui', 'arial', 'helvetica'];
  if (standardFonts.includes(fontName.toLowerCase())) return;

  const safeId = `google-font-${fontName.toLowerCase().replace(/[^a-z0-9]/g, '-')}`;
  if (document.getElementById(safeId)) return;

  const link = document.createElement('link');
  link.id = safeId;
  link.rel = 'stylesheet';
  link.href = `https://fonts.googleapis.com/css2?family=${encodeURIComponent(fontName)}:wght@400;600;700&display=swap`;
  document.head.appendChild(link);
}

export function applySubtitleStyleToDom(style: SubtitleStyle, targetElement?: HTMLElement | null) {
  const el = targetElement ?? (typeof document !== 'undefined' ? document.documentElement : null);
  if (!el) return;

  const fontSizeRem = (style.fontSizePercent / 100) * 1.5;
  el.style.setProperty('--sub-font-size', `${fontSizeRem}rem`);
  el.style.setProperty('--sub-color', style.textColor);
  el.style.setProperty('--sub-bottom-offset', `${style.bottomOffset}px`);
  el.style.setProperty('--sub-font-family', `"${style.fontFamily}", Inter, system-ui, sans-serif`);

  // Background
  if (style.bgOpacity <= 0) {
    el.style.setProperty('--sub-bg', 'transparent');
    el.style.setProperty('--sub-backdrop-filter', 'none');
    el.style.setProperty('--sub-box-shadow', 'none');
  } else {
    el.style.setProperty('--sub-bg', hexToRgba(style.bgColor, style.bgOpacity));
    el.style.setProperty('--sub-backdrop-filter', 'blur(4px)');
    el.style.setProperty('--sub-box-shadow', '0 4px 12px rgba(0, 0, 0, 0.4)');
  }

  // Shadow / Outline
  if (style.textShadow === 'outline') {
    el.style.setProperty(
      '--sub-text-shadow',
      '-1.5px -1.5px 0 #000, 1.5px -1.5px 0 #000, -1.5px 1.5px 0 #000, 1.5px 1.5px 0 #000, 0 2px 5px rgba(0,0,0,0.9)'
    );
  } else if (style.textShadow === 'shadow') {
    el.style.setProperty('--sub-text-shadow', '0 2px 4px rgba(0, 0, 0, 0.9), 0 0 2px rgba(0, 0, 0, 0.8)');
  } else {
    el.style.setProperty('--sub-text-shadow', 'none');
  }

  loadGoogleFont(style.fontFamily);
}

function loadSavedSubtitleStyle(): SubtitleStyle {
  if (typeof localStorage === 'undefined') return { ...DEFAULT_SUBTITLE_STYLE };
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<SubtitleStyle>;
      return {
        fontSizePercent: typeof parsed.fontSizePercent === 'number' ? parsed.fontSizePercent : DEFAULT_SUBTITLE_STYLE.fontSizePercent,
        textColor: typeof parsed.textColor === 'string' ? parsed.textColor : DEFAULT_SUBTITLE_STYLE.textColor,
        bgColor: typeof parsed.bgColor === 'string' ? parsed.bgColor : DEFAULT_SUBTITLE_STYLE.bgColor,
        bgOpacity: typeof parsed.bgOpacity === 'number' ? parsed.bgOpacity : DEFAULT_SUBTITLE_STYLE.bgOpacity,
        bottomOffset: typeof parsed.bottomOffset === 'number' ? parsed.bottomOffset : DEFAULT_SUBTITLE_STYLE.bottomOffset,
        fontFamily: typeof parsed.fontFamily === 'string' && parsed.fontFamily.trim() ? parsed.fontFamily : DEFAULT_SUBTITLE_STYLE.fontFamily,
        textShadow: parsed.textShadow === 'outline' || parsed.textShadow === 'shadow' || parsed.textShadow === 'none' ? parsed.textShadow : DEFAULT_SUBTITLE_STYLE.textShadow
      };
    }
  } catch {}
  return { ...DEFAULT_SUBTITLE_STYLE };
}

class SubtitleStyleStore {
  current = $state<SubtitleStyle>(loadSavedSubtitleStyle());

  constructor() {
    if (typeof window !== 'undefined') {
      applySubtitleStyleToDom(this.current);
    }
  }

  update(patch: Partial<SubtitleStyle>) {
    this.current = { ...this.current, ...patch };
    this.save();
    applySubtitleStyleToDom(this.current);
  }

  reset() {
    this.current = { ...DEFAULT_SUBTITLE_STYLE };
    this.save();
    applySubtitleStyleToDom(this.current);
  }

  private save() {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(this.current));
    } catch {}
  }
}

export const subtitleStyleStore = new SubtitleStyleStore();

export function formatCueHtml(raw: string): string {
  if (!raw) return '';
  return raw
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/\n/g, '<br />')
    .replace(/&lt;(\/?(?:b|i|u))&gt;/gi, '<$1>')
    .replace(/&lt;c(?:\.[a-z0-9_-]+)*&gt;/gi, '')
    .replace(/&lt;\/c&gt;/gi, '');
}

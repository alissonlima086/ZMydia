import type { SupportedLocale } from './types';
import ptBR from './locales/pt-BR.json';
import enUS from './locales/en-US.json';
import esES from './locales/es-ES.json';

const STORAGE_KEY = 'mymedia_language';

const dictionaries: Record<SupportedLocale, Record<string, any>> = {
  'pt-BR': ptBR,
  'en-US': enUS,
  'es-ES': esES
};

function getInitialLocale(): SupportedLocale {
  if (typeof window === 'undefined') return 'pt-BR';
  const saved = typeof localStorage !== 'undefined' ? localStorage.getItem(STORAGE_KEY) : null;
  if (saved === 'pt-BR' || saved === 'en-US' || saved === 'es-ES') {
    return saved;
  }
  const navLang = typeof navigator !== 'undefined' ? navigator.language.toLowerCase() : '';
  if (navLang.startsWith('es')) return 'es-ES';
  if (navLang.startsWith('en')) return 'en-US';
  return 'pt-BR';
}

class I18nStore {
  currentLocale = $state<SupportedLocale>(getInitialLocale());

  constructor() {
    if (typeof document !== 'undefined') {
      document.documentElement.lang = this.currentLocale;
    }
  }

  setLocale(locale: SupportedLocale) {
    this.currentLocale = locale;
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(STORAGE_KEY, locale);
    }
    if (typeof document !== 'undefined') {
      document.documentElement.lang = locale;
    }
  }

  t(path: string, params?: Record<string, string | number>): string {
    const keys = path.split('.');
    const dict = dictionaries[this.currentLocale] || dictionaries['pt-BR'];
    const fallbackDict = dictionaries['pt-BR'];

    let val: any = dict;
    for (const k of keys) {
      if (val && typeof val === 'object' && k in val) {
        val = val[k];
      } else {
        val = undefined;
        break;
      }
    }

    if (typeof val !== 'string') {
      let fbVal: any = fallbackDict;
      for (const k of keys) {
        if (fbVal && typeof fbVal === 'object' && k in fbVal) {
          fbVal = fbVal[k];
        } else {
          fbVal = undefined;
          break;
        }
      }
      val = typeof fbVal === 'string' ? fbVal : path;
    }

    if (params) {
      for (const [pKey, pVal] of Object.entries(params)) {
        val = val.replaceAll(`{${pKey}}`, String(pVal));
      }
    }

    return val;
  }
}

export const i18n = new I18nStore();
export const t = (path: string, params?: Record<string, string | number>) => i18n.t(path, params);

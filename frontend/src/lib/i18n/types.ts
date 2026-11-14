export type SupportedLocale = 'pt-BR' | 'en-US' | 'es-ES';

export interface LocaleInfo {
  code: SupportedLocale;
  nativeName: string;
  flag: string;
}

export const SUPPORTED_LOCALES: LocaleInfo[] = [
  {
    code: 'pt-BR',
    nativeName: 'Português (Brasil)',
    flag: '🇧🇷'
  },
  {
    code: 'en-US',
    nativeName: 'English (US)',
    flag: '🇺🇸'
  },
  {
    code: 'es-ES',
    nativeName: 'Español',
    flag: '🇪🇸'
  }
];

export type TranslationDict = Record<string, any>;

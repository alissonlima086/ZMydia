import { describe, expect, it } from 'vitest';
import ptBR from './locales/pt-BR.json';
import enUS from './locales/en-US.json';
import esES from './locales/es-ES.json';

function flatten(value: unknown, prefix = ''): Record<string, string> {
  if (typeof value === 'string') return { [prefix]: value };
  if (!value || typeof value !== 'object') return {};

  return Object.entries(value).reduce<Record<string, string>>((result, [key, child]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    return Object.assign(result, flatten(child, path));
  }, {});
}

describe('translation dictionaries', () => {
  const dictionaries = {
    'pt-BR': flatten(ptBR),
    'en-US': flatten(enUS),
    'es-ES': flatten(esES)
  };
  const referenceKeys = Object.keys(dictionaries['pt-BR']).sort();

  it.each(Object.entries(dictionaries))('%s has every translation key', (_locale, dictionary) => {
    expect(Object.keys(dictionary).sort()).toEqual(referenceKeys);
  });

  it.each(Object.entries(dictionaries))('%s has no blank translations', (_locale, dictionary) => {
    expect(Object.entries(dictionary).filter(([, value]) => value.trim() === '')).toEqual([]);
  });

  it.each(Object.entries(dictionaries))('%s preserves interpolation placeholders', (_locale, dictionary) => {
    const placeholders = (value: string) => (value.match(/\{[^}]+\}/g) ?? []).map((match) => match.slice(1, -1)).sort();
    const mismatches = referenceKeys.filter((key) =>
      placeholders(dictionary[key]).join('|') !== placeholders(dictionaries['pt-BR'][key]).join('|')
    );
    expect(mismatches).toEqual([]);
  });
});

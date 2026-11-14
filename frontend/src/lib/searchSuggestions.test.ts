import { describe, expect, it } from 'vitest';
import { selectChannelSuggestions, type ChannelSuggestion } from './searchSuggestions';

const channels: ChannelSuggestion[] = [
  { id: 'unrelated', name: 'Alpha', handle: '@alpha', media_count: 100 },
  { id: 'middle', name: 'The Aurora Live', handle: '@live', media_count: 8 },
  { id: 'prefix-small', name: 'Aurora Archive', handle: '@archive', media_count: 2 },
  { id: 'exact', name: 'Aurora', handle: '@aurora', media_count: 1 },
  { id: 'prefix-large', name: 'Aurora Atlas', handle: '@atlas', media_count: 20 },
  { id: 'fourth', name: 'Aurora Sessions', handle: '@sessions', media_count: 3 }
];

describe('channel search suggestions', () => {
  it('filters a full channel response and always returns at most three matches', () => {
    const results = selectChannelSuggestions(channels, 'aurora');

    expect(results.map((channel) => channel.id)).toEqual(['exact', 'prefix-large', 'fourth']);
    expect(results).toHaveLength(3);
    expect(results.some((channel) => channel.id === 'unrelated')).toBe(false);
  });

  it('also searches channel handles', () => {
    expect(selectChannelSuggestions(channels, '@live')).toEqual([channels[1]]);
  });

  it('excludes channels containing configured hidden words', () => {
    const results = selectChannelSuggestions(channels, 'aurora', 3, ['atlas', '#sessions']);

    expect(results.map((channel) => channel.id)).toEqual(['exact', 'prefix-small', 'middle']);
    expect(results.some((channel) => channel.id === 'prefix-large')).toBe(false);
    expect(results.some((channel) => channel.id === 'fourth')).toBe(false);
  });
});

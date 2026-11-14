import { describe, expect, it } from 'vitest';
import { extractVideoId, isNavigationPathActive, normalizePath, parseChannelRoute } from './router';

describe('router', () => {
  it('normalizes leading and trailing slashes', () => {
    expect(normalizePath('historico/')).toBe('/historico');
    expect(normalizePath('/')).toBe('/');
  });

  it('keeps playlist subroutes active without matching unrelated paths', () => {
    expect(isNavigationPathActive('/playlist/42', '/playlists')).toBe(true);
    expect(isNavigationPathActive('/playlists/favoritas', '/playlists')).toBe(true);
    expect(isNavigationPathActive('/playlists-extra', '/playlists')).toBe(false);
  });

  it('keeps parent navigation entries active on detail aliases', () => {
    expect(isNavigationPathActive('/watch/21', '/')).toBe(true);
    expect(isNavigationPathActive('/canal/Canal%20Exemplo', '/canais')).toBe(true);
    expect(isNavigationPathActive('/videos', '/biblioteca')).toBe(true);
  });

  it('parses channel ids and tabs safely', () => {
    expect(parseChannelRoute('/canal/Canal%20Exemplo/mencoes')).toEqual({
      channelId: 'Canal Exemplo',
      tab: 'mencoes'
    });
    expect(parseChannelRoute('/canal/%E0%A4%A')).toBeNull();
  });

  it('extracts positive video ids from query or path', () => {
    expect(extractVideoId('/watch?v=21')).toBe(21);
    expect(extractVideoId('/video/34')).toBe(34);
    expect(extractVideoId('/watch?v=-1')).toBeNull();
    expect(extractVideoId('/video/not-a-number')).toBeNull();
  });
});

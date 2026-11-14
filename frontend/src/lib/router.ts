const CHANNEL_PREFIX = '/canal/';

export type ChannelRoute = {
  channelId: string;
  tab: string;
};

export function normalizePath(pathname: string): string {
  if (!pathname) return '/';
  const withLeadingSlash = pathname.startsWith('/') ? pathname : `/${pathname}`;
  if (withLeadingSlash === '/') return '/';
  return withLeadingSlash.replace(/\/+$/, '') || '/';
}

export function isNavigationPathActive(currentPath: string, itemPath: string): boolean {
  const current = normalizePath(currentPath);
  const item = normalizePath(itemPath);

  if (item === '/playlists') {
    return current === item || current.startsWith('/playlists/') || current.startsWith('/playlist/');
  }

  if (item === '/') {
    return current === '/' || current === '/watch' || current.startsWith('/watch/') || current.startsWith('/video/');
  }

  if (item === '/canais') {
    return current === item || current.startsWith('/canal/');
  }

  if (item === '/biblioteca') {
    return current === item || current === '/videos';
  }

  return current === item;
}

export function parseChannelRoute(pathname: string): ChannelRoute | null {
  const path = normalizePath(pathname);
  if (!path.startsWith(CHANNEL_PREFIX)) return null;

  const [encodedId, tab = 'home'] = path.slice(CHANNEL_PREFIX.length).split('/');
  if (!encodedId) return null;

  try {
    return { channelId: decodeURIComponent(encodedId), tab };
  } catch {
    return null;
  }
}

export function extractVideoId(urlLike: string | URL): number | null {
  const url = typeof urlLike === 'string' ? new URL(urlLike, 'http://localhost') : urlLike;
  const queryId = parsePositiveInteger(url.searchParams.get('v'));
  if (queryId !== null) return queryId;

  const match = normalizePath(url.pathname).match(/^\/(?:watch|video)\/(\d+)$/);
  return parsePositiveInteger(match?.[1] ?? null);
}

function parsePositiveInteger(value: string | null): number | null {
  if (!value || !/^\d+$/.test(value)) return null;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : null;
}

export function pushRoute(path: string): string {
  window.history.pushState({}, '', path);
  window.dispatchEvent(new Event('routechange'));
  return window.location.pathname;
}

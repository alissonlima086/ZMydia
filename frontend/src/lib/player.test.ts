import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('./api', () => ({
  apiFetch: vi.fn(async (input: string) => {
    if (input.includes('/subtitles')) return Response.json([]);
    if (input === '/api/playback/79') {
      return Response.json({ media_id: 79, position_ms: 30_000, duration_ms: 120_000, completed: false });
    }
    if (input === '/api/playback/80') {
      return Response.json({ media_id: 80, position_ms: 120_000, duration_ms: 120_000, completed: true });
    }
    if (input.startsWith('/api/playback/')) {
      return Response.json({ media_id: 77, position_ms: 0, duration_ms: 120_000, completed: false });
    }
    return Response.json({});
  })
}));

vi.mock('./music.svelte', () => ({
  musicStore: {
    resetPlaybackSession: vi.fn(),
    loadTrackForMedia: vi.fn(),
    onPlaybackTick: vi.fn()
  }
}));

afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetModules();
});

describe('player mounting', () => {
  it('loads a pending media source when the video element mounts after the click', async () => {
    vi.stubGlobal('window', { addEventListener: vi.fn(), location: { pathname: '/', search: '' } });
    vi.stubGlobal('history', { pushState: vi.fn() });

    const { player } = await import('./player.svelte');
    const media = {
      id: 77,
      title: 'Pending video',
      duration_ms: 120_000,
      width: 1920,
      height: 1080,
      rotation: 0,
      is_short: false,
      is_available: true,
      channel: null,
      tags: [],
      thumbnail_url: '/thumb.jpg'
    };

    await player.playMedia(media, true, false);
    expect(player.currentMedia?.id).toBe(77);

    const listeners = new Map<string, EventListener>();
    const video = {
      src: '',
      paused: true,
      volume: 1,
      muted: false,
      playbackRate: 1,
      loop: false,
      duration: 120,
      currentTime: 0,
      readyState: 1,
      textTracks: [],
      addEventListener: vi.fn((name: string, listener: EventListener) => listeners.set(name, listener)),
      play: vi.fn(async () => undefined),
      pause: vi.fn()
    } as unknown as HTMLVideoElement;

    player.attach(video);

    await vi.waitFor(() => expect(video.src).toBe('/api/media/77/stream'));
    expect(video.play).toHaveBeenCalledOnce();
  });

  it('only advances after the video ends when autoplay is enabled', async () => {
    const stored = new Map<string, string>();
    vi.stubGlobal('localStorage', {
      getItem: vi.fn((key: string) => stored.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => stored.set(key, value))
    });
    vi.stubGlobal('window', { addEventListener: vi.fn(), location: { pathname: '/', search: '' } });
    vi.stubGlobal('history', { pushState: vi.fn() });

    const { player } = await import('./player.svelte');
    const listeners = new Map<string, EventListener>();
    const video = {
      volume: 1,
      muted: false,
      playbackRate: 1,
      loop: false,
      duration: 120,
      currentTime: 120,
      addEventListener: vi.fn((name: string, listener: EventListener) => listeners.set(name, listener))
    } as unknown as HTMLVideoElement;
    const media = {
      id: 77,
      title: 'First video',
      duration_ms: 120_000,
      width: 1920,
      height: 1080,
      rotation: 0,
      is_short: false,
      is_available: true,
      channel: null,
      tags: [],
      thumbnail_url: '/first.jpg'
    };

    player.queue = [media, { ...media, id: 78, title: 'Second video' }];
    player.currentIndex = 0;
    player.attach(video);
    const playNext = vi.spyOn(player, 'playNext').mockResolvedValue();
    const ended = listeners.get('ended') as unknown as () => void;

    ended();
    expect(playNext).not.toHaveBeenCalled();

    player.toggleAutoplay();
    ended();
    expect(playNext).toHaveBeenCalledOnce();
    expect(stored.get('mymedia_player_autoplay')).toBe('true');
  });

  it('uses the new media progress instead of inheriting the previous video position', async () => {
    vi.stubGlobal('window', { addEventListener: vi.fn(), location: { pathname: '/', search: '' } });
    vi.stubGlobal('history', { pushState: vi.fn() });

    const { player } = await import('./player.svelte');
    const media = {
      id: 77,
      title: 'Previous video',
      duration_ms: 120_000,
      width: 1920,
      height: 1080,
      rotation: 0,
      is_short: false,
      is_available: true,
      channel: null,
      tags: [],
      thumbnail_url: '/previous.jpg'
    };
    const video = {
      src: '/api/media/77/stream',
      paused: false,
      volume: 1,
      muted: false,
      playbackRate: 1,
      loop: false,
      duration: 120,
      currentTime: 120,
      readyState: 1,
      textTracks: [],
      addEventListener: vi.fn(),
      play: vi.fn(async () => undefined),
      pause: vi.fn()
    } as unknown as HTMLVideoElement;

    player.currentMedia = media;
    player.videoElement = video;

    await player.playMedia({ ...media, id: 78, title: 'Unwatched video' }, true, false);
    expect(video.currentTime).toBe(0);
    expect(player.currentTime).toBe(0);

    video.currentTime = 60;
    await player.playMedia({ ...media, id: 79, title: 'Partially watched video' }, true, false);
    expect(video.currentTime).toBe(30);
    expect(player.currentTime).toBe(30);

    video.currentTime = 60;
    await player.playMedia({ ...media, id: 80, title: 'Completed video' }, true, false);
    expect(video.currentTime).toBe(0);
    expect(player.currentTime).toBe(0);
  });

  it('pauses regular playback when an exclusive view such as Shorts opens', async () => {
    vi.stubGlobal('window', { addEventListener: vi.fn(), location: { pathname: '/shorts', search: '' } });
    const { player } = await import('./player.svelte');
    const pause = vi.fn();
    player.videoElement = { paused: false, pause, currentTime: 12 } as unknown as HTMLVideoElement;
    player.isPlaying = true;

    player.pauseForExclusivePlayback();

    expect(pause).toHaveBeenCalledOnce();
    expect(player.isPlaying).toBe(false);
  });
});

import { apiFetch } from './api';
import { t } from './i18n/i18n.svelte';
import { musicStore } from './music.svelte';

export type MediaItem = {
  id: number;
  title: string;
  duration_ms: number | null;
  width: number | null;
  height: number | null;
  rotation: number | null;
  is_short: boolean;
  is_available: boolean;
  is_favorite?: boolean;
  is_official_thumbnail?: boolean;
  description?: string | null;
  user_notes?: string | null;
  youtube_id?: string | null;
  published_at?: string | null;
  channel: {
    id: string;
    name: string;
    youtube_channel_id?: string | null;
    handle?: string | null;
    avatar_url?: string | null;
  } | null;
  tags: { id: number; display_name: string; normalized_name: string }[];
  thumbnail_url: string;
  playback?: {
    position_ms: number;
    duration_ms: number | null;
    completed: boolean;
  } | null;
};

export type SubtitleTrack = {
  id: number;
  lang_code: string;
  label: string;
  source: string;
  url: string;
};

export type PlaylistContext = {
  id?: string;
  title: string;
  author?: string | null;
  items: MediaItem[];
  isLooping: boolean;
  isShuffled: boolean;
};

type PlaybackState = {
  media_id: number;
  position_ms: number;
  duration_ms: number | null;
  completed: boolean;
};

const STORAGE_VOLUME_KEY = 'mymedia_player_volume';
const STORAGE_MUTED_KEY = 'mymedia_player_muted';
const STORAGE_RATE_KEY = 'mymedia_player_speed';
const STORAGE_AUTOPLAY_KEY = 'mymedia_player_autoplay';
const STORAGE_SUBTITLE_LANG_KEY = 'mymedia_player_subtitle_lang';

function getStoredSubtitleLang(): string | null {
  try {
    const val = localStorage.getItem(STORAGE_SUBTITLE_LANG_KEY);
    if (val !== null && val !== '') return val;
  } catch {}
  return null;
}

function getStoredVolume(): number {
  try {
    const val = localStorage.getItem(STORAGE_VOLUME_KEY);
    if (val !== null) {
      const parsed = parseFloat(val);
      if (!isNaN(parsed) && parsed >= 0 && parsed <= 1) return parsed;
    }
  } catch {}
  return 1;
}

function getStoredMuted(): boolean {
  try {
    const val = localStorage.getItem(STORAGE_MUTED_KEY);
    if (val !== null) return val === 'true';
  } catch {}
  return false;
}

function getStoredRate(): number {
  try {
    const val = localStorage.getItem(STORAGE_RATE_KEY);
    if (val !== null) {
      const parsed = parseFloat(val);
      if (!isNaN(parsed) && parsed >= 0.25 && parsed <= 4) return parsed;
    }
  } catch {}
  return 1;
}

function getStoredAutoplay(): boolean {
  try {
    return localStorage.getItem(STORAGE_AUTOPLAY_KEY) === 'true';
  } catch {}
  return false;
}

class PlayerStore {
  currentMedia = $state<MediaItem | null>(null);
  queue = $state<MediaItem[]>([]);
  currentIndex = $state<number>(-1);
  isPlaying = $state(false);
  currentTime = $state(0);
  duration = $state(0);
  volume = $state(getStoredVolume());
  isMuted = $state(getStoredMuted());
  playbackRate = $state(getStoredRate());
  isAutoplayEnabled = $state(getStoredAutoplay());
  isExpanded = $state(false);
  isLoading = $state(false);
  errorMessage = $state<string | null>(null);
  playbackMap = $state<Record<number, { position_ms: number; duration_ms: number | null; completed: boolean }>>({});
  availableSubtitles = $state<SubtitleTrack[]>([]);
  selectedSubtitleLang = $state<string | null>(getStoredSubtitleLang());
  isDownloadingSubtitles = $state(false);
  activeCueText = $state<string | null>(null);
  isLooping = $state(false);
  videoFit = $state<'contain' | 'cover'>('contain');
  playlistContext = $state<PlaylistContext | null>(null);
  isPlaylistExpanded = $state(true);

  getPlayback(mediaId: number) {
    return this.playbackMap[mediaId] ?? null;
  }

  videoElement: HTMLVideoElement | null = null;
  previousUrl: string = '/';
  private pendingMedia: MediaItem | null = null;
  private syncInterval: number | null = null;
  private lastSavedPositionMs = 0;

  // Sessão de scrobble interno (50% assistidos)
  private internalScrobbleMediaId: number | null = null;
  private internalAccumulatedWatchMs = 0;
  private internalLastPositionMs = 0;
  private internalHasScrobbled = false;
  private engagementSessionId: string | null = null;

  resetInternalScrobbleSession() {
    this.internalAccumulatedWatchMs = 0;
    this.internalLastPositionMs = 0;
    this.internalHasScrobbled = false;
  }

  private beginEngagementSession() {
    this.engagementSessionId =
      typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
        ? crypto.randomUUID()
        : `session-${Date.now()}-${Math.random().toString(36).slice(2)}`;
  }

  private async recordPlaybackEngagement(completed = false, keepalive = false) {
    const media = this.currentMedia;
    const sessionId = this.engagementSessionId;
    const durationMs = Math.floor((this.videoElement?.duration || this.duration || 0) * 1000);
    if (!media || !sessionId || durationMs <= 0 || this.internalAccumulatedWatchMs < 1000) return;

    try {
      await apiFetch(`/api/playback/${media.id}/engagement`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          session_id: sessionId,
          watched_ms: Math.min(this.internalAccumulatedWatchMs, durationMs),
          duration_ms: durationMs,
          completed
        }),
        keepalive
      });
    } catch {
      // A retenção melhora recomendações, mas nunca deve interromper a reprodução.
    }
  }

  private async recordInternalScrobble(mediaId: number, durationMs: number | null) {
    try {
      await apiFetch(`/api/playback/${mediaId}/scrobble`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          duration_ms: durationMs && durationMs > 0 ? durationMs : null
        })
      });
    } catch {
      // Falha silenciosa de telemetria
    }
  }

  private trackInternalScrobbleTick(mediaId: number, currentPositionMs: number, durationMs: number) {
    if (durationMs <= 0) return;

    if (this.internalScrobbleMediaId !== mediaId) {
      this.internalScrobbleMediaId = mediaId;
      this.resetInternalScrobbleSession();
    }

    // Detecta volta ao início (loop ou rewind manual) após ter passado de 40%
    if (this.internalLastPositionMs > durationMs * 0.4 && currentPositionMs < durationMs * 0.2) {
      this.resetInternalScrobbleSession();
    }

    if (this.internalLastPositionMs > 0 && currentPositionMs > this.internalLastPositionMs) {
      const delta = currentPositionMs - this.internalLastPositionMs;
      // Aceita delta até 10s para acomodar abas em segundo plano no navegador
      if (delta > 0 && delta <= 10000) {
        this.internalAccumulatedWatchMs += delta;
      }
    }
    this.internalLastPositionMs = currentPositionMs;

    const requiredWatchMs = Math.min(durationMs * 0.5, 4 * 60 * 1000);
    if (!this.internalHasScrobbled && this.internalAccumulatedWatchMs >= requiredWatchMs) {
      this.internalHasScrobbled = true;
      void this.recordInternalScrobble(mediaId, durationMs);
    }
  }

  openVideoUrl(mediaId: number) {
    if (typeof window === 'undefined') return;
    if (window.location.pathname !== '/watch') {
      this.previousUrl = window.location.pathname + window.location.search;
    }
    const listParam = this.playlistContext?.id ? `&list=${encodeURIComponent(this.playlistContext.id)}` : '';
    const target = `/watch?v=${mediaId}${listParam}`;
    if (window.location.pathname + window.location.search !== target) {
      history.pushState({ videoId: mediaId }, '', target);
    }
  }

  restorePreviousUrl() {
    if (typeof window === 'undefined') return;
    if (window.location.pathname === '/watch' || window.location.search.includes('v=')) {
      const fallback = this.previousUrl && this.previousUrl !== '/watch' ? this.previousUrl : '/';
      history.pushState({}, '', fallback);
    }
  }

  attach(video: HTMLVideoElement) {
    if (this.videoElement === video) return;
    this.videoElement = video;

    video.volume = this.volume;
    video.muted = this.isMuted;
    video.playbackRate = this.playbackRate;
    video.loop = this.isLooping;

    video.addEventListener('play', () => {
      this.isPlaying = true;
      this.startSyncTimer();
    });

    video.addEventListener('pause', () => {
      this.isPlaying = false;
      this.stopSyncTimer();
      void this.saveProgress();
      void this.recordPlaybackEngagement();
    });

    video.addEventListener('timeupdate', () => {
      this.currentTime = video.currentTime;
      this.updateActiveCues();

      // Dispara o salvamento inicial no histórico assim que atinge 5 segundos de reprodução
      const currentMs = Math.floor(video.currentTime * 1000);
      if (currentMs >= 5000 && this.lastSavedPositionMs < 5000) {
        void this.saveProgress();
      }

      if (this.currentMedia) {
        const durationMs = Math.floor((video.duration || this.duration || 0) * 1000);
        musicStore.onPlaybackTick(this.currentMedia.id, currentMs, durationMs);
        this.trackInternalScrobbleTick(this.currentMedia.id, currentMs, durationMs);
      }
    });

    video.addEventListener('seeked', () => {
      this.updateActiveCues();
    });

    video.addEventListener('durationchange', () => {
      if (video.duration && !isNaN(video.duration)) {
        this.duration = video.duration;
      }
    });

    video.addEventListener('waiting', () => {
      this.isLoading = true;
    });

    video.addEventListener('canplay', () => {
      this.isLoading = false;
      this.errorMessage = null;
    });

    video.addEventListener('ended', () => {
      this.isPlaying = false;
      this.stopSyncTimer();
      void this.saveProgress();
      if (this.currentMedia) {
        const currentMs = Math.floor(video.currentTime * 1000);
        const durationMs = Math.floor((video.duration || this.duration || 0) * 1000);
        musicStore.onPlaybackTick(this.currentMedia.id, currentMs, durationMs);
        this.trackInternalScrobbleTick(this.currentMedia.id, currentMs, durationMs);
        void this.recordPlaybackEngagement(true);
      }
      if (this.isLooping) {
        this.resetInternalScrobbleSession();
        this.beginEngagementSession();
        musicStore.resetPlaybackSession();
        if (this.videoElement) {
          this.videoElement.currentTime = 0;
          void this.videoElement.play();
        }
        return;
      }
      if (!this.isAutoplayEnabled) return;
      if (this.playlistContext) {
        void this.playNext();
        return;
      }
      if (this.queue.length > 0 && this.currentIndex + 1 < this.queue.length) {
        void this.playNext();
      }
    });

    video.addEventListener('error', () => {
      this.isLoading = false;
      this.isPlaying = false;
      const mediaError = video.error;
      if (mediaError?.code === 4) {
        this.errorMessage = t('player.unsupported_format');
      } else {
        this.errorMessage = t('player.playback_failed');
      }
    });

    window.addEventListener('beforeunload', () => {
      void this.saveProgress();
      void this.recordPlaybackEngagement(false, true);
    });

    if (this.pendingMedia) {
      const media = this.pendingMedia;
      void this.playMedia(media, this.isExpanded);
    }
  }

  async playMedia(media: MediaItem, expand = true, updateUrl = true) {
    // A UI do player pode terminar de montar depois do clique. Nesse caso a mídia
    // já é a atual, mas ainda não ganhou um `src`; consome a pendência normalmente.
    if (this.currentMedia?.id === media.id && this.pendingMedia?.id === media.id && this.videoElement) {
      this.pendingMedia = null;
      this.currentMedia = null;
      await this.playMedia(media, expand, updateUrl);
      return;
    }

    // Se clicar no mesmo vídeo que já está carregado
    if (this.currentMedia?.id === media.id) {
      if (expand) {
        this.isExpanded = true;
        if (updateUrl) this.openVideoUrl(media.id);
      }
      if (this.videoElement && this.videoElement.paused) {
        await this.videoElement.play();
      }
      return;
    }

    // 1. Salvar progresso do vídeo anterior (enquanto this.currentMedia ainda é o vídeo anterior!)
    if (this.currentMedia && this.videoElement) {
      await this.recordPlaybackEngagement();
      await this.saveProgress();
      this.stopSyncTimer();
      this.videoElement.pause();
    }

    // 2. Atualizar para a nova mídia e resetar relógio
    this.currentMedia = media;
    this.resetInternalScrobbleSession();
    this.beginEngagementSession();
    musicStore.resetPlaybackSession();
    void musicStore.loadTrackForMedia(media.id);
    this.availableSubtitles = [];
    this.activeCueText = null;
    void this.loadSubtitles(media.id);
    if (expand) {
      this.isExpanded = true;
      if (updateUrl) this.openVideoUrl(media.id);
    }
    this.errorMessage = null;
    this.isLoading = true;
    this.currentTime = 0;
    this.lastSavedPositionMs = 0;
    this.videoFit = 'contain';
    this.duration = media.duration_ms ? media.duration_ms / 1000 : 0;

    if (!this.videoElement) {
      this.pendingMedia = media;
      return;
    }

    // 3. Obter estado salvo da nova mídia (para retomar onde ela parou ou começar do zero)
    let resumeTimeSec = 0;
    try {
      const res = await apiFetch(`/api/playback/${media.id}`);
      if (res.ok) {
        const state = (await res.json()) as PlaybackState;
        this.playbackMap[media.id] = {
          position_ms: state.position_ms,
          duration_ms: state.duration_ms,
          completed: state.completed
        };
        if (state.position_ms > 3000 && !state.completed) {
          resumeTimeSec = state.position_ms / 1000;
          this.lastSavedPositionMs = state.position_ms;
        }
      }
    } catch {
      // Ignorar falha de rede ao buscar progresso
    }

    const streamUrl = `/api/media/${media.id}/stream`;
    this.videoElement.src = streamUrl;
    this.videoElement.playbackRate = this.playbackRate;
    this.videoElement.volume = this.volume;
    this.videoElement.muted = this.isMuted;

    if (resumeTimeSec > 0) {
      const applyResume = () => {
        if (this.videoElement) {
          this.videoElement.currentTime = resumeTimeSec;
        }
      };
      if (this.videoElement.readyState >= 1) {
        applyResume();
      } else {
        this.videoElement.addEventListener('loadedmetadata', applyResume, { once: true });
      }
    }

    try {
      await this.videoElement.play();
      this.isPlaying = true;
      this.errorMessage = null;
    } catch (error) {
      const errName = (error as { name?: string })?.name;
      if (errName === 'NotAllowedError') {
        // Autoplay bloqueado pela política do navegador na nova guia (sem gesto prévio do usuário nesta aba).
        // O vídeo permanece carregado e pausado, pronto para tocar no primeiro clique, sem exibir falso erro.
        this.isPlaying = false;
        this.errorMessage = null;
      } else if (errName === 'AbortError') {
        // Ignora aborto por nova chamada
      } else {
        this.errorMessage = t('player.start_failed');
      }
    } finally {
      this.isLoading = false;
    }
  }

  togglePlay() {
    if (!this.videoElement || !this.currentMedia) return;
    if (this.videoElement.paused) {
      this.videoElement
        .play()
        .then(() => {
          this.isPlaying = true;
          this.errorMessage = null;
        })
        .catch((err) => {
          const errName = (err as { name?: string })?.name;
          if (errName !== 'AbortError' && errName !== 'NotAllowedError') {
            this.errorMessage = t('player.start_failed');
          }
        });
    } else {
      this.videoElement.pause();
      this.isPlaying = false;
    }
  }

  pauseForExclusivePlayback() {
    if (this.videoElement && !this.videoElement.paused) {
      this.videoElement.pause();
    }
    this.isPlaying = false;
    void this.saveProgress();
  }

  seek(seconds: number) {
    if (!this.videoElement) return;
    const clamped = Math.max(0, Math.min(seconds, this.duration || Infinity));
    this.videoElement.currentTime = clamped;
    this.currentTime = clamped;
    void this.saveProgress();
  }

  seekRelative(deltaSeconds: number) {
    if (!this.videoElement) return;
    this.seek(this.videoElement.currentTime + deltaSeconds);
  }

  setVolume(vol: number) {
    const clamped = Math.max(0, Math.min(vol, 1));
    this.volume = clamped;
    try { localStorage.setItem(STORAGE_VOLUME_KEY, String(clamped)); } catch {}
    if (this.videoElement) {
      this.videoElement.volume = clamped;
    }
    if (clamped > 0 && this.isMuted) {
      this.isMuted = false;
      try { localStorage.setItem(STORAGE_MUTED_KEY, 'false'); } catch {}
      if (this.videoElement) {
        this.videoElement.muted = false;
      }
    }
  }

  toggleMute() {
    this.isMuted = !this.isMuted;
    try { localStorage.setItem(STORAGE_MUTED_KEY, String(this.isMuted)); } catch {}
    if (this.videoElement) {
      this.videoElement.muted = this.isMuted;
    }
  }

  setSpeed(rate: number) {
    this.playbackRate = rate;
    try { localStorage.setItem(STORAGE_RATE_KEY, String(rate)); } catch {}
    if (this.videoElement) {
      this.videoElement.playbackRate = rate;
    }
  }

  toggleAutoplay() {
    this.isAutoplayEnabled = !this.isAutoplayEnabled;
    try { localStorage.setItem(STORAGE_AUTOPLAY_KEY, String(this.isAutoplayEnabled)); } catch {}
  }

  toggleLoop() {
    this.isLooping = !this.isLooping;
    if (this.videoElement) {
      this.videoElement.loop = this.isLooping;
    }
  }

  toggleFit() {
    this.videoFit = this.videoFit === 'contain' ? 'cover' : 'contain';
  }

  openExpanded(updateUrl = true) {
    this.isExpanded = true;
    if (updateUrl && this.currentMedia) {
      this.openVideoUrl(this.currentMedia.id);
    }
  }

  toggleExpanded() {
    this.isExpanded = !this.isExpanded;
    if (this.isExpanded && this.currentMedia) {
      this.openVideoUrl(this.currentMedia.id);
    } else {
      this.restorePreviousUrl();
    }
  }

  closeExpanded(updateUrl = true) {
    this.isExpanded = false;
    if (updateUrl) {
      this.restorePreviousUrl();
    }
  }

  async stop(updateUrl = true) {
    // 1. Salva o progresso ANTES de descarregar o elemento de vídeo
    await this.recordPlaybackEngagement();
    await this.saveProgress();
    this.stopSyncTimer();

    // 2. Descarrega o elemento de vídeo
    if (this.videoElement) {
      this.videoElement.pause();
      this.videoElement.removeAttribute('src');
      this.videoElement.load();
    }
    this.currentMedia = null;
    this.engagementSessionId = null;
    this.resetInternalScrobbleSession();
    musicStore.resetPlaybackSession();
    this.availableSubtitles = [];
    this.activeCueText = null;
    this.isPlaying = false;
    this.isExpanded = false;
    this.currentTime = 0;
    this.duration = 0;
    this.videoFit = 'contain';
    this.errorMessage = null;
    this.playlistContext = null;
    this.isPlaylistExpanded = true;
    if (updateUrl) {
      this.restorePreviousUrl();
    }
  }

  async playPlaylist(
    playlist: { id?: string; title: string; author?: string | null; items: MediaItem[] },
    startIndex = 0
  ) {
    if (!playlist.items.length) return;
    this.playlistContext = {
      id: playlist.id,
      title: playlist.title,
      author: playlist.author ?? null,
      items: [...playlist.items],
      isLooping: false,
      isShuffled: false
    };
    this.isPlaylistExpanded = true;
    this.queue = [...playlist.items];
    const validIndex = Math.max(0, Math.min(startIndex, playlist.items.length - 1));
    this.currentIndex = validIndex;
    await this.playMedia(playlist.items[validIndex], true);
  }

  async playFromPlaylist(index: number) {
    if (!this.playlistContext || index < 0 || index >= this.playlistContext.items.length) return;
    this.currentIndex = index;
    await this.playMedia(this.playlistContext.items[index], false);
  }

  togglePlaylistLoop() {
    if (!this.playlistContext) return;
    this.playlistContext.isLooping = !this.playlistContext.isLooping;
  }

  togglePlaylistShuffle() {
    if (!this.playlistContext) return;
    this.playlistContext.isShuffled = !this.playlistContext.isShuffled;
  }

  togglePlaylistExpand() {
    this.isPlaylistExpanded = !this.isPlaylistExpanded;
  }

  async playQueue(items: MediaItem[], startIndex = 0) {
    if (!items.length) return;
    this.playlistContext = null;
    this.queue = items;
    this.currentIndex = Math.max(0, Math.min(startIndex, items.length - 1));
    await this.playMedia(items[this.currentIndex], true);
  }

  async playNext() {
    if (this.playlistContext) {
      const len = this.playlistContext.items.length;
      if (len === 0) return;
      let nextIndex: number;
      if (this.playlistContext.isShuffled && len > 1) {
        do {
          nextIndex = Math.floor(Math.random() * len);
        } while (nextIndex === this.currentIndex);
      } else {
        nextIndex = this.currentIndex + 1;
        if (nextIndex >= len) {
          if (this.playlistContext.isLooping) {
            nextIndex = 0;
          } else {
            return;
          }
        }
      }
      this.currentIndex = nextIndex;
      await this.playMedia(this.playlistContext.items[nextIndex], false);
      return;
    }

    if (this.queue.length === 0 || this.currentIndex + 1 >= this.queue.length) return;
    this.currentIndex += 1;
    await this.playMedia(this.queue[this.currentIndex], false);
  }

  async playPrevious() {
    if (this.playlistContext) {
      const len = this.playlistContext.items.length;
      if (len === 0) return;
      let prevIndex = this.currentIndex - 1;
      if (prevIndex < 0) {
        if (this.playlistContext.isLooping) {
          prevIndex = len - 1;
        } else {
          prevIndex = 0;
        }
      }
      this.currentIndex = prevIndex;
      await this.playMedia(this.playlistContext.items[prevIndex], false);
      return;
    }

    if (this.queue.length === 0 || this.currentIndex <= 0) return;
    this.currentIndex -= 1;
    await this.playMedia(this.queue[this.currentIndex], false);
  }

  async toggleFavorite(targetMedia?: MediaItem) {
    const item = targetMedia ?? this.currentMedia;
    if (!item) return;

    const previous = !!item.is_favorite;
    const next = !previous;

    // Atualização otimista imediata
    item.is_favorite = next;
    if (this.currentMedia && this.currentMedia.id === item.id) {
      this.currentMedia = { ...this.currentMedia, is_favorite: next };
    }

    try {
      const res = await apiFetch(`/api/favorites/${item.id}/toggle`, { method: 'POST' });
      if (res.ok) {
        const data = (await res.json()) as { is_favorite: boolean };
        item.is_favorite = data.is_favorite;
        if (this.currentMedia && this.currentMedia.id === item.id) {
          this.currentMedia = { ...this.currentMedia, is_favorite: data.is_favorite };
        }
      } else {
        item.is_favorite = previous;
        if (this.currentMedia && this.currentMedia.id === item.id) {
          this.currentMedia = { ...this.currentMedia, is_favorite: previous };
        }
      }
    } catch {
      item.is_favorite = previous;
      if (this.currentMedia && this.currentMedia.id === item.id) {
        this.currentMedia = { ...this.currentMedia, is_favorite: previous };
      }
    }
  }

  updateCurrentMediaNotes(notes: string | null) {
    if (!this.currentMedia) return;
    this.currentMedia = { ...this.currentMedia, user_notes: notes };

    const inQueue = this.queue.find((it) => it.id === this.currentMedia!.id);
    if (inQueue) {
      inQueue.user_notes = notes;
    }
    const inPlaylist = this.playlistContext?.items.find((it) => it.id === this.currentMedia!.id);
    if (inPlaylist) {
      inPlaylist.user_notes = notes;
    }
  }

  async loadSubtitles(mediaId: number) {
    try {
      const res = await apiFetch(`/api/media/${mediaId}/subtitles`);
      if (res.ok) {
        const subs = (await res.json()) as SubtitleTrack[];
        this.availableSubtitles = subs;
        this.syncActiveSubtitleTrack();
      } else {
        this.availableSubtitles = [];
      }
    } catch {
      this.availableSubtitles = [];
    }
  }

  setSubtitleLang(lang: string | null) {
    this.selectedSubtitleLang = lang;
    try {
      if (lang) {
        localStorage.setItem(STORAGE_SUBTITLE_LANG_KEY, lang);
      } else {
        localStorage.removeItem(STORAGE_SUBTITLE_LANG_KEY);
      }
    } catch {}
    this.syncActiveSubtitleTrack();
  }

  syncActiveSubtitleTrack() {
    if (!this.videoElement) return;
    const lang = this.selectedSubtitleLang;
    const isPip = typeof document !== 'undefined' && document.pictureInPictureElement === this.videoElement;

    for (let i = 0; i < this.videoElement.textTracks.length; i++) {
      const track = this.videoElement.textTracks[i];
      if (lang && (track.language === lang || track.language.startsWith(lang))) {
        // Modo 'hidden' permite que cues sejam processadas e disparadas sem o renderizador nativo colidir com nosso overlay
        track.mode = isPip ? 'showing' : 'hidden';
        track.oncuechange = () => {
          this.updateActiveCues();
        };
      } else {
        track.mode = 'disabled';
        track.oncuechange = null;
      }
    }
    this.updateActiveCues();
  }

  updateActiveCues() {
    if (!this.videoElement || !this.selectedSubtitleLang) {
      if (this.activeCueText !== null) this.activeCueText = null;
      return;
    }
    const lang = this.selectedSubtitleLang;
    for (let i = 0; i < this.videoElement.textTracks.length; i++) {
      const track = this.videoElement.textTracks[i];
      if (lang && (track.language === lang || track.language.startsWith(lang))) {
        if (track.activeCues && track.activeCues.length > 0) {
          const joined = Array.from(track.activeCues)
            .map((c) => (c as VTTCue).text)
            .filter(Boolean)
            .join('\n');
          if (this.activeCueText !== joined) {
            this.activeCueText = joined;
          }
          return;
        }
      }
    }
    if (this.activeCueText !== null) {
      this.activeCueText = null;
    }
  }

  async downloadSubtitles(mediaId: number, languages?: string[]): Promise<SubtitleTrack[]> {
    this.isDownloadingSubtitles = true;
    try {
      const res = await apiFetch(`/api/media/${mediaId}/subtitles/download`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ languages: languages ?? null })
      });
      if (res.ok) {
        const subs = (await res.json()) as SubtitleTrack[];
        this.availableSubtitles = subs;
        if (!this.selectedSubtitleLang && subs.length > 0) {
          const pt = subs.find((s) => s.lang_code.startsWith('pt'));
          this.setSubtitleLang(pt ? pt.lang_code : subs[0].lang_code);
        } else {
          this.syncActiveSubtitleTrack();
        }
        return subs;
      }
    } catch (err) {
      console.error('Failed to download subtitles:', err);
    } finally {
      this.isDownloadingSubtitles = false;
    }
    return [];
  }

  private startSyncTimer() {
    this.stopSyncTimer();
    this.syncInterval = window.setInterval(() => {
      void this.saveProgress();
      void this.recordPlaybackEngagement();
    }, 5000);
  }

  private stopSyncTimer() {
    if (this.syncInterval !== null) {
      clearInterval(this.syncInterval);
      this.syncInterval = null;
    }
  }

  async saveProgress() {
    if (!this.currentMedia) return;
    const rawSeconds =
      this.videoElement && !isNaN(this.videoElement.currentTime) && this.videoElement.currentTime > 0
        ? this.videoElement.currentTime
        : this.currentTime;
    const currentMs = Math.floor(rawSeconds * 1000);
    const durationMs = Math.floor((this.videoElement?.duration || this.duration || 0) * 1000);
    const completed = durationMs > 0 && currentMs >= durationMs * 0.9;

    // Regra: só grava no histórico se assistiu ao menos 5 segundos (5000ms),
    // se completou o vídeo, ou se o vídeo já tinha histórico prévio salvo.
    if (currentMs < 5000 && !completed && this.lastSavedPositionMs < 5000) {
      return;
    }

    this.playbackMap[this.currentMedia.id] = {
      position_ms: currentMs,
      duration_ms: durationMs > 0 ? durationMs : null,
      completed
    };
    this.currentMedia.playback = {
      position_ms: currentMs,
      duration_ms: durationMs > 0 ? durationMs : null,
      completed
    };

    // Evitar gravações redundantes se a posição variou menos de 1 segundo
    if (Math.abs(currentMs - this.lastSavedPositionMs) < 1000) return;
    this.lastSavedPositionMs = currentMs;

    try {
      await apiFetch(`/api/playback/${this.currentMedia.id}`, {
        method: 'PUT',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          position_ms: currentMs,
          duration_ms: durationMs > 0 ? durationMs : null
        })
      });
    } catch {
      // Ignorar falha pontual de envio de progresso
    }
  }
}

export const player = new PlayerStore();

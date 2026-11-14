import { apiFetch } from './api';
import type { ListenBrainzSettings, TokenValidationResult } from './settings/types';

export type MediaMusicTrack = {
  media_id: number;
  recording_mbid: string;
  artist_name: string;
  track_name: string;
  release_name?: string | null;
  source: string;
  created_at: string;
  updated_at: string;
  last_scrobbled_at?: string | null;
};

export type MusicCandidate = {
  recording_mbid: string;
  track_name: string;
  artist_name: string;
  release_name?: string | null;
  duration_secs?: number | null;
  score: number;
  source: string;
  disambiguation?: string | null;
  release_date?: string | null;
  release_type?: string | null;
};

class MusicStore {
  settings = $state<ListenBrainzSettings>({
    enabled: false,
    user_token: '',
    username: null,
    auto_scrobble: true,
    has_token: false
  });

  loaded = $state(false);
  currentTrack = $state<MediaMusicTrack | null>(null);
  currentMediaId = $state<number | null>(null);

  // Scrobble tracking
  private accumulatedWatchMs = 0;
  private lastPositionMs = 0;
  private hasScrobbledCurrentSession = false;

  async init() {
    if (this.loaded) return;
    await this.loadSettings();
  }

  async loadSettings(): Promise<ListenBrainzSettings> {
    try {
      const res = await apiFetch('/api/settings/listenbrainz');
      if (res.ok) {
        this.settings = (await res.json()) as ListenBrainzSettings;
      }
    } catch {
      // Ignora erro inicial de carregamento
    } finally {
      this.loaded = true;
    }
    return this.settings;
  }

  async saveSettings(newSettings: ListenBrainzSettings): Promise<ListenBrainzSettings> {
    const res = await apiFetch('/api/settings/listenbrainz', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(newSettings)
    });
    if (!res.ok) {
      throw new Error(`Erro ao salvar configurações (${res.status})`);
    }
    this.settings = (await res.json()) as ListenBrainzSettings;
    return this.settings;
  }

  async validateToken(token: string): Promise<TokenValidationResult> {
    const res = await apiFetch('/api/settings/listenbrainz/validate', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token })
    });
    return (await res.json()) as TokenValidationResult;
  }

  async loadTrackForMedia(mediaId: number): Promise<MediaMusicTrack | null> {
    if (!this.loaded) {
      await this.loadSettings();
    }
    if (this.currentMediaId === mediaId && this.currentTrack) {
      return this.currentTrack;
    }
    this.currentMediaId = mediaId;
    this.currentTrack = null;
    this.resetPlaybackSession();

    if (!this.settings.enabled) {
      return null;
    }

    try {
      const res = await apiFetch(`/api/media/${mediaId}/music`);
      if (res.ok) {
        this.currentTrack = (await res.json()) as MediaMusicTrack | null;
        if (this.currentTrack) {
          void this.sendPlayingNow(mediaId);
        }
      }
    } catch {
      this.currentTrack = null;
    }
    return this.currentTrack;
  }

  async recognizeMusic(mediaId: number): Promise<MusicCandidate[]> {
    const res = await apiFetch(`/api/media/${mediaId}/music/recognize`, {
      method: 'POST'
    });
    if (!res.ok) {
      let errMsg = `Falha no reconhecimento (${res.status})`;
      try {
        const body = await res.json();
        if (body?.error) errMsg = body.error;
      } catch {}
      throw new Error(errMsg);
    }
    return (await res.json()) as MusicCandidate[];
  }

  async searchMusic(query: string): Promise<MusicCandidate[]> {
    const clean = query.trim();
    if (!clean) return [];
    const res = await apiFetch(`/api/music/search?q=${encodeURIComponent(clean)}`);
    if (!res.ok) {
      let errMsg = `Falha na busca (${res.status})`;
      try {
        const body = await res.json();
        if (body?.error) errMsg = body.error;
      } catch {}
      throw new Error(errMsg);
    }
    return (await res.json()) as MusicCandidate[];
  }

  async lookupMbid(id: string): Promise<MusicCandidate | null> {
    const res = await apiFetch(`/api/music/lookup-mbid?id=${encodeURIComponent(id)}`);
    if (!res.ok) {
      let errMsg = `Falha na consulta (${res.status})`;
      try {
        const body = await res.json();
        if (body?.error) errMsg = body.error;
      } catch {}
      throw new Error(errMsg);
    }
    return (await res.json()) as MusicCandidate | null;
  }

  async associateMusic(
    mediaId: number,
    data: {
      recording_mbid?: string;
      artist_name: string;
      track_name: string;
      release_name?: string | null;
      source?: string;
    }
  ): Promise<MediaMusicTrack> {
    const res = await apiFetch(`/api/media/${mediaId}/music`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        recording_mbid: data.recording_mbid || '',
        artist_name: data.artist_name,
        track_name: data.track_name,
        release_name: data.release_name || null,
        source: data.source || 'manual'
      })
    });
    if (!res.ok) {
      throw new Error(`Falha ao vincular música (${res.status})`);
    }
    const saved = (await res.json()) as MediaMusicTrack;
    if (this.currentMediaId === mediaId) {
      this.currentTrack = saved;
    }
    return saved;
  }

  async deleteMusic(mediaId: number): Promise<void> {
    const res = await apiFetch(`/api/media/${mediaId}/music`, {
      method: 'DELETE'
    });
    if (!res.ok) {
      throw new Error(`Falha ao desvincular música (${res.status})`);
    }
    if (this.currentMediaId === mediaId) {
      this.currentTrack = null;
    }
  }

  async scrobble(mediaId: number): Promise<{ success: boolean; scrobbled_at: string; message: string }> {
    const res = await apiFetch(`/api/media/${mediaId}/music/scrobble`, {
      method: 'POST'
    });
    const data = await res.json();
    if (!res.ok) {
      throw new Error(data.message || `Erro no scrobble (${res.status})`);
    }
    if (this.currentTrack && this.currentMediaId === mediaId) {
      this.currentTrack.last_scrobbled_at = data.scrobbled_at;
    }
    return data;
  }

  private async sendPlayingNow(mediaId: number) {
    if (!this.settings.enabled || !this.settings.has_token) return;
    try {
      await apiFetch(`/api/media/${mediaId}/music/playing-now`, { method: 'POST' });
    } catch {}
  }

  resetPlaybackSession() {
    this.accumulatedWatchMs = 0;
    this.lastPositionMs = 0;
    this.hasScrobbledCurrentSession = false;
  }

  onPlaybackTick(mediaId: number, currentPositionMs: number, durationMs: number | null) {
    if (!this.settings.enabled || !this.settings.auto_scrobble) return;
    if (!this.currentTrack || this.currentMediaId !== mediaId) return;
    if (!durationMs || durationMs <= 0) return;

    // Detect loop or restart: if position jumped back to near start after playing past 40%
    if (this.lastPositionMs > durationMs * 0.4 && currentPositionMs < durationMs * 0.2) {
      this.resetPlaybackSession();
      void this.sendPlayingNow(mediaId);
    }

    if (this.hasScrobbledCurrentSession) {
      this.lastPositionMs = currentPositionMs;
      return;
    }

    if (this.lastPositionMs > 0 && currentPositionMs > this.lastPositionMs) {
      const delta = currentPositionMs - this.lastPositionMs;
      // Aceita delta razoável entre ticks (até 10s para acomodar abas em segundo plano no navegador)
      if (delta > 0 && delta <= 10000) {
        this.accumulatedWatchMs += delta;
      }
    }
    this.lastPositionMs = currentPositionMs;

    // Regra padrão ListenBrainz / Last.fm: 50% da música ou 4 minutos de reprodução
    const requiredWatchMs = Math.min(durationMs * 0.5, 4 * 60 * 1000);
    if (this.accumulatedWatchMs >= requiredWatchMs) {
      this.hasScrobbledCurrentSession = true;
      void this.scrobble(mediaId);
    }
  }
}

export const musicStore = new MusicStore();

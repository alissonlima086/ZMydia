<script lang="ts">
  import type { MediaMusicTrack } from '../music.svelte';
  import { t } from '../i18n/i18n.svelte';

  let {
    track,
    busy = false,
    onScrobble,
    onUnlink
  }: {
    track: MediaMusicTrack;
    busy?: boolean;
    onScrobble: () => void;
    onUnlink: () => void;
  } = $props();

  function formatScrobbleDate(value?: string | null): string {
    if (!value) return '';
    const numeric = Number(value);
    if (!Number.isNaN(numeric) && numeric > 0) {
      const milliseconds = numeric < 10_000_000_000 ? numeric * 1000 : numeric;
      return new Date(milliseconds).toLocaleString();
    }
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
  }
</script>

<div class="current-track-card">
  <div class="current-track-badge">
    <span class="material-symbols-outlined">verified</span>
    <span>{t('music.currently_linked')}</span>
  </div>
  <div class="current-track-info">
    <div class="current-track-title-row">
      <span class="track-title">{track.track_name}</span>
      {#if track.recording_mbid}
        <a
          href="https://listenbrainz.org/track/{track.recording_mbid}"
          target="_blank"
          rel="noopener noreferrer"
          class="candidate-lb-link"
          title={t('music.view_on_listenbrainz')}
        >
          <span>ListenBrainz</span>
          <span class="material-symbols-outlined external-link-icon">open_in_new</span>
        </a>
      {/if}
    </div>
    <span class="track-artist">{track.artist_name}</span>
    {#if track.release_name}<span class="track-release">Álbum: {track.release_name}</span>{/if}
    {#if track.last_scrobbled_at}
      <span class="scrobble-date">Último scrobble: {formatScrobbleDate(track.last_scrobbled_at)}</span>
    {/if}
  </div>
  <div class="current-track-actions">
    <button type="button" class="action-btn scrobble-now-btn" disabled={busy} onclick={onScrobble} title="Registrar agora na sua conta do ListenBrainz">
      <span class="material-symbols-outlined">send</span>
      <span>{t('music.scrobble_now')}</span>
    </button>
    <button type="button" class="action-btn unlink-btn" disabled={busy} onclick={onUnlink} title="Desvincular esta música">
      <span class="material-symbols-outlined">link_off</span>
      <span>{t('music.unlink')}</span>
    </button>
  </div>
</div>

<style>
  .current-track-card { background: rgba(var(--accent-rgb), .08); border: 1px solid rgba(var(--accent-rgb), .3); border-radius: 12px; padding: 16px; display: flex; flex-direction: column; gap: 10px; }
  .current-track-badge { display: flex; align-items: center; gap: 6px; font-size: 12px; font-weight: 600; color: var(--accent); text-transform: uppercase; letter-spacing: .5px; }
  .current-track-badge .material-symbols-outlined { font-size: 16px; }
  .current-track-info { display: flex; flex-direction: column; gap: 2px; }
  .current-track-title-row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .track-title { font-size: 16px; font-weight: 600; color: var(--text-primary); }
  .track-artist { font-size: 14px; color: var(--accent); font-weight: 500; }
  .track-release, .scrobble-date { font-size: 12px; color: var(--text-muted); }
  .current-track-actions { display: flex; gap: 10px; margin-top: 4px; }
  .action-btn { display: inline-flex; align-items: center; gap: 6px; padding: 7px 12px; border-radius: 6px; font-size: 12px; font-weight: 500; cursor: pointer; border: none; transition: all .15s; }
  .action-btn:disabled { cursor: wait; opacity: .6; }
  .scrobble-now-btn { background: var(--accent); color: #fff; }
  .scrobble-now-btn:hover:not(:disabled) { background: var(--accent-hover); }
  .unlink-btn { background: rgba(239, 68, 68, .15); color: #ef4444; border: 1px solid rgba(239, 68, 68, .3); }
  .unlink-btn:hover:not(:disabled) { background: rgba(239, 68, 68, .25); }
  .candidate-lb-link { display: inline-flex; align-items: center; gap: 3px; font-size: 11px; font-weight: 500; color: var(--accent); background: rgba(var(--accent-rgb), .1); border: 1px solid rgba(var(--accent-rgb), .25); border-radius: 4px; padding: 1px 6px; text-decoration: none; flex-shrink: 0; transition: all .15s ease; }
  .candidate-lb-link:hover { background: rgba(var(--accent-rgb), .22); color: #fff; border-color: var(--accent); }
  .external-link-icon { font-size: 12px; }
</style>

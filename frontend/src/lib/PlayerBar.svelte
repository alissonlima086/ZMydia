<script lang="ts">
  import { player } from './player.svelte';
  import { t } from './i18n/i18n.svelte';

  function formatTime(seconds: number): string {
    if (!seconds || isNaN(seconds) || seconds <= 0) return '0:00';
    const total = Math.floor(seconds);
    const m = Math.floor(total / 60);
    const s = total % 60;
    const pad = (n: number) => n.toString().padStart(2, '0');
    return `${m}:${pad(s)}`;
  }

  const progressPercent = $derived(
    player.duration > 0 ? Math.min(100, (player.currentTime / player.duration) * 100) : 0
  );
</script>

<aside class="player-bar" aria-label={t('player_bar.global_player_aria')} class:active={player.currentMedia !== null}>
  <button
    type="button"
    class="player-cover-btn"
    onclick={() => player.toggleExpanded()}
    title={player.currentMedia ? t('player_bar.expand') : ''}
    disabled={player.currentMedia === null}
  >
    <div class="player-cover">
      {#if player.currentMedia}
        <img src={player.currentMedia.thumbnail_url} alt="" class="player-cover-thumb" />
      {:else}
        <span class="material-symbols-outlined icon-20">play_arrow</span>
      {/if}
    </div>
  </button>

  <div
    class="player-copy-clickable"
    onclick={() => player.currentMedia && player.toggleExpanded()}
    role="button"
    tabindex="0"
    onkeydown={(e) => e.key === 'Enter' && player.currentMedia && player.toggleExpanded()}
  >
    <div class="player-copy">
      <strong>{player.currentMedia?.title ?? t('player.no_media_selected')}</strong>
      <span>
        {#if player.currentMedia}
          {#if player.currentMedia.channel?.name}
            <span class="material-symbols-outlined icon-13 icon-inline icon-gap-after">subscriptions</span>
            {player.currentMedia.channel.name} • 
          {/if}
          {formatTime(player.currentTime)} / {formatTime(player.duration)}
        {:else}
          {t('player.choose_item_to_start')}
        {/if}
      </span>
    </div>
  </div>

  <div class="player-bar-actions">
    <button
      class="player-bar-btn"
      type="button"
      onclick={() => player.toggleMute()}
      disabled={player.currentMedia === null}
      title={player.isMuted ? t('player.unmute') : t('player.mute')}
    >
      <span class="material-symbols-outlined">
        {player.isMuted || player.volume === 0 ? 'volume_off' : 'volume_up'}
      </span>
    </button>

    <button
      class="player-button"
      type="button"
      aria-label={player.isPlaying ? t('player.pause') : t('player.play')}
      onclick={() => player.togglePlay()}
      disabled={player.currentMedia === null}
    >
      <span class="material-symbols-outlined icon-24">
        {player.isPlaying ? 'pause' : 'play_arrow'}
      </span>
    </button>

    <button
      class="player-bar-btn expand-btn"
      type="button"
      onclick={() => player.toggleExpanded()}
      disabled={player.currentMedia === null}
      title={t('player_bar.expand')}
    >
      <span class="material-symbols-outlined">open_in_full</span>
    </button>

    <button
      class="player-bar-btn close-player-btn"
      type="button"
      onclick={() => void player.stop()}
      disabled={player.currentMedia === null}
      title={t('player_bar.close_stop')}
    >
      <span class="material-symbols-outlined">close</span>
    </button>
  </div>

  <!-- Barra de progresso inferior interativa -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="player-progress"
    aria-label={t('player_bar.progress_aria')}
    onclick={(e) => {
      if (!player.duration) return;
      const rect = e.currentTarget.getBoundingClientRect();
      const clickX = e.clientX - rect.left;
      const pct = Math.max(0, Math.min(1, clickX / rect.width));
      player.seek(pct * player.duration);
    }}
  >
    <div class="player-progress-fill" style="width: {progressPercent}%"></div>
  </div>
</aside>

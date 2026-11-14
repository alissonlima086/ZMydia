<script lang="ts">
  import type { PlaylistContext } from './player.svelte';
  import { t } from './i18n/i18n.svelte';

  interface Props {
    playlist: PlaylistContext;
    currentIndex: number;
    isExpanded: boolean;
    onToggleExpand: () => void;
    onSelectVideo: (index: number) => void;
    onToggleLoop: () => void;
    onToggleShuffle: () => void;
  }

  let {
    playlist,
    currentIndex,
    isExpanded,
    onToggleExpand,
    onSelectVideo,
    onToggleLoop,
    onToggleShuffle
  }: Props = $props();

  let listEl = $state<HTMLElement | null>(null);

  // Calcula o próximo item para exibir no modo recolhido (collapsed)
  let nextItem = $derived.by(() => {
    if (!playlist.items || playlist.items.length === 0) return null;
    const nextIdx = currentIndex + 1;
    if (nextIdx < playlist.items.length) {
      return playlist.items[nextIdx];
    }
    if (playlist.isLooping && playlist.items.length > 0) {
      return playlist.items[0];
    }
    return null;
  });

  // Mantém o vídeo atual da playlist visível na lista quando expandido
  $effect(() => {
    if (isExpanded && listEl && currentIndex >= 0) {
      const activeEl = listEl.querySelector(`#playlist-item-${currentIndex}`);
      if (activeEl) {
        activeEl.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
      }
    }
  });

  function formatDuration(ms: number | null): string {
    if (!ms || ms <= 0) return '';
    const totalSecs = Math.floor(ms / 1000);
    const hours = Math.floor(totalSecs / 3600);
    const minutes = Math.floor((totalSecs % 3600) / 60);
    const seconds = totalSecs % 60;
    const pad = (n: number) => n.toString().padStart(2, '0');
    if (hours > 0) return `${hours}:${pad(minutes)}:${pad(seconds)}`;
    return `${minutes}:${pad(seconds)}`;
  }
</script>

{#if isExpanded}
  <div class="playlist-panel">
    <!-- Cabeçalho da Playlist Expandida -->
    <div class="playlist-panel-header">
      <div class="playlist-header-top">
        <div class="playlist-title-col">
          <h3 class="playlist-panel-title" title={playlist.title}>{playlist.title}</h3>
          <span class="playlist-panel-sub">
            {#if playlist.author}
              <span>{playlist.author} · </span>
            {/if}
            <span>{currentIndex + 1} / {playlist.items.length}</span>
          </span>
        </div>
        <button
          type="button"
          class="playlist-close-btn"
          onclick={onToggleExpand}
          title={t('playlist_panel.hide')}
          aria-label={t('playlist_panel.hide')}
        >
          <span class="material-symbols-outlined">close</span>
        </button>
      </div>

      <!-- Controles de Repetição e Shuffle -->
      <div class="playlist-header-actions">
        <button
          type="button"
          class="playlist-action-btn"
          class:active={playlist.isLooping}
          onclick={onToggleLoop}
          title={t('playlist_panel.loop_toggle')}
        >
          <span class="material-symbols-outlined">repeat</span>
        </button>

        <button
          type="button"
          class="playlist-action-btn"
          class:active={playlist.isShuffled}
          onclick={onToggleShuffle}
          title={t('playlist_panel.shuffle_toggle')}
        >
          <span class="material-symbols-outlined">shuffle</span>
        </button>
      </div>
    </div>

    <!-- Lista de Vídeos da Playlist -->
    <div bind:this={listEl} class="playlist-items-scroll">
      {#each playlist.items as item, index (item.id)}
        {@const isActive = index === currentIndex}
        <div
          id="playlist-item-{index}"
          class="playlist-item-row"
          class:active={isActive}
          onclick={() => onSelectVideo(index)}
          role="button"
          tabindex="0"
          onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && onSelectVideo(index)}
        >
          <!-- Índice ou Ícone de Play -->
          <div class="playlist-item-prefix">
            {#if isActive}
              <span class="material-symbols-outlined play-indicator">play_arrow</span>
            {:else}
              <span class="item-index-number">{index + 1}</span>
            {/if}
          </div>

          <!-- Miniatura com Duração -->
          <div class="playlist-thumb-wrap">
            {#if item.thumbnail_url}
              <img src={item.thumbnail_url} alt="" class="playlist-thumb-img" loading={index < 8 ? 'eager' : 'lazy'} decoding={index < 6 ? 'sync' : 'async'} fetchpriority={index < 3 ? 'high' : 'auto'} />
            {:else}
              <div class="playlist-thumb-fallback">
                <span class="material-symbols-outlined">movie</span>
              </div>
            {/if}
            {#if item.duration_ms}
              <span class="playlist-thumb-duration">{formatDuration(item.duration_ms)}</span>
            {/if}
          </div>

          <!-- Detalhes do Vídeo -->
          <div class="playlist-item-info">
            <h4 class="playlist-item-title" title={item.title}>{item.title}</h4>
            {#if item.channel}
              <span class="playlist-item-channel" title={item.channel.name}>
                {item.channel.name}
              </span>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  </div>
{:else}
  <!-- Modo Recolhido / Oculto (Barra compacta fixa) -->
  <div
    class="playlist-collapsed-bar"
    role="button"
    tabindex="0"
    onclick={onToggleExpand}
    onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && onToggleExpand()}
    title={t('playlist_panel.expand')}
  >
    <div class="collapsed-info">
      <div class="collapsed-next-title" title={nextItem ? `${t('playlist_panel.next_video')} ${nextItem.title}` : t('playlist_panel.end_of_playlist')}>
        {#if nextItem}
          <span class="collapsed-next-prefix">{t('playlist_panel.next_video')}</span> {nextItem.title}
        {:else}
          <span class="collapsed-next-prefix">{t('playlist_panel.end_of_playlist')}</span>
        {/if}
      </div>
      <div class="collapsed-sub">
        <span class="collapsed-playlist-title">{playlist.title}</span>
        <span class="collapsed-sep">·</span>
        <span class="collapsed-counter">{currentIndex + 1} / {playlist.items.length}</span>
      </div>
    </div>
    <button
      type="button"
      class="collapsed-expand-btn"
      onclick={(e) => {
        e.stopPropagation();
        onToggleExpand();
      }}
      title={t('playlist_panel.expand')}
      aria-label={t('playlist_panel.expand')}
    >
      <span class="material-symbols-outlined">expand_more</span>
    </button>
  </div>
{/if}

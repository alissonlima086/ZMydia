<script lang="ts">
  import { onMount } from 'svelte';
  import { apiFetch } from './api';
  import { player, type MediaItem } from './player.svelte';
  import SaveToPlaylistModal from './SaveToPlaylistModal.svelte';
  import { t } from './i18n/i18n.svelte';
  import Button from './ui/Button.svelte';
  import PageHeader from './ui/PageHeader.svelte';
  import PageState from './ui/PageState.svelte';

  let favorites = $state<MediaItem[]>([]);
  let loading = $state(true);
  let errorMessage = $state('');
  let playlistModalMedia = $state<MediaItem | null>(null);
  let sortOrder = $state<'recent' | 'title_asc' | 'title_desc' | 'duration_desc' | 'duration_asc'>('recent');

  async function loadFavorites() {
    loading = true;
    errorMessage = '';
    try {
      const res = await apiFetch('/api/favorites');
      if (!res.ok) {
        throw new Error(`Falha ao obter favoritos (${res.status})`);
      }
      favorites = (await res.json()) as MediaItem[];
    } catch (err) {
      errorMessage = err instanceof Error ? err.message : String(err);
    } finally {
      loading = false;
    }
  }

  const sortedFavorites = $derived.by(() => {
    const list = [...favorites];
    switch (sortOrder) {
      case 'title_asc':
        return list.sort((a, b) => a.title.localeCompare(b.title, 'pt-BR'));
      case 'title_desc':
        return list.sort((a, b) => b.title.localeCompare(a.title, 'pt-BR'));
      case 'duration_desc':
        return list.sort((a, b) => (b.duration_ms ?? 0) - (a.duration_ms ?? 0));
      case 'duration_asc':
        return list.sort((a, b) => (a.duration_ms ?? 0) - (b.duration_ms ?? 0));
      case 'recent':
      default:
        return list;
    }
  });

  function playAll() {
    if (sortedFavorites.length === 0) return;
    void playFavoriteAt(0);
  }

  function playFavoriteAt(index: number) {
    if (sortedFavorites.length === 0) return;
    void player.playPlaylist(
      {
        title: t('favorites.title'),
        author: t('nav.favorites'),
        items: sortedFavorites
      },
      index
    );
  }

  function handleMediaClick(event: MouseEvent, item: MediaItem) {
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || event.button !== 0) {
      return;
    }
    event.preventDefault();
    const index = sortedFavorites.findIndex((favorite) => favorite.id === item.id);
    if (index >= 0) playFavoriteAt(index);
  }

  function handleAuxClick(event: MouseEvent, item: MediaItem) {
    if (event.button === 1) {
      event.preventDefault();
      window.open(`/watch?v=${item.id}`, '_blank');
    }
  }

  function formatDuration(ms: number | null): string {
    if (!ms || ms <= 0) return '';
    const totalSecs = Math.floor(ms / 1000);
    const hours = Math.floor(totalSecs / 3600);
    const minutes = Math.floor((totalSecs % 3600) / 60);
    const seconds = totalSecs % 60;
    const pad = (n: number) => n.toString().padStart(2, '0');
    if (hours > 0) {
      return `${hours}:${pad(minutes)}:${pad(seconds)}`;
    }
    return `${minutes}:${pad(seconds)}`;
  }

  async function toggleFav(event: MouseEvent, item: MediaItem) {
    event.stopPropagation();
    await player.toggleFavorite(item);
    favorites = favorites.filter((fav) => fav.id !== item.id);
  }

  onMount(() => {
    void loadFavorites();
  });
</script>

<div class="favorites-view">
  <PageHeader title={t('favorites.title')} description={t('nav.favorites_desc')}>
    {#snippet meta()}<span class="favorites-meta-count">{t('favorites.count_badge', { count: favorites.length })}</span>{/snippet}
    {#snippet actions()}{#if favorites.length > 0}<Button variant="primary" onclick={playAll}><span class="material-symbols-outlined icon-18" aria-hidden="true">play_arrow</span>{t('playlists_view.play_all')}</Button><label class="favorites-sort-wrap"><span class="material-symbols-outlined sort-icon" aria-hidden="true">sort</span><select class="sort-select" bind:value={sortOrder} aria-label={t('catalog.sort_aria')}><option value="recent">{t('catalog.recent')}</option><option value="title_asc">{t('catalog.title_asc')}</option><option value="title_desc">{t('catalog.title_desc')}</option><option value="duration_desc">{t('catalog.duration_desc')}</option><option value="duration_asc">{t('catalog.duration_asc')}</option></select></label>{/if}{/snippet}
  </PageHeader>

  {#if loading}
    <PageState kind="loading" title={t('favorites.loading')} />
  {:else if errorMessage}
    {#snippet retry()}<Button onclick={() => void loadFavorites()}>{t('common.retry')}</Button>{/snippet}
    <PageState kind="error" icon="error" title={t('favorites.load_error')} description={errorMessage} actions={retry} />
  {:else if favorites.length === 0}
    {#snippet explore()}<a href="/" class="primary-action"><span class="material-symbols-outlined icon-18">home</span>{t('nav.explore')}</a>{/snippet}
    <PageState kind="empty" icon="favorite_border" title={t('favorites.empty_title')} description={t('favorites.empty_desc')} actions={explore} />
  {:else}
    <div class="catalog-grid">
      {#each sortedFavorites as item, index (item.id)}
        {@const playback = player.getPlayback(item.id) ?? item.playback}
        <div
          class="media-card"
          role="button"
          tabindex="0"
          onclick={(e) => handleMediaClick(e, item)}
          onauxclick={(e) => handleAuxClick(e, item)}
          onkeydown={(e) => e.key === 'Enter' && playFavoriteAt(index)}
        >
          <div class="thumb-container">
            <a
              href={`/watch?v=${item.id}`}
              class="thumb-link"
              onclick={(e) => handleMediaClick(e, item)}
              aria-label={item.title}
            >
              <img
                src={item.thumbnail_url}
                alt={item.title}
                loading={index < 12 ? 'eager' : 'lazy'}
                decoding={index < 8 ? 'sync' : 'async'}
                fetchpriority={index < 4 ? 'high' : 'auto'}
                class="thumb-img"
              />
            </a>

            <div class="thumb-top-left-badges">
              {#if item.is_short}
                <span class="badge short-badge">Short</span>
              {/if}
              {#if item.is_official_thumbnail}
                <span class="badge official-badge" title={t('common.official_thumbnail_tip')} aria-label={t('common.official')}>
                  <span class="material-symbols-outlined official-icon">verified</span>
                </span>
              {/if}
            </div>

            {#if playback?.completed}
              <span class="badge watched-badge" title={t('common.watched')}>
                <span class="material-symbols-outlined watched-icon">check</span>
                <span>{t('common.watched')}</span>
              </span>
            {/if}

            {#if item.duration_ms}
              <span class="badge duration-badge">
                {formatDuration(item.duration_ms)}
              </span>
            {/if}

            <div class="card-top-right-actions">
              <button
                type="button"
                class="card-action-btn card-playlist-btn"
                onclick={(e) => {
                  e.stopPropagation();
                  playlistModalMedia = item;
                }}
                title={t('common.save_to_playlist')}
                aria-label={t('common.save_to_playlist')}
              >
                <span class="material-symbols-outlined">playlist_add</span>
              </button>
              <button
                type="button"
                class="card-action-btn card-fav-btn favorited"
                onclick={(e) => void toggleFav(e, item)}
                title={t('common.unfavorite')}
              >
                <span class="material-symbols-outlined">favorite</span>
              </button>
            </div>

            {#if !playback?.completed && playback?.position_ms && item.duration_ms && item.duration_ms > 0}
              <div class="playback-progress-track">
                <div
                  class="playback-progress-bar"
                  style="width: {Math.min(100, Math.round((playback.position_ms / item.duration_ms) * 100))}%"
                ></div>
              </div>
            {/if}
          </div>

          <div class="card-info">
            <div class="channel-avatar" aria-hidden="true">
              {#if item.channel?.avatar_url}
                <img src={item.channel.avatar_url} alt="" class="avatar-img" />
              {:else if item.channel}
                {item.channel.name.charAt(0).toUpperCase()}
              {:else}
                <span class="material-symbols-outlined icon-18">video_library</span>
              {/if}
            </div>

            <div class="card-text-col">
              <a
                href={`/watch?v=${item.id}`}
                class="card-title-link"
                onclick={(e) => handleMediaClick(e, item)}
              >
                <h3 class="card-title" title={item.title}>{item.title}</h3>
              </a>

              {#if item.channel}
                <div class="channel-row">
                  <span class="channel-static-name">
                    {item.channel.name}
                  </span>
                </div>
              {/if}

              {#if item.tags.length > 0}
                <div class="card-tags">
                  {#each item.tags.slice(0, 3) as tag (tag.id)}
                    <span class="tag-badge">
                      #{tag.display_name}
                    </span>
                  {/each}
                </div>
              {/if}
            </div>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

{#if playlistModalMedia}
  <SaveToPlaylistModal
    media={playlistModalMedia}
    onClose={() => (playlistModalMedia = null)}
  />
{/if}

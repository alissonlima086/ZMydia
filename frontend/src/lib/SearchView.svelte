<script lang="ts">
  import { onMount } from 'svelte';
  import { apiFetch } from './api';
  import { player, type MediaItem } from './player.svelte';
  import SaveToPlaylistModal from './SaveToPlaylistModal.svelte';
  import Button from './ui/Button.svelte';
  import PageState from './ui/PageState.svelte';
  import MediaGridSkeleton from './ui/MediaGridSkeleton.svelte';
  import { t } from './i18n/i18n.svelte';

  interface Props {
    externalQuery?: string;
    onQueryChange?: (val: string) => void;
  }

  let { externalQuery = '', onQueryChange }: Props = $props();

  const STORAGE_KEY_HIDE_BLOCKED = 'mymedia_explore_hide_blocked';

  let query = $state('');
  let sortBy = $state<'recent' | 'title' | 'duration'>('recent');
  let hideBlockedVideos = $state(
    typeof localStorage !== 'undefined' && localStorage.getItem(STORAGE_KEY_HIDE_BLOCKED) === 'true'
  );
  let results = $state<MediaItem[]>([]);
  let playlistModalMedia = $state<MediaItem | null>(null);
  let loading = $state(false);
  let loadingMore = $state(false);
  let errorMessage = $state<string | null>(null);
  let offset = $state(0);
  let hasMore = $state(false);
  let hasSearched = $state(false);

  let searchTimeout: number | null = null;
  const limit = 30;

  onMount(() => {
    const urlParams = new URLSearchParams(window.location.search);
    const qFromUrl = urlParams.get('q') || urlParams.get('search');
    query = qFromUrl || externalQuery || '';
    if (onQueryChange && qFromUrl) onQueryChange(qFromUrl);
    void executeSearch(true);
  });

  function handlePopState() {
    const urlParams = new URLSearchParams(window.location.search);
    const qFromUrl = urlParams.get('q') || urlParams.get('search') || '';
    if (qFromUrl !== query) {
      query = qFromUrl;
      if (onQueryChange) onQueryChange(qFromUrl);
      void executeSearch(true);
    }
  }

  $effect(() => {
    if (externalQuery !== undefined && externalQuery !== query) {
      query = externalQuery;
      if (searchTimeout) clearTimeout(searchTimeout);
      searchTimeout = window.setTimeout(() => {
        void executeSearch(true);
      }, 300);
    }
  });

  function onQueryInput() {
    if (onQueryChange) onQueryChange(query);
    if (searchTimeout) clearTimeout(searchTimeout);
    searchTimeout = window.setTimeout(() => {
      void executeSearch(true);
    }, 300);
  }

  async function executeSearch(reset = false) {
    if (reset) {
      offset = 0;
      results = [];
    }
    loading = reset;
    loadingMore = !reset;
    errorMessage = null;

    try {
      const params = new URLSearchParams();
      if (query.trim()) {
        params.set('q', query.trim());
      }
      params.set('is_short', 'false');
      params.set('sort', sortBy);
      params.set('limit', limit.toString());
      params.set('offset', offset.toString());
      if (hideBlockedVideos) {
        params.set('exclude_home_hidden', 'true');
      }

      const res = await apiFetch(`/api/search?${params.toString()}`);
      if (!res.ok) {
        throw new Error(`Erro na busca: HTTP ${res.status}`);
      }
      const data = (await res.json()) as MediaItem[];
      if (reset) {
        results = data;
      } else {
        results = [...results, ...data];
      }
      hasMore = data.length >= limit;
      hasSearched = true;
    } catch (err) {
      errorMessage = err instanceof Error ? err.message : String(err);
    } finally {
      loading = false;
      loadingMore = false;
    }
  }

  function loadMore() {
    if (loadingMore || !hasMore) return;
    offset += limit;
    void executeSearch(false);
  }

  function setSort(sort: 'recent' | 'title' | 'duration') {
    sortBy = sort;
    void executeSearch(true);
  }

  function toggleHideBlocked(e: Event) {
    const target = e.target as HTMLInputElement;
    hideBlockedVideos = target.checked;
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(STORAGE_KEY_HIDE_BLOCKED, String(hideBlockedVideos));
    }
    void executeSearch(true);
  }

  function openChannelPage(e: MouseEvent, channelId: string) {
    e.stopPropagation();
    window.history.pushState({}, '', `/canal/${channelId}`);
    window.dispatchEvent(new PopStateEvent('popstate'));
  }

  function clearSearch() {
    query = '';
    if (onQueryChange) onQueryChange('');
    void executeSearch(true);
  }

  function handleMediaClick(event: MouseEvent, item: MediaItem) {
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || event.button !== 0) {
      return;
    }
    event.preventDefault();
    void player.playMedia(item, true);
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
</script>

<svelte:window onpopstate={handlePopState} />

<div class="search-view">
  <div class="search-controls">
    {#if query.trim()}
      <div class="search-query-indicator">
        <span class="material-symbols-outlined indicator-icon">search</span>
        <span>{t('search.searching_for')} <strong>“{query.trim()}”</strong></span>
        {#if hasSearched}
          <span class="results-badge">{t('search.results_badge', { count: results.length })}{hasMore ? '+' : ''}</span>
        {/if}
      </div>
    {/if}

    <div class="search-filters-row">
      <div class="filter-group">
        <span class="filter-label">{t('search.sort_label')}</span>
        <div class="pill-group">
          <button
            type="button"
            class="filter-pill-btn"
            class:active={sortBy === 'recent'}
            onclick={() => setSort('recent')}
          >
            {t('search.sort_recent')}
          </button>
          <button
            type="button"
            class="filter-pill-btn"
            class:active={sortBy === 'title'}
            onclick={() => setSort('title')}
          >
            {t('search.sort_title')}
          </button>
          <button
            type="button"
            class="filter-pill-btn"
            class:active={sortBy === 'duration'}
            onclick={() => setSort('duration')}
          >
            {t('search.sort_duration')}
          </button>
        </div>
      </div>

      <div class="filter-group filter-group-checkbox">
        <label
          class="filter-checkbox-label"
          title={t('search.filter_words_tip')}
        >
          <input
            type="checkbox"
            class="filter-checkbox-input"
            checked={hideBlockedVideos}
            onchange={toggleHideBlocked}
          />
          <span class="filter-checkbox-text">{t('search.filter_words')}</span>
        </label>
      </div>
    </div>
  </div>

  {#if loading}
    <MediaGridSkeleton count={6} label={t('search.searching_library')} />
  {:else if errorMessage}
    {#snippet retrySearch()}
      <Button variant="secondary" onclick={() => void executeSearch(true)}>{t('common.retry')}</Button>
    {/snippet}
    <PageState kind="error" icon="error" title={t('search.load_error')} description={errorMessage} actions={retrySearch} />
  {:else if hasSearched && results.length === 0}
    <PageState kind="empty" icon="search_off" title={t('search.empty_results', { query })} description={t('search.empty_tip')} />
  {:else}
    <div class="catalog-grid">
      {#each results as item, index (item.id)}
        {@const playback = player.getPlayback(item.id) ?? item.playback}
        <div
          class="media-card"
          role="button"
          tabindex="0"
          onclick={(e) => handleMediaClick(e, item)}
          onauxclick={(e) => handleAuxClick(e, item)}
          onkeydown={(e) => {
            if (e.target !== e.currentTarget || (e.key !== 'Enter' && e.key !== ' ')) return;
            e.preventDefault();
            void player.playMedia(item, true);
          }}
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
                class="card-action-btn card-fav-btn"
                class:favorited={item.is_favorite}
                onclick={(e) => {
                  e.stopPropagation();
                  void player.toggleFavorite(item);
                }}
                title={item.is_favorite ? t('common.unfavorite') : t('common.favorite')}
              >
                <span class="material-symbols-outlined">
                  {item.is_favorite ? 'favorite' : 'favorite_border'}
                </span>
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
            <div
              class="channel-avatar"
              class:channel-avatar-clickable={!!item.channel}
              onclick={(e) => item.channel && openChannelPage(e, item.channel.id)}
              aria-hidden="true"
            >
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
                  <button
                    type="button"
                    class="channel-btn"
                    onclick={(e) => openChannelPage(e, item.channel!.id)}
                    title={t('common.view_channel', { name: item.channel.name })}
                  >
                    {item.channel.name}
                  </button>
                </div>
              {/if}

              {#if item.tags.length > 0}
                <div class="card-tags">
                  {#each item.tags.slice(0, 3) as tag (tag.id)}
                    <span class="tag-badge">#{tag.display_name}</span>
                  {/each}
                  {#if item.tags.length > 3}
                    <span class="tag-badge more">+{item.tags.length - 3}</span>
                  {/if}
                </div>
              {/if}
            </div>
          </div>
        </div>
      {/each}
    </div>

    {#if hasMore}
      <div class="feed-load-more">
        <button
          type="button"
          class="load-more-btn"
          disabled={loadingMore}
          onclick={loadMore}
        >
          {#if loadingMore}
            <span class="spinner-small"></span> {t('common.loading')}
          {:else}
            {t('search.load_more_results')}
          {/if}
        </button>
      </div>
    {/if}
  {/if}
</div>

{#if playlistModalMedia}
  <SaveToPlaylistModal
    media={playlistModalMedia}
    onClose={() => (playlistModalMedia = null)}
  />
{/if}

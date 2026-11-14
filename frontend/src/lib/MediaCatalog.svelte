<script lang="ts">
  import { onMount } from 'svelte';
  import { apiFetch } from './api';
  import { player, type MediaItem } from './player.svelte';
  import SaveToPlaylistModal from './SaveToPlaylistModal.svelte';
  import Button from './ui/Button.svelte';
  import PageState from './ui/PageState.svelte';
  import MediaGridSkeleton from './ui/MediaGridSkeleton.svelte';
  import { t } from './i18n/i18n.svelte';

  type TagSummary = {
    id: number;
    display_name: string;
    normalized_name: string;
    media_count: number;
  };

  type FeedResponse = {
    items: MediaItem[];
    next_cursor: string | null;
    seed: number;
  };

  interface Props {
    isFeed?: boolean;
    initialTag?: string | null;
    initialChannel?: string | null;
    isShort?: boolean | null;
    onSelectChannel?: (channelId: string) => void;
  }

  let {
    isFeed = true,
    initialTag = null,
    initialChannel = null,
    isShort = isFeed ? false : null,
    onSelectChannel
  }: Props = $props();

  let mediaList = $state<MediaItem[]>([]);
  let allTags = $state<TagSummary[]>([]);
  let playlistModalMedia = $state<MediaItem | null>(null);

  function openChannelPage(e: MouseEvent, channelId: string) {
    e.stopPropagation();
    window.history.pushState({}, '', `/canal/${channelId}`);
    window.dispatchEvent(new PopStateEvent('popstate'));
  }
  let displayedTags = $state<TagSummary[]>([]);
  let selectedTag = $state<string | null>(null);
  let selectedChannel = $state<string | null>(null);
  let sortOrder = $state<string>('random');
  let recEnabled = $state(false);
  const isCursorFeed = $derived(sortOrder === 'random' || sortOrder === 'recommended');
  let nextCursor = $state<string | null>(null);
  let sessionSeed = $state<number | null>(null);
  let sessionRecentIds = $state<string | null>(null);
  let recentFeedGenerations = $state<number[][]>([]);
  let mainFeedCache = $state<{
    items: MediaItem[];
    nextCursor: string | null;
    seed: number;
    recentIds: string | null;
  } | null>(null);
  let loading = $state(true);
  let loadingMore = $state(false);
  let errorMessage = $state<string | null>(null);
  let offset = $state(0);
  let hasMoreStatic = $state(false);

  onMount(() => {
    selectedTag = initialTag;
    selectedChannel = initialChannel;
    try {
      const stored = JSON.parse(sessionStorage.getItem('mymedia-recent-feed-generations') ?? '[]');
      if (Array.isArray(stored)) {
        recentFeedGenerations = stored
          .slice(0, 3)
          .map((generation) =>
            Array.isArray(generation)
              ? generation.filter((id): id is number => Number.isInteger(id)).slice(0, 24)
              : []
          );
      }
    } catch {
      recentFeedGenerations = [];
    }
    void initCatalog();
  });

  function serializeRecentGenerations(): string | null {
    const serialized = recentFeedGenerations
      .filter((generation) => generation.length > 0)
      .slice(0, 3)
      .map((generation) => generation.join(','))
      .join('|');
    return serialized || null;
  }

  function rememberRecommendedGrid(items: MediaItem[]) {
    if (sortOrder !== 'recommended' || items.length === 0) return;
    recentFeedGenerations = [items.slice(0, 24).map((item) => item.id), ...recentFeedGenerations]
      .slice(0, 3);
    try {
      sessionStorage.setItem(
        'mymedia-recent-feed-generations',
        JSON.stringify(recentFeedGenerations)
      );
    } catch {
      // A recomendação continua funcionando quando o armazenamento da sessão está indisponível.
    }
  }

  function startNewFeedSession() {
    sessionSeed = null;
    sessionRecentIds = null;
  }

  async function initCatalog() {
    if (isFeed) {
      try {
        const rec = await request<{ enabled: boolean }>('/api/settings/recommendation');
        if (rec && rec.enabled) {
          recEnabled = true;
          sortOrder = 'recommended';
        }
      } catch {}
    }
    void loadData();
  }

  $effect(() => {
    if (initialChannel !== undefined && initialChannel !== selectedChannel) {
      selectedChannel = initialChannel;
      void applyFilter();
    }
  });

  async function request<T>(url: string): Promise<T> {
    const response = await apiFetch(url);
    if (!response.ok) {
      const payload = (await response.json().catch(() => ({}))) as { error?: string };
      throw new Error(payload.error ?? `HTTP ${response.status}`);
    }
    const text = await response.text();
    if (!text) return undefined as T;
    return JSON.parse(text) as T;
  }

  function sampleTags() {
    if (allTags.length <= 10) {
      displayedTags = [...allTags];
      return;
    }
    const sample = [...allTags].sort(() => 0.5 - Math.random()).slice(0, 10);
    if (selectedTag) {
      const inSample = sample.some(
        (t) => t.normalized_name === selectedTag || t.display_name === selectedTag
      );
      if (!inSample) {
        const found = allTags.find(
          (t) => t.normalized_name === selectedTag || t.display_name === selectedTag
        );
        if (found) {
          sample[sample.length - 1] = found;
        }
      }
    }
    displayedTags = sample;
  }

  async function loadData() {
    loading = true;
    errorMessage = null;
    nextCursor = null;
    offset = 0;
    try {
      const tagsUrl = isFeed ? '/api/tags?exclude_home_hidden=true' : '/api/tags';
      const [mediaRes, tagsRes] = await Promise.all([
        fetchFirstPage(),
        request<TagSummary[]>(tagsUrl)
      ]);
      mediaList = mediaRes;
      allTags = tagsRes;
      sampleTags();
      if (!selectedTag && !selectedChannel && isCursorFeed && sessionSeed !== null) {
        mainFeedCache = {
          items: [...mediaRes],
          nextCursor,
          seed: sessionSeed,
          recentIds: sessionRecentIds
        };
      }
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : String(error);
    } finally {
      loading = false;
    }
  }

  async function fetchFirstPage(): Promise<MediaItem[]> {
    offset = 0;
    if (isCursorFeed) {
      const params = new URLSearchParams();
      if (sessionSeed !== null) params.set('seed', sessionSeed.toString());
      if (sessionRecentIds === null) sessionRecentIds = serializeRecentGenerations();
      if (sessionRecentIds) params.set('recent_ids', sessionRecentIds);
      params.set('limit', '24');
      if (sortOrder === 'recommended') params.set('mode', 'recommended');
      if (sortOrder === 'random') params.set('mode', 'random');
      if (selectedTag) params.set('tag', selectedTag);
      if (selectedChannel) params.set('channel', selectedChannel);
      if (isShort !== null) params.set('is_short', isShort ? 'true' : 'false');
      if (isFeed) params.set('exclude_home_hidden', 'true');

      const res = await request<FeedResponse>(`/api/feed?${params.toString()}`);
      nextCursor = res.next_cursor;
      sessionSeed = res.seed;
      rememberRecommendedGrid(res.items);
      return res.items;
    } else {
      const params = new URLSearchParams();
      params.set('sort', sortOrder);
      if (selectedTag) params.set('tag', selectedTag);
      if (selectedChannel) params.set('channel', selectedChannel);
      if (isShort !== null) params.set('is_short', isShort ? 'true' : 'false');
      if (isFeed) params.set('exclude_home_hidden', 'true');
      params.set('limit', '24');
      params.set('offset', '0');

      const res = await request<MediaItem[]>(`/api/media?${params.toString()}`);
      hasMoreStatic = res.length >= 24;
      offset = res.length;
      return res;
    }
  }

  async function loadMore() {
    if (loadingMore) return;
    loadingMore = true;
    try {
      if (isCursorFeed) {
        if (!nextCursor || sessionSeed === null) return;
        const params = new URLSearchParams();
        params.set('seed', sessionSeed.toString());
        if (sessionRecentIds) params.set('recent_ids', sessionRecentIds);
        params.set('cursor', nextCursor);
        params.set('limit', '24');
        if (sortOrder === 'recommended') params.set('mode', 'recommended');
        if (sortOrder === 'random') params.set('mode', 'random');
        if (selectedTag) params.set('tag', selectedTag);
        if (selectedChannel) params.set('channel', selectedChannel);
        if (isShort !== null) params.set('is_short', isShort ? 'true' : 'false');
        if (isFeed) params.set('exclude_home_hidden', 'true');

        const res = await request<FeedResponse>(`/api/feed?${params.toString()}`);
        nextCursor = res.next_cursor;
        mediaList = [...mediaList, ...res.items];
        if (!selectedTag && !selectedChannel && isCursorFeed) {
          mainFeedCache = {
            items: [...mediaList],
            nextCursor,
            seed: sessionSeed,
            recentIds: sessionRecentIds
          };
        }
      } else {
        if (!hasMoreStatic) return;
        const params = new URLSearchParams();
        params.set('sort', sortOrder);
        if (selectedTag) params.set('tag', selectedTag);
        if (selectedChannel) params.set('channel', selectedChannel);
        if (isShort !== null) params.set('is_short', isShort ? 'true' : 'false');
        if (isFeed) params.set('exclude_home_hidden', 'true');
        params.set('limit', '24');
        params.set('offset', offset.toString());

        const res = await request<MediaItem[]>(`/api/media?${params.toString()}`);
        hasMoreStatic = res.length >= 24;
        offset += res.length;
        mediaList = [...mediaList, ...res];
      }
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : String(error);
    } finally {
      loadingMore = false;
    }
  }

  async function handleSortChange() {
    mainFeedCache = null;
    if (isCursorFeed) {
      startNewFeedSession();
    }
    await applyFilter();
  }

  async function refreshCatalog() {
    mainFeedCache = null;
    if (isCursorFeed) {
      startNewFeedSession();
    }
    sampleTags();
    await applyFilter();
  }

  async function selectTag(tagName: string | null) {
    if (!tagName || selectedTag === tagName) {
      return clearAllFilters();
    }
    selectedTag = tagName;
    if (isCursorFeed) startNewFeedSession();
    await applyFilter();
  }

  async function selectChannel(channelId: string | null) {
    if (onSelectChannel && channelId) {
      onSelectChannel(channelId);
      return;
    }
    if (!channelId || selectedChannel === channelId) {
      selectedChannel = null;
      if (!selectedTag) {
        return clearAllFilters();
      }
      await applyFilter();
      return;
    }
    selectedChannel = channelId;
    if (isCursorFeed) startNewFeedSession();
    await applyFilter();
  }

  async function clearAllFilters() {
    if (selectedTag === null && selectedChannel === null) {
      // Já está em "Tudo", não reinicia o feed nem re-sorteia tags
      return;
    }
    selectedTag = null;
    selectedChannel = null;

    // Se já temos o feed principal em cache, restaura instantaneamente
    if (isCursorFeed && mainFeedCache && mainFeedCache.items.length > 0) {
      sessionSeed = mainFeedCache.seed;
      sessionRecentIds = mainFeedCache.recentIds;
      mediaList = [...mainFeedCache.items];
      nextCursor = mainFeedCache.nextCursor;
      return;
    }

    if (isCursorFeed) startNewFeedSession();
    await applyFilter();
  }

  async function applyFilter() {
    loading = true;
    nextCursor = null;
    offset = 0;
    try {
      mediaList = await fetchFirstPage();
      if (!selectedTag && !selectedChannel && isCursorFeed && sessionSeed !== null) {
        mainFeedCache = {
          items: [...mediaList],
          nextCursor,
          seed: sessionSeed,
          recentIds: sessionRecentIds
        };
      }
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : String(error);
    } finally {
      loading = false;
    }
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

<div class="catalog-section">
  <div class="catalog-header-bar">
    {#if displayedTags.length > 0}
      <div class="tag-bar">
        <button
          type="button"
          class="tag-chip"
          class:active={selectedTag === null && selectedChannel === null}
          onclick={() => void clearAllFilters()}
        >
          {t('catalog.all_tag')}
        </button>
        {#each displayedTags as tag (tag.id)}
          <button
            type="button"
            class="tag-chip"
            class:active={selectedTag === tag.normalized_name || selectedTag === tag.display_name}
            onclick={() => void selectTag(tag.normalized_name)}
          >
            {tag.display_name}
          </button>
        {/each}
      </div>
    {/if}

    <div class="catalog-actions">
      <div class="sort-selector-wrap">
        <span class="material-symbols-outlined sort-icon">sort</span>
        <select
          class="sort-select"
          bind:value={sortOrder}
          onchange={() => void handleSortChange()}
          aria-label={t('catalog.sort_label')}
        >
          {#if isFeed && recEnabled}
            <option value="recommended">{t('catalog.recommended')}</option>
          {/if}
          <option value="random">{t('catalog.random')}</option>
          <option value="title_asc">{t('catalog.title_asc')}</option>
          <option value="title_desc">{t('catalog.title_desc')}</option>
          <option value="recent">{t('catalog.recent')}</option>
          <option value="duration_desc">{t('catalog.duration_desc')}</option>
          <option value="duration_asc">{t('catalog.duration_asc')}</option>
        </select>
      </div>

      <button
        type="button"
        class="refresh-catalog-btn"
        onclick={() => void refreshCatalog()}
        title={t('catalog.update_list')}
      >
        <span class="material-symbols-outlined icon-16">refresh</span>
        <span>{t('catalog.refresh_btn')}</span>
      </button>
    </div>
  </div>

  {#if selectedTag || selectedChannel}
    <div class="active-filters-bar">
      <span>{t('catalog.active_filters')}</span>
      {#if selectedTag}
        <button type="button" class="filter-pill" onclick={() => void selectTag(null)}>
          <span>Tag: #{selectedTag}</span>
          <span class="material-symbols-outlined icon-14 filter-remove-icon">close</span>
        </button>
      {/if}
      {#if selectedChannel}
        <button type="button" class="filter-pill" onclick={() => void selectChannel(null)}>
          <span>Canal: {selectedChannel}</span>
          <span class="material-symbols-outlined icon-14 filter-remove-icon">close</span>
        </button>
      {/if}
      <button type="button" class="clear-all-btn" onclick={() => void clearAllFilters()}>
        Limpar todos
      </button>
    </div>
  {/if}

  {#if loading}
    <MediaGridSkeleton label={isFeed ? t('catalog.loading_feed') : t('catalog.loading_videos')} />
  {:else if errorMessage}
    {#snippet retryCatalog()}
      <Button variant="secondary" onclick={() => void loadData()}>{t('common.retry')}</Button>
    {/snippet}
    <PageState kind="error" icon="error" title={t('catalog.load_error')} description={errorMessage} actions={retryCatalog} />
  {:else if mediaList.length === 0}
    {#snippet emptyCatalogAction()}
      {#if selectedTag || selectedChannel}
        <Button variant="secondary" onclick={() => void clearAllFilters()}>{t('catalog.clear_filters')}</Button>
      {/if}
    {/snippet}
    <PageState
      kind="empty"
      icon="video_library"
      title={t('catalog.no_videos_found')}
      description={selectedTag || selectedChannel ? undefined : t('catalog.empty_desc')}
      actions={emptyCatalogAction}
    />
  {:else}
    <div class="catalog-grid">
      {#each mediaList as item, index (item.id)}
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
                    title={`Ver canal ${item.channel.name}`}
                  >
                    {item.channel.name}
                  </button>
                </div>
              {/if}

              {#if item.tags.length > 0}
                <div class="card-tags">
                  {#each item.tags.slice(0, 3) as tag (tag.id)}
                    <button
                      type="button"
                      class="tag-badge"
                      onclick={(e) => {
                        e.stopPropagation();
                        void selectTag(tag.normalized_name);
                      }}
                    >
                      #{tag.display_name}
                    </button>
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

    {#if (isCursorFeed && nextCursor) || (!isCursorFeed && hasMoreStatic)}
      <div class="feed-load-more">
        <button
          type="button"
          class="load-more-btn"
          disabled={loadingMore}
          onclick={() => void loadMore()}
        >
          {#if loadingMore}
            <span class="spinner-small"></span> {t('catalog.loading_videos')}
          {:else}
            Carregar mais vídeos
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

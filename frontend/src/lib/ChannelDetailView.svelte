<script lang="ts">
  import { apiFetch } from './api';
  import { player, type MediaItem } from './player.svelte';
  import SaveToPlaylistModal from './SaveToPlaylistModal.svelte';
  import { t } from './i18n/i18n.svelte';

  type ChannelDetail = {
    id: string;
    name: string;
    avatar_url: OptionString;
    youtube_channel_id: OptionString;
    handle: OptionString;
    has_name_conflict: boolean;
    media_count: number;
  };
  type OptionString = string | null;

  type PlaylistSummary = {
    id: string;
    title: string;
    item_count: number;
  };

  let {
    channelId,
    initialTab = 'home',
    onBack
  }: {
    channelId: string;
    initialTab?: string;
    onBack?: () => void;
  } = $props();

  let channel = $state<ChannelDetail | null>(null);
  let allMedia = $state<MediaItem[]>([]);
  let mentionMedia = $state<MediaItem[]>([]);
  let channelPlaylists = $state<PlaylistSummary[]>([]);
  let loading = $state(true);
  let errorMessage = $state<string | null>(null);
  let loadRequestId = 0;

  type TabType = 'home' | 'videos' | 'shorts' | 'mentions' | 'playlists';

  function parseTab(t?: string): TabType {
    if (t === 'videos' || t === 'shorts' || t === 'mentions' || t === 'playlists') return t;
    return 'home';
  }

  let activeTab = $state<TabType>('home');
  let searchQuery = $state('');
  let videoSort = $state<'recent' | 'oldest' | 'duration'>('recent');

  $effect(() => {
    if (initialTab) {
      activeTab = parseTab(initialTab);
    }
  });

  function selectTab(tab: TabType) {
    activeTab = tab;
    const path = tab === 'home' ? `/canal/${channelId}` : `/canal/${channelId}/${tab}`;
    if (window.location.pathname !== path) {
      window.history.pushState({}, '', path);
    }
  }

  let playlistModalMedia = $state<MediaItem | null>(null);

  $effect(() => {
    const requestedChannelId = channelId;
    searchQuery = '';
    void loadChannelData(requestedChannelId);
  });

  async function loadChannelData(requestedChannelId = channelId) {
    const requestId = ++loadRequestId;
    loading = true;
    errorMessage = null;
    channel = null;
    allMedia = [];
    mentionMedia = [];
    channelPlaylists = [];
    try {
      const encodedChannelId = encodeURIComponent(requestedChannelId);
      const [cRes, mRes, mentionRes, pRes] = await Promise.all([
        apiFetch(`/api/channels/${encodedChannelId}`),
        apiFetch(`/api/media?channel=${encodedChannelId}&limit=200&sort=published_desc`),
        apiFetch(`/api/channels/${encodedChannelId}/mentions`),
        apiFetch('/api/playlists').catch(() => null)
      ]);

      if (requestId !== loadRequestId) return;

      if (!cRes.ok) {
        throw new Error(`Canal não encontrado (HTTP ${cRes.status})`);
      }

      const loadedChannel = (await cRes.json()) as ChannelDetail;
      const loadedMedia = mRes.ok ? ((await mRes.json()) as MediaItem[]) : [];
      const loadedMentions = mentionRes.ok ? ((await mentionRes.json()) as MediaItem[]) : [];
      const loadedPlaylists: PlaylistSummary[] = [];

      if (pRes && pRes.ok) {
        const playlists = (await pRes.json()) as PlaylistSummary[];
        // Filtra playlists que contêm ao menos um vídeo deste canal
        await Promise.all(
          playlists.map(async (pl) => {
            const detailRes = await apiFetch(`/api/playlists/${pl.id}`).catch(() => null);
            if (detailRes && detailRes.ok) {
              const detail = (await detailRes.json()) as { items: MediaItem[] };
              if (detail.items?.some((it) => it.channel?.id === requestedChannelId)) {
                loadedPlaylists.push(pl);
              }
            }
          })
        );
      }

      if (requestId !== loadRequestId) return;
      channel = loadedChannel;
      allMedia = loadedMedia;
      mentionMedia = loadedMentions;
      channelPlaylists = loadedPlaylists;
    } catch (err) {
      if (requestId === loadRequestId) {
        errorMessage = err instanceof Error ? err.message : String(err);
      }
    } finally {
      if (requestId === loadRequestId) {
        loading = false;
      }
    }
  }

  // Vídeos filtrados
  const normalVideos = $derived(allMedia.filter((m) => !m.is_short));
  const shortVideos = $derived(allMedia.filter((m) => m.is_short));
  const featuredVideo = $derived(normalVideos[0] || allMedia[0] || null);

  function compareByPublication(a: MediaItem, b: MediaItem, oldestFirst = false): number {
    const aPublished = a.published_at ? Date.parse(a.published_at) : Number.NaN;
    const bPublished = b.published_at ? Date.parse(b.published_at) : Number.NaN;
    const aHasDate = Number.isFinite(aPublished);
    const bHasDate = Number.isFinite(bPublished);

    // Itens sem data conhecida permanecem no fim em ambas as direções.
    if (aHasDate !== bHasDate) return aHasDate ? -1 : 1;
    if (aHasDate && bHasDate && aPublished !== bPublished) {
      return oldestFirst ? aPublished - bPublished : bPublished - aPublished;
    }
    return oldestFirst ? a.id - b.id : b.id - a.id;
  }

  // Vídeos normais ordenados
  const sortedNormalVideos = $derived.by(() => {
    let list = [...normalVideos];
    if (searchQuery.trim()) {
      const q = searchQuery.trim().toLowerCase();
      list = list.filter((m) => m.title.toLowerCase().includes(q) || m.tags.some((t) => t.display_name.toLowerCase().includes(q)));
    }
    if (videoSort === 'oldest') {
      return list.sort((a, b) => compareByPublication(a, b, true));
    } else if (videoSort === 'duration') {
      return list.sort((a, b) => (b.duration_ms || 0) - (a.duration_ms || 0));
    }
    return list.sort((a, b) => compareByPublication(a, b));
  });

  let shortsSort = $state<'recent' | 'oldest' | 'duration'>('recent');

  const filteredShorts = $derived.by(() => {
    if (!searchQuery.trim()) return shortVideos;
    const q = searchQuery.trim().toLowerCase();
    return shortVideos.filter((m) => m.title.toLowerCase().includes(q));
  });

  const sortedShorts = $derived.by(() => {
    let list = [...filteredShorts];
    if (shortsSort === 'oldest') {
      return list.sort((a, b) => compareByPublication(a, b, true));
    } else if (shortsSort === 'duration') {
      return list.sort((a, b) => (b.duration_ms || 0) - (a.duration_ms || 0));
    }
    return list.sort((a, b) => compareByPublication(a, b));
  });

  const filteredMentions = $derived.by(() => {
    if (!searchQuery.trim()) return mentionMedia;
    const query = searchQuery.trim().toLowerCase();
    return mentionMedia.filter(
      (item) =>
        item.title.toLowerCase().includes(query) ||
        item.channel?.name.toLowerCase().includes(query)
    );
  });

  function handleShortClick(event: MouseEvent, item: MediaItem) {
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || event.button !== 0) {
      return;
    }
    event.preventDefault();
    const url = `/shorts?channel=${encodeURIComponent(channelId)}&v=${item.id}`;
    window.history.pushState({}, '', url);
    window.dispatchEvent(new CustomEvent('routechange'));
  }

  function playAllVideos() {
    if (normalVideos.length > 0) {
      void player.playQueue(normalVideos, 0);
    } else if (allMedia.length > 0) {
      void player.playQueue(allMedia, 0);
    }
  }

  function handleMediaClick(event: MouseEvent, item: MediaItem) {
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || event.button !== 0) {
      return;
    }
    event.preventDefault();
    void player.playMedia(item, true);
  }

  function openChannelPage(event: MouseEvent, id: string) {
    event.stopPropagation();
    window.history.pushState({}, '', `/canal/${id}`);
    window.dispatchEvent(new PopStateEvent('popstate'));
  }

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

  function totalDurationFormatted(): string {
    const totalMs = allMedia.reduce((acc, it) => acc + (it.duration_ms || 0), 0);
    const hours = Math.floor(totalMs / 3600000);
    const mins = Math.floor((totalMs % 3600000) / 60000);
    if (hours > 0) return `${hours}h ${mins}m de conteúdo`;
    return `${mins} min de conteúdo`;
  }
</script>

<div class="channel-page-layout">
  {#if loading}
    <div class="catalog-loading">
      <div class="spinner"></div>
      <p>{t('channels.loading')}</p>
    </div>
  {:else if errorMessage || !channel}
    <div class="catalog-error">
      <span>{errorMessage || 'Canal não encontrado.'}</span>
      <button type="button" onclick={() => void loadChannelData()}>{t('common.retry')}</button>
    </div>
  {:else}
    <!-- 1. Banner Panorâmico Estilo YouTube -->
    <div class="channel-banner-wrapper">
      <div
        class="channel-banner"
        style={featuredVideo?.thumbnail_url
          ? `background-image: linear-gradient(180deg, rgba(15,15,15,0.2) 0%, rgba(15,15,15,0.85) 100%), url('${featuredVideo.thumbnail_url}')`
          : 'background: linear-gradient(135deg, #1f2937 0%, #111827 100%)'}
      >
        <div class="channel-banner-glow"></div>
      </div>
    </div>

    <!-- 2. Cabeçalho de Identidade do Criador -->
    <div class="channel-header-container">
      <div class="channel-header-main">
        <!-- Avatar Gigante -->
        <div class="channel-avatar-large">
          {#if channel.avatar_url}
            <img src={channel.avatar_url} alt={channel.name} class="avatar-large-img" />
          {:else}
            <span class="material-symbols-outlined avatar-large-fallback">subscriptions</span>
          {/if}
        </div>

        <!-- Dados do Canal -->
        <div class="channel-identity-meta">
          <div class="channel-title-line">
            <h1 class="channel-page-title">{channel.name}</h1>
            <span class="material-symbols-outlined verified-badge" title={t('channels.verified_channel')}>verified</span>
          </div>

          <div class="channel-info-row">
            {#if channel.handle}
              <span class="channel-handle-text">{channel.handle}</span>
              <span class="meta-dot">•</span>
            {/if}
            <span class="channel-count-text">
              {t('common.video_count', { count: allMedia.length })}
            </span>
            <span class="meta-dot">•</span>
            <span class="channel-duration-text">{totalDurationFormatted()}</span>
          </div>

          <p class="channel-bio-text">
            {t('channels.bio_text', { count: allMedia.length })}
            {#if shortVideos.length > 0}
              {t('channels.bio_shorts', { count: shortVideos.length })}
            {/if}
          </p>

          <!-- Botões de Ação do Canal -->
          <div class="channel-action-buttons">
            <button
              type="button"
              class="primary-channel-btn play-all-btn"
              onclick={playAllVideos}
              disabled={allMedia.length === 0}
            >
              <span class="material-symbols-outlined">play_arrow</span>
              <span>{t('channels.play_all')}</span>
            </button>

            {#if channel.youtube_channel_id}
              <a
                href={`https://www.youtube.com/channel/${channel.youtube_channel_id}`}
                target="_blank"
                rel="noreferrer"
                class="secondary-channel-btn yt-link-btn"
                title={t('channels.open_official_yt')}
              >
                <span class="material-symbols-outlined icon-17">open_in_new</span>
                <span>{t('channels.open_official_yt')}</span>
              </a>
            {/if}

            {#if onBack}
              <button type="button" class="secondary-channel-btn" onclick={onBack}>
                <span class="material-symbols-outlined icon-17">arrow_back</span>
                <span>{t('channels.all_channels')}</span>
              </button>
            {/if}
          </div>
        </div>
      </div>
    </div>

    <!-- 3. Barra de Abas de Navegação -->
    <div class="channel-tabs-bar">
      <div class="channel-tabs-list">
        <button
          type="button"
          class="channel-tab-btn"
          class:active={activeTab === 'home'}
          onclick={() => selectTab('home')}
        >
          {t('channels.tabs_home')}
        </button>

        <button
          type="button"
          class="channel-tab-btn"
          class:active={activeTab === 'videos'}
          onclick={() => selectTab('videos')}
        >
          {t('channels.tabs_videos_count', { count: normalVideos.length })}
        </button>

        {#if shortVideos.length > 0}
          <button
            type="button"
            class="channel-tab-btn"
            class:active={activeTab === 'shorts'}
            onclick={() => selectTab('shorts')}
          >
            {t('channels.tabs_shorts_count', { count: shortVideos.length })}
          </button>
        {/if}

        <button
          type="button"
          class="channel-tab-btn"
          class:active={activeTab === 'mentions'}
          onclick={() => selectTab('mentions')}
        >
          {t('channels.tabs_mentions_count', { count: mentionMedia.length })}
        </button>

        {#if channelPlaylists.length > 0}
          <button
            type="button"
            class="channel-tab-btn"
            class:active={activeTab === 'playlists'}
            onclick={() => selectTab('playlists')}
          >
            {t('channels.tabs_playlists')} ({channelPlaylists.length})
          </button>
        {/if}
      </div>

      <!-- Campo de Busca no Canal -->
      <div class="channel-search-box">
        <span class="material-symbols-outlined channel-search-icon">search</span>
        <input
          type="text"
          class="channel-search-input"
          placeholder={t('channels.search_in_channel')}
          bind:value={searchQuery}
        />
        {#if searchQuery}
          <button type="button" class="channel-search-clear" onclick={() => (searchQuery = '')}>
            <span class="material-symbols-outlined icon-15">close</span>
          </button>
        {/if}
      </div>
    </div>

    <!-- 4. Conteúdo das Abas -->
    <div class="channel-tab-content">
      <!-- ABA 1: INÍCIO (Destaque + Recentes + Shorts) -->
      {#if activeTab === 'home'}
        {#if featuredVideo && !searchQuery}
          <section class="channel-featured-section">
            <h3 class="section-label">{t('channels.featured')}</h3>
            <div class="featured-video-card">
              <a
                href={`/watch?v=${featuredVideo.id}`}
                class="featured-thumb-wrap"
                onclick={(e) => handleMediaClick(e, featuredVideo)}
              >
                <img src={featuredVideo.thumbnail_url} alt={featuredVideo.title} class="featured-thumb-img" />
                {#if featuredVideo.duration_ms}
                  <span class="badge duration-badge">{formatDuration(featuredVideo.duration_ms)}</span>
                {/if}
              </a>
              <div class="featured-details">
                <a
                  href={`/watch?v=${featuredVideo.id}`}
                  class="featured-title-link"
                  onclick={(e) => handleMediaClick(e, featuredVideo)}
                >
                  <h2>{featuredVideo.title}</h2>
                </a>
                <p class="featured-desc">
                  {featuredVideo.description || t('channels.featured_fallback')}
                </p>
                <button
                  type="button"
                  class="primary-action featured-play-btn"
                  onclick={() => void player.playMedia(featuredVideo, true)}
                >
                  <span class="material-symbols-outlined">play_arrow</span>
                  <span>{t('downloads.watch_now')}</span>
                </button>
              </div>
            </div>
          </section>
        {/if}

        <!-- Seção: Shorts do Canal (Estilo YouTube - Sessão Exclusiva na Página Principal) -->
        {#if shortVideos.length > 0}
          <section class="channel-section-block channel-shorts-section">
            <div class="section-header-row">
              <div class="shorts-header-title">
                <svg viewBox="0 0 24 24" width="22" height="22" class="shorts-icon-svg" aria-hidden="true">
                  <path d="M17.77 10.32l-1.2-.5-1.1-.47.65-1.14a4.43 4.43 0 00.48-1.96 4.49 4.49 0 00-4.48-4.48c-.9 0-1.76.27-2.5.78L4.65 6.27A4.47 4.47 0 003 9.77c0 1.25.5 2.45 1.4 3.32l1.2.5 1.1.48-.65 1.13c-.32.55-.48 1.18-.48 1.83 0 2.48 2 4.5 4.48 4.5.9 0 1.76-.27 2.5-.78l4.97-3.72a4.47 4.47 0 001.65-3.5 4.48 4.48 0 00-1.4-3.21z" fill="#ff0033"/>
                  <path d="M10 8.5v7l6-3.5-6-3.5z" fill="#ffffff"/>
                </svg>
                <h3 class="section-label">Shorts</h3>
              </div>
              {#if shortVideos.length > 6}
                <button type="button" class="view-all-link shorts-view-all" onclick={() => selectTab('shorts')}>
                  <span>{t('shorts.view_all')}</span>
                  <span class="material-symbols-outlined icon-16">arrow_forward</span>
                </button>
              {/if}
            </div>

            <div class="channel-shorts-row">
              {#each shortVideos.slice(0, 6) as item, index (item.id)}
                <div class="channel-short-card">
                  <div class="short-thumb-wrapper">
                    <a
                      href={`/shorts?channel=${encodeURIComponent(channelId)}&v=${item.id}`}
                      class="short-thumb-link"
                      onclick={(e) => handleShortClick(e, item)}
                    >
                      <img src={item.thumbnail_url} alt={item.title} class="short-thumb-img" loading="eager" decoding={index < 4 ? 'sync' : 'async'} fetchpriority={index < 2 ? 'high' : 'auto'} />
                      <div class="short-hover-overlay">
                        <span class="material-symbols-outlined short-play-icon">play_arrow</span>
                      </div>
                      <span class="badge short-badge">Short</span>
                    </a>

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
                  </div>

                  <a
                    href={`/shorts?channel=${encodeURIComponent(channelId)}&v=${item.id}`}
                    class="channel-item-title-link"
                    onclick={(e) => handleShortClick(e, item)}
                  >
                    <h4 class="channel-item-title short-title-clamp" title={item.title}>{item.title}</h4>
                  </a>
                </div>
              {/each}
            </div>
          </section>
        {/if}

        <!-- Seção: Recentes -->
        <section class="channel-section-block">
          <div class="section-header-row">
            <h3 class="section-label">{t('channels.channel_videos')}</h3>
            {#if normalVideos.length > 8}
              <button type="button" class="view-all-link" onclick={() => selectTab('videos')}>
                {t('common.view_all')}
              </button>
            {/if}
          </div>

          <div class="channel-video-grid">
            {#each sortedNormalVideos.slice(0, 12) as item, index (item.id)}
              {@const playback = player.getPlayback(item.id) ?? item.playback}
              <div class="channel-card-item">
                <div class="thumb-wrap">
                  <a
                    href={`/watch?v=${item.id}`}
                    class="thumb-link"
                    onclick={(e) => handleMediaClick(e, item)}
                  >
                    <img src={item.thumbnail_url} alt={item.title} class="thumb-img" loading="eager" decoding={index < 8 ? 'sync' : 'async'} fetchpriority={index < 4 ? 'high' : 'auto'} />
                  </a>

                  {#if playback?.completed}
                    <span class="badge watched-badge">
                      <span class="material-symbols-outlined watched-icon">check</span>
                      <span>{t('common.watched')}</span>
                    </span>
                  {/if}

                  {#if item.duration_ms}
                    <span class="badge duration-badge">{formatDuration(item.duration_ms)}</span>
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

                <div class="channel-item-meta">
                  <a
                    href={`/watch?v=${item.id}`}
                    class="channel-item-title-link"
                    onclick={(e) => handleMediaClick(e, item)}
                  >
                    <h4 class="channel-item-title" title={item.title}>{item.title}</h4>
                  </a>
                </div>
              </div>
            {/each}
          </div>
        </section>

      <!-- ABA 2: VÍDEOS COMPLETOS -->
      {:else if activeTab === 'videos'}
        <div class="tab-filters-row">
          <div class="sort-chips">
            <button
              type="button"
              class="sort-chip"
              class:active={videoSort === 'recent'}
              onclick={() => (videoSort = 'recent')}
            >
              {t('common.sort_recent')}
            </button>
            <button
              type="button"
              class="sort-chip"
              class:active={videoSort === 'duration'}
              onclick={() => (videoSort = 'duration')}
            >
              {t('common.sort_longest')}
            </button>
            <button
              type="button"
              class="sort-chip"
              class:active={videoSort === 'oldest'}
              onclick={() => (videoSort = 'oldest')}
            >
              {t('common.sort_oldest')}
            </button>
          </div>
        </div>

        {#if sortedNormalVideos.length === 0}
          <div class="catalog-empty">
            <p>{t('channels.no_videos_channel')}</p>
          </div>
        {:else}
          <div class="channel-video-grid">
            {#each sortedNormalVideos as item, index (item.id)}
              {@const playback = player.getPlayback(item.id) ?? item.playback}
              <div class="channel-card-item">
                <div class="thumb-wrap">
                  <a
                    href={`/watch?v=${item.id}`}
                    class="thumb-link"
                    onclick={(e) => handleMediaClick(e, item)}
                  >
                    <img src={item.thumbnail_url} alt={item.title} class="thumb-img" loading={index < 12 ? 'eager' : 'lazy'} decoding={index < 8 ? 'sync' : 'async'} fetchpriority={index < 4 ? 'high' : 'auto'} />
                  </a>

                  {#if playback?.completed}
                    <span class="badge watched-badge">
                      <span class="material-symbols-outlined watched-icon">check</span>
                      <span>{t('common.watched')}</span>
                    </span>
                  {/if}

                  {#if item.duration_ms}
                    <span class="badge duration-badge">{formatDuration(item.duration_ms)}</span>
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

                <div class="channel-item-meta">
                  <a
                    href={`/watch?v=${item.id}`}
                    class="channel-item-title-link"
                    onclick={(e) => handleMediaClick(e, item)}
                  >
                    <h4 class="channel-item-title" title={item.title}>{item.title}</h4>
                  </a>
                </div>
              </div>
            {/each}
          </div>
        {/if}

      <!-- ABA 3: SHORTS (Aba dedicada no canal com filtros estilo YouTube) -->
      {:else if activeTab === 'shorts'}
        <div class="tab-filters-row">
          <div class="sort-chips">
            <button
              type="button"
              class="sort-chip"
              class:active={shortsSort === 'recent'}
              onclick={() => (shortsSort = 'recent')}
            >
              {t('channels.shorts_recent')}
            </button>
            <button
              type="button"
              class="sort-chip"
              class:active={shortsSort === 'duration'}
              onclick={() => (shortsSort = 'duration')}
            >
              {t('channels.shorts_duration')}
            </button>
            <button
              type="button"
              class="sort-chip"
              class:active={shortsSort === 'oldest'}
              onclick={() => (shortsSort = 'oldest')}
            >
              {t('channels.shorts_oldest')}
            </button>
          </div>
        </div>

        {#if sortedShorts.length === 0}
          <div class="catalog-empty">
            <p>{t('channels.no_shorts')}</p>
          </div>
        {:else}
          <div class="channel-shorts-grid">
            {#each sortedShorts as item, index (item.id)}
              <div class="channel-short-card-item">
                <div class="short-thumb-wrapper">
                  <a
                    href={`/shorts?channel=${encodeURIComponent(channelId)}&v=${item.id}`}
                    class="short-thumb-link"
                    onclick={(e) => handleShortClick(e, item)}
                  >
                    <img src={item.thumbnail_url} alt={item.title} class="short-thumb-img" loading={index < 12 ? 'eager' : 'lazy'} decoding={index < 8 ? 'sync' : 'async'} fetchpriority={index < 4 ? 'high' : 'auto'} />
                    <div class="short-hover-overlay">
                      <span class="material-symbols-outlined short-play-icon">play_arrow</span>
                    </div>
                    <span class="badge short-badge">Short</span>
                  </a>

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
                </div>

                <div class="channel-item-meta">
                  <a
                    href={`/shorts?channel=${encodeURIComponent(channelId)}&v=${item.id}`}
                    class="channel-item-title-link"
                    onclick={(e) => handleShortClick(e, item)}
                  >
                    <h4 class="channel-item-title short-title-clamp" title={item.title}>{item.title}</h4>
                  </a>
                </div>
              </div>
            {/each}
          </div>
        {/if}

      <!-- ABA 4: MENÇÕES EM OUTROS CANAIS -->
      {:else if activeTab === 'mentions'}
        {#if filteredMentions.length === 0}
          <div class="catalog-empty">
            <p>{t('channels.no_mentions')}</p>
            <small>{t('channels.no_mentions_desc', { name: channel.name })}</small>
          </div>
        {:else}
          <div class="mentions-intro">
            <span class="material-symbols-outlined">alternate_email</span>
            <p>{t('channels.mentions_desc', { name: channel.name })}</p>
          </div>
          <div class="channel-video-grid">
            {#each filteredMentions as item, index (item.id)}
              {@const playback = player.getPlayback(item.id) ?? item.playback}
              {@const sourceChannel = item.channel}
              <div class="channel-card-item">
                <div class="thumb-wrap">
                  <a href={`/watch?v=${item.id}`} class="thumb-link" onclick={(e) => handleMediaClick(e, item)}>
                    <img src={item.thumbnail_url} alt={item.title} class="thumb-img" loading={index < 12 ? 'eager' : 'lazy'} decoding={index < 8 ? 'sync' : 'async'} fetchpriority={index < 4 ? 'high' : 'auto'} />
                  </a>
                  {#if playback?.completed}
                    <span class="badge watched-badge">
                      <span class="material-symbols-outlined watched-icon">check</span>
                      <span>{t('common.watched')}</span>
                    </span>
                  {/if}
                  {#if item.duration_ms}
                    <span class="badge duration-badge">{formatDuration(item.duration_ms)}</span>
                  {/if}
                  <div class="card-top-right-actions">
                    <button
                      type="button"
                      class="card-action-btn card-playlist-btn"
                      onclick={(event) => {
                        event.stopPropagation();
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
                      onclick={(event) => {
                        event.stopPropagation();
                        void player.toggleFavorite(item);
                      }}
                      title={item.is_favorite ? t('common.unfavorite') : t('common.favorite')}
                    >
                      <span class="material-symbols-outlined">{item.is_favorite ? 'favorite' : 'favorite_border'}</span>
                    </button>
                  </div>
                </div>
                <div class="channel-item-meta">
                  <a href={`/watch?v=${item.id}`} class="channel-item-title-link" onclick={(e) => handleMediaClick(e, item)}>
                    <h4 class="channel-item-title" title={item.title}>{item.title}</h4>
                  </a>
                  {#if sourceChannel}
                    <button type="button" class="mention-source" onclick={(event) => openChannelPage(event, sourceChannel.id)}>
                      {sourceChannel.name}
                    </button>
                  {/if}
                </div>
              </div>
            {/each}
          </div>
        {/if}

      <!-- ABA 5: PLAYLISTS -->
      {:else if activeTab === 'playlists'}
        {#if channelPlaylists.length === 0}
          <div class="catalog-empty">
            <p>{t('channels.no_playlists')}</p>
          </div>
        {:else}
          <div class="channel-playlists-grid">
            {#each channelPlaylists as pl (pl.id)}
              <div class="playlist-card">
                <div class="playlist-icon-wrap">
                  <span class="material-symbols-outlined">playlist_play</span>
                </div>
                <div class="playlist-info">
                  <h4>{pl.title}</h4>
                  <p>{t('common.video_count', { count: pl.item_count })}</p>
                </div>
              </div>
            {/each}
          </div>
        {/if}
      {/if}
    </div>
  {/if}
</div>

{#if playlistModalMedia}
  <SaveToPlaylistModal
    media={playlistModalMedia}
    onClose={() => (playlistModalMedia = null)}
  />
{/if}

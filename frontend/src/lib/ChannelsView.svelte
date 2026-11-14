<script lang="ts">
  import { t } from './i18n/i18n.svelte';
  import { onMount } from 'svelte';
  import { apiFetch } from './api';
  import Button from './ui/Button.svelte';
  import PageState from './ui/PageState.svelte';

  type ChannelSummary = {
    id: string;
    name: string;
    avatar_url?: string | null;
    handle?: string | null;
    has_name_conflict: boolean;
    media_count: number;
    latest_thumbnail_url?: string | null;
  };

  let channels = $state<ChannelSummary[]>([]);
  let loading = $state(true);
  let errorMessage = $state<string | null>(null);

  let searchQuery = $state('');
  let sortBy = $state<'count_desc' | 'name_asc' | 'count_asc'>('count_desc');
  let viewMode = $state<'grid' | 'list'>('grid');

  onMount(() => {
    void loadChannels();
  });

  async function loadChannels() {
    loading = true;
    errorMessage = null;
    try {
      const res = await apiFetch('/api/channels');
      if (!res.ok) {
        throw new Error(`Erro ao carregar canais: HTTP ${res.status}`);
      }
      channels = (await res.json()) as ChannelSummary[];
    } catch (err) {
      errorMessage = err instanceof Error ? err.message : String(err);
    } finally {
      loading = false;
    }
  }

  function openChannel(channel: ChannelSummary) {
    window.history.pushState({}, '', `/canal/${channel.id}`);
    window.dispatchEvent(new PopStateEvent('popstate'));
  }

  // Gradientes determinísticos para canais sem avatar
  const GRADIENTS = [
    'linear-gradient(135deg, #f43f5e 0%, #fb7185 100%)', // Rose
    'linear-gradient(135deg, #8b5cf6 0%, #a78bfa 100%)', // Violet
    'linear-gradient(135deg, #06b6d4 0%, #22d3ee 100%)', // Cyan
    'linear-gradient(135deg, #10b981 0%, #34d399 100%)', // Emerald
    'linear-gradient(135deg, #f59e0b 0%, #fbbf24 100%)', // Amber
    'linear-gradient(135deg, #ec4899 0%, #f472b6 100%)', // Pink
    'linear-gradient(135deg, #3b82f6 0%, #60a5fa 100%)', // Blue
    'linear-gradient(135deg, #6366f1 0%, #818cf8 100%)', // Indigo
  ];

  function getChannelMonogram(name: string): string {
    if (!name) return '?';
    const clean = name.replace(/^@/, '').trim();
    return clean.charAt(0).toUpperCase();
  }

  function getChannelGradient(name: string): string {
    let hash = 0;
    for (let i = 0; i < name.length; i++) {
      hash = name.charCodeAt(i) + ((hash << 5) - hash);
    }
    const index = Math.abs(hash) % GRADIENTS.length;
    return GRADIENTS[index];
  }

  const filteredChannels = $derived.by(() => {
    let list = channels;
    const q = searchQuery.trim().toLowerCase();
    if (q) {
      list = list.filter((c) => {
        const matchesName = c.name.toLowerCase().includes(q);
        const matchesHandle = c.handle ? c.handle.toLowerCase().includes(q) : false;
        return matchesName || matchesHandle;
      });
    }

    // Ordenação
    return [...list].sort((a, b) => {
      if (sortBy === 'count_desc') {
        if (b.media_count !== a.media_count) return b.media_count - a.media_count;
        return a.name.localeCompare(b.name, 'pt-BR');
      } else if (sortBy === 'count_asc') {
        if (a.media_count !== b.media_count) return a.media_count - b.media_count;
        return a.name.localeCompare(b.name, 'pt-BR');
      } else {
        return a.name.localeCompare(b.name, 'pt-BR');
      }
    });
  });
</script>

<div class="channels-view-page">
  {#if loading}
    <PageState kind="loading" title={t('channels.loading')} />
  {:else if errorMessage}
    {#snippet retry()}<Button onclick={() => void loadChannels()}>{t('common.retry')}</Button>{/snippet}
    <PageState kind="error" icon="error" title={t('channels.empty_title')} description={errorMessage} actions={retry} />
  {:else if channels.length === 0}
    <PageState kind="empty" icon="subscriptions" title={t('channels.empty_title')} description={t('channels.empty_desc')} />
  {:else}
    <!-- Topbar com Título, Busca, Ordenação e Alternador de Modo -->
    <header class="channels-header-toolbar">
      <div class="channels-search-bar">
        <span class="material-symbols-outlined search-icon">search</span>
        <input
          type="text"
          class="channels-search-input"
          placeholder={t('channels.search_placeholder')}
          bind:value={searchQuery}
        />
        {#if searchQuery.trim()}
          <button
            type="button"
            class="search-clear-btn"
            onclick={() => (searchQuery = '')}
            title={t('common.clear_search')}
            aria-label={t('common.clear_search')}
          >
            <span class="material-symbols-outlined">close</span>
          </button>
        {/if}
      </div>

      <div class="channels-toolbar-actions">
        <!-- Chips de Ordenação -->
        <div class="channels-sort-chips">
          <button
            type="button"
            class="channels-sort-pill"
            class:active={sortBy === 'count_desc'}
            onclick={() => (sortBy = 'count_desc')}
            title={t('channels.sort_more_videos_tip')}
          >
            <span class="material-symbols-outlined sort-icon">trending_up</span>
            <span>{t('channels.sort_more_videos')}</span>
          </button>

          <button
            type="button"
            class="channels-sort-pill"
            class:active={sortBy === 'name_asc'}
            onclick={() => (sortBy = 'name_asc')}
            title={t('channels.sort_alpha_tip')}
          >
            <span class="material-symbols-outlined sort-icon">sort_by_alpha</span>
            <span>A - Z</span>
          </button>

          <button
            type="button"
            class="channels-sort-pill"
            class:active={sortBy === 'count_asc'}
            onclick={() => (sortBy = 'count_asc')}
            title={t('channels.sort_fewer_videos_tip')}
          >
            <span class="material-symbols-outlined sort-icon">filter_list</span>
            <span>{t('channels.sort_fewer_videos')}</span>
          </button>
        </div>

        <!-- Toggle Grade / Lista -->
        <div class="channels-view-toggle">
          <button
            type="button"
            class="toggle-mode-btn"
            class:active={viewMode === 'grid'}
            onclick={() => (viewMode = 'grid')}
            title={t('channels.grid_view')}
            aria-label={t('channels.grid_view')}
          >
            <span class="material-symbols-outlined">grid_view</span>
          </button>
          <button
            type="button"
            class="toggle-mode-btn"
            class:active={viewMode === 'list'}
            onclick={() => (viewMode = 'list')}
            title={t('channels.list_view')}
            aria-label={t('channels.list_view')}
          >
            <span class="material-symbols-outlined">view_list</span>
          </button>
        </div>
      </div>
    </header>

    {#if filteredChannels.length === 0}
      {#snippet clearSearch()}<Button onclick={() => (searchQuery = '')}>{t('search.clear_search')}</Button>{/snippet}
      <PageState kind="empty" icon="search_off" title={t('channels.empty_title')} description={t('channels.no_videos_channel')} actions={clearSearch} />
    {:else if viewMode === 'grid'}
      <!-- Visualização em GRADE MODERNA (Cards com Banner e Avatar Proeminente) -->
      <div class="channels-cards-grid">
        {#each filteredChannels as channel, index (channel.id)}
          <div
            class="channel-rich-card"
            role="button"
            tabindex="0"
            onclick={() => openChannel(channel)}
            onkeydown={(e) => e.key === 'Enter' && openChannel(channel)}
          >
            <!-- Banner do card (preview do último vídeo ou gradiente dinâmico) -->
            <div class="channel-card-banner">
              {#if channel.latest_thumbnail_url}
                <img
                  src={channel.latest_thumbnail_url}
                  alt=""
                  class="channel-banner-img"
                  loading={index < 8 ? 'eager' : 'lazy'}
                  decoding={index < 6 ? 'sync' : 'async'}
                  fetchpriority={index < 3 ? 'high' : 'auto'}
                />
              {/if}
              <div class="channel-banner-glow"></div>
            </div>

            <!-- Corpo do Card -->
            <div class="channel-card-body">
              <!-- Avatar sobrepondo o banner -->
              <div class="channel-card-avatar-wrap">
                <div
                  class="channel-rich-monogram"
                  style="background: {getChannelGradient(channel.name)}"
                >
                  <span>{getChannelMonogram(channel.name)}</span>
                </div>
                {#if channel.avatar_url}
                  <img
                    src={channel.avatar_url}
                    alt={channel.name}
                    class="channel-rich-avatar-img"
                    loading={index < 8 ? 'eager' : 'lazy'}
                    decoding={index < 6 ? 'sync' : 'async'}
                    onerror={(e) => {
                      (e.currentTarget as HTMLImageElement).style.display = 'none';
                    }}
                  />
                {/if}
              </div>

              <!-- Informações do canal -->
              <div class="channel-card-details">
                <div class="channel-name-row">
                  <h3 class="channel-rich-name" title={channel.name}>{channel.name}</h3>
                  {#if channel.handle}
                    <span class="channel-verified-badge material-symbols-outlined" title={t('channels.synced_badge_tip')}>
                      check_circle
                    </span>
                  {/if}
                </div>

                {#if channel.handle}
                  <span class="channel-rich-handle">{channel.handle}</span>
                {/if}

                <div class="channel-rich-badge-row">
                  <span class="channel-pill-count">
                    <span class="material-symbols-outlined count-icon">video_library</span>
                    {t('common.video_count', { count: channel.media_count })}
                  </span>
                </div>
              </div>

              <!-- Botão Acessar Canal -->
              <div class="channel-card-action-footer">
                <span class="access-channel-text">{t('channels.access_channel')}</span>
                <span class="material-symbols-outlined access-arrow">arrow_forward</span>
              </div>
            </div>
          </div>
        {/each}
      </div>
    {:else}
      <!-- Visualização em LISTA COMPACTA (Estilo Gerenciador de Inscrições do YouTube) -->
      <div class="channels-compact-list">
        {#each filteredChannels as channel, index (channel.id)}
          <div
            class="channel-compact-row"
            role="button"
            tabindex="0"
            onclick={() => openChannel(channel)}
            onkeydown={(e) => e.key === 'Enter' && openChannel(channel)}
          >
            <div class="compact-avatar-wrap">
              <div
                class="compact-monogram"
                style="background: {getChannelGradient(channel.name)}"
              >
                <span>{getChannelMonogram(channel.name)}</span>
              </div>
              {#if channel.avatar_url}
                <img
                  src={channel.avatar_url}
                  alt={channel.name}
                  class="compact-avatar-img"
                  loading={index < 12 ? 'eager' : 'lazy'}
                  decoding={index < 8 ? 'sync' : 'async'}
                  fetchpriority={index < 4 ? 'high' : 'auto'}
                  onerror={(e) => {
                    (e.currentTarget as HTMLImageElement).style.display = 'none';
                  }}
                />
              {/if}
            </div>

            <div class="compact-info-stack">
              <div class="compact-name-line">
                <h3 class="compact-channel-name">{channel.name}</h3>
                {#if channel.handle}
                  <span class="material-symbols-outlined compact-verified-icon">check_circle</span>
                {/if}
              </div>
              {#if channel.handle}
                <span class="compact-handle">{channel.handle}</span>
              {/if}
            </div>

            <div class="compact-count-pill">
              {t('common.video_count', { count: channel.media_count })}
            </div>

            <button type="button" class="compact-enter-btn" aria-label={t('channels.access_channel_aria', { name: channel.name })}>
              <span>{t('channels.access_channel')}</span>
              <span class="material-symbols-outlined icon-16">arrow_forward</span>
            </button>
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>

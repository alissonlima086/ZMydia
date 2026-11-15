<script lang="ts">
  import { onMount } from 'svelte';
  import { apiFetch, apiJson, getCsrfToken } from './lib/api';
  import { authStore } from './lib/auth.svelte';
  import LoginView from './lib/LoginView.svelte';
  import SetupView from './lib/SetupView.svelte';
  import { musicStore } from './lib/music.svelte';
  import ApiStatus from './lib/ApiStatus.svelte';
  import MediaCatalog from './lib/MediaCatalog.svelte';
  import SearchBar from './lib/SearchBar.svelte';
  import VideoModal from './lib/VideoModal.svelte';
  import DownloadsPopover from './lib/DownloadsPopover.svelte';
  import LazyPage from './lib/ui/LazyPage.svelte';
  import ConfirmDialog from './lib/ui/ConfirmDialog.svelte';
  import type { UpdateStatus } from './lib/settings/types';
  import { player, type MediaItem } from './lib/player.svelte';
  import { t } from './lib/i18n/i18n.svelte';
  import {
    extractVideoId,
    isNavigationPathActive,
    parseChannelRoute,
    pushRoute
  } from './lib/router';

  const loadChannels = () => import('./lib/ChannelsView.svelte');
  const loadChannelDetail = () => import('./lib/ChannelDetailView.svelte');
  const loadFavorites = () => import('./lib/FavoritesView.svelte');
  const loadHistory = () => import('./lib/HistoryView.svelte');
  const loadPlaylists = () => import('./lib/PlaylistsView.svelte');
  const loadSearch = () => import('./lib/SearchView.svelte');
  const loadShorts = () => import('./lib/ShortsView.svelte');
  const loadSources = () => import('./lib/SourcesPanel.svelte');
  const loadStats = () => import('./lib/StatsView.svelte');
  const uiCatalogModule = import.meta.env.DEV && window.location.pathname === '/dev/ui'
    ? import('./lib/dev/UiCatalog.svelte')
    : null;

  type Page = {
    path: string;
    label: string;
    icon: string;
    eyebrow: string;
    title: string;
    description: string;
  };

  type NavSection = {
    title?: string;
    items: Page[];
  };

  const navSections = $derived<NavSection[]>([
    {
      items: [
        { path: '/', label: t('nav.home'), icon: 'home', eyebrow: t('nav.home_eyebrow'), title: t('nav.home_title'), description: t('nav.home_desc') },
        { path: '/shorts', label: t('nav.shorts'), icon: 'smartphone', eyebrow: t('nav.shorts_eyebrow'), title: t('nav.shorts_title'), description: t('nav.shorts_desc') },
        { path: '/explorar', label: t('nav.explore'), icon: 'search', eyebrow: t('nav.explore_eyebrow'), title: t('nav.explore_title'), description: t('nav.explore_desc') },
        { path: '/canais', label: t('nav.channels'), icon: 'subscriptions', eyebrow: t('nav.channels_eyebrow'), title: t('nav.channels_title'), description: t('nav.channels_desc') }
      ]
    },
    {
      title: t('nav.section_you'),
      items: [
        { path: '/historico', label: t('nav.history'), icon: 'history', eyebrow: t('nav.history_eyebrow'), title: t('nav.history_title'), description: t('nav.history_desc') },
        { path: '/favoritos', label: t('nav.favorites'), icon: 'favorite', eyebrow: t('nav.favorites_eyebrow'), title: t('nav.favorites_title'), description: t('nav.favorites_desc') },
        { path: '/playlists', label: t('nav.playlists'), icon: 'playlist_play', eyebrow: t('nav.playlists_eyebrow'), title: t('nav.playlists_title'), description: t('nav.playlists_desc') },
        { path: '/stats', label: t('nav.stats'), icon: 'query_stats', eyebrow: t('nav.stats_eyebrow'), title: t('nav.stats_title'), description: t('nav.stats_desc') },
        { path: '/biblioteca', label: t('nav.library'), icon: 'video_library', eyebrow: t('nav.library_eyebrow'), title: t('nav.library_title'), description: t('nav.library_desc') }
      ]
    },
    {
      items: [
        { path: '/configuracoes', label: t('nav.settings'), icon: 'settings', eyebrow: t('nav.settings_eyebrow'), title: t('nav.settings_title'), description: t('nav.settings_desc') }
      ]
    }
  ]);

  let currentPath = $state(window.location.pathname);
  let searchQuery = $state('');
  let homeKey = $state(0);
  let availableUpdate = $state<UpdateStatus | null>(null);
  let automaticUpdateCheckStarted = false;

  const AUTO_UPDATE_CHECK_INTERVAL_SECONDS = 24 * 60 * 60;

  async function checkUpdatesOnEntry() {
    if (automaticUpdateCheckStarted) return;
    automaticUpdateCheckStarted = true;
    try {
      let status = await apiJson<UpdateStatus>('/api/updates/status');
      const now = Math.floor(Date.now() / 1000);
      const checkIsStale = !status.last_checked_at
        || now - status.last_checked_at >= AUTO_UPDATE_CHECK_INTERVAL_SECONDS;
      const canRefresh = !['downloaded', 'applying'].includes(status.phase)
        && status.installation_method !== 'development';
      if (checkIsStale && canRefresh) {
        try {
          status = await apiJson<UpdateStatus>('/api/updates/check', { method: 'POST' });
        } catch {
          // Mantém o último estado conhecido caso a consulta automática falhe.
        }
      }
      const version = status.available_version;
      const canNotify = version
        && ['available', 'downloaded'].includes(status.phase)
        && sessionStorage.getItem(`zmydia:update-notified:${version}`) !== '1';
      if (canNotify && version) {
        sessionStorage.setItem(`zmydia:update-notified:${version}`, '1');
        availableUpdate = status;
      }
    } catch {
      // A verificação automática nunca bloqueia a entrada na aplicação.
    }
  }

  function openAvailableUpdate() {
    availableUpdate = null;
    selectSettingsTab('dashboard');
  }

  function routeTitle(path: string): string {
    if (path === '/') return t('nav.home');
    if (path === '/shorts') return t('nav.shorts');
    if (path === '/explorar') return t('nav.explore');
    if (path === '/canais' || path.startsWith('/canal/')) return t('nav.channels');
    if (path === '/historico') return t('nav.history');
    if (path === '/favoritos') return t('nav.favorites');
    if (path === '/playlists' || path.startsWith('/playlist')) return t('nav.playlists');
    if (path === '/stats') return t('nav.stats');
    if (path === '/biblioteca' || path === '/videos') return t('nav.library');
    if (path === '/configuracoes') return t('nav.settings');
    return t('nav.home');
  }

  function getSettingsTabFromUrl(): string {
    if (typeof window === 'undefined') return 'dashboard';
    const tab = new URLSearchParams(window.location.search).get('tab');
    const valid = ['dashboard', 'conta', 'idioma', 'pastas', 'youtube', 'legendas', 'feed', 'extensao', 'listenbrainz'];
    return tab && valid.includes(tab) ? tab : 'dashboard';
  }

  let settingsTab = $state(getSettingsTabFromUrl());

  function selectSettingsTab(tabId: string) {
    settingsTab = tabId;
    searchQuery = '';
    const url = new URL(window.location.href);
    url.pathname = '/configuracoes';
    url.searchParams.set('tab', tabId);
    window.history.pushState({}, '', url.toString());
    currentPath = '/configuracoes';
  }

  function extractVideoIdFromLocation(): number | null {
    if (typeof window === 'undefined') return null;
    return extractVideoId(window.location.href);
  }

  async function handleUrlRouting() {
    const videoId = extractVideoIdFromLocation();
    const url = new URL(window.location.href);
    const listParam = url.searchParams.get('list');

    if (listParam && (!player.playlistContext || player.playlistContext.id !== listParam)) {
      try {
        const res = await apiFetch(`/api/playlists/${listParam}`);
        if (res.ok) {
          const playlist = await res.json();
          if (playlist.items && playlist.items.length > 0) {
            const startIdx = videoId ? playlist.items.findIndex((it: MediaItem) => it.id === videoId) : 0;
            void player.playPlaylist(playlist, Math.max(0, startIdx));
            return;
          }
        }
      } catch {}
    }

    if (videoId !== null) {
      if (player.currentMedia?.id === videoId) {
        player.isExpanded = true;
      } else {
        try {
          const res = await apiFetch(`/api/media/${videoId}`);
          if (res.ok) {
            const media = (await res.json()) as MediaItem;
            void player.playMedia(media, true, false);
          }
        } catch {
          // Ignora erro se vídeo não puder ser carregado
        }
      }
    } else if (player.isExpanded) {
      player.closeExpanded(false);
    }
  }

  function handlePopState() {
    currentPath = window.location.pathname;
    if (currentPath !== '/explorar') {
      searchQuery = '';
    }
    if (currentPath === '/configuracoes') {
      settingsTab = getSettingsTabFromUrl();
    }
    if (currentPath === '/explorar') {
      searchQuery = new URLSearchParams(window.location.search).get('q') ?? '';
    }
    void handleUrlRouting();
  }

  function navigate(event: MouseEvent, path: string) {
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    if (player.isExpanded) {
      player.closeExpanded(false);
    }
    currentPath = pushRoute(path);
    if (currentPath !== '/explorar') {
      searchQuery = '';
    }
    if (currentPath === '/configuracoes') {
      settingsTab = getSettingsTabFromUrl();
    }
    if (path === '/') {
      resetHome();
      return;
    }
  }

  function goTo(path: string) {
    currentPath = pushRoute(path);
    if (currentPath !== '/explorar') {
      searchQuery = '';
    }
  }

  function resetHome(event?: MouseEvent) {
    if (event) {
      if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
      event.preventDefault();
    }
    if (player.isExpanded) {
      player.closeExpanded(false);
    }
    currentPath = pushRoute('/');
    searchQuery = '';
    homeKey += 1;
  }

  function handleKeydown(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
      event.preventDefault();
      const input = document.querySelector('.search-bar-input') as HTMLInputElement | null;
      if (input) {
        input.focus();
        input.select();
      }
    }
  }

  onMount(() => {
    void getCsrfToken();
    void musicStore.init();
    void (async () => {
      const isAuth = await authStore.checkStatus();
      if (isAuth) {
        await handleUrlRouting();
      }
    })();

    const onRoute = () => {
      currentPath = window.location.pathname;
      if (currentPath !== '/explorar') {
        searchQuery = '';
      }
    };
    window.addEventListener('routechange', onRoute);
    return () => {
      window.removeEventListener('routechange', onRoute);
    };
  });

  let previousAuth = false;
  $effect(() => {
    if (authStore.authenticated && !previousAuth) {
      previousAuth = true;
      void musicStore.loadSettings();
      void handleUrlRouting();
      void checkUpdatesOnEntry();
    } else if (!authStore.authenticated) {
      previousAuth = false;
      automaticUpdateCheckStarted = false;
    }
  });

  $effect(() => {
    if (currentPath === '/shorts') {
      player.pauseForExclusivePlayback();
    }

    if (!authStore.authenticated) {
      document.title = `ZMydia | ${authStore.setupRequired ? t('auth.create_admin') : t('auth.sign_in_title')}`;
    } else if (currentPath !== '/shorts' && player.currentMedia && (player.isExpanded || player.isPlaying)) {
      document.title = `${player.currentMedia.title} | ZMydia`;
    } else {
      document.title = `ZMydia | ${routeTitle(currentPath)}`;
    }
  });
</script>

<svelte:window onpopstate={handlePopState} onkeydown={handleKeydown} />

{#if uiCatalogModule}
  {#await uiCatalogModule}
    <div class="auth-loading-screen">
      <span class="material-symbols-outlined spin auth-loading-icon">progress_activity</span>
    </div>
  {:then module}
    {@const UiCatalog = module.default}
    <UiCatalog />
  {/await}
{:else if authStore.loading}
  <div class="auth-loading-screen">
    <span class="material-symbols-outlined spin auth-loading-icon">progress_activity</span>
  </div>
{:else if !authStore.authenticated}
  {#if authStore.setupRequired || authStore.recoveryCode}
    <SetupView />
  {:else}
    <LoginView />
  {/if}
{:else}
  <div class="app-shell">
  <aside class="sidebar">
    <a class="brand" href="/" onclick={(event) => resetHome(event)} title={t('nav.back_to_home')}>
      <span class="brand-mark">Z</span>
      <span><strong>ZMydia</strong><small>{t('nav.personal_library')}</small></span>
    </a>

    <nav aria-label={t('nav.main_nav')}>
      {#each navSections as section, i}
        {#if i > 0}
          <div class="nav-divider"></div>
        {/if}
        {#if section.title}
          <div class="nav-section-title">
            <span>{section.title}</span>
            <span class="material-symbols-outlined nav-section-chevron">chevron_right</span>
          </div>
        {/if}
        {#each section.items as item}
          <a
            href={item.path}
            class:active={isNavigationPathActive(currentPath, item.path)}
            aria-current={isNavigationPathActive(currentPath, item.path) ? 'page' : undefined}
            onclick={(event) => navigate(event, item.path)}
          >
            <span class="material-symbols-outlined nav-icon" aria-hidden="true">{item.icon}</span>
            {item.label}
          </a>

        {/each}
      {/each}
    </nav>

    <ApiStatus />
  </aside>

  <div class="main-column">
    <header class="topbar">
      <div class="topbar-search-wrap">
        <SearchBar
          initialQuery={searchQuery}
          onSearchSubmit={(q) => (searchQuery = q)}
          onSelectVideo={() => (searchQuery = '')}
        />
      </div>

      <div class="topbar-actions">
        <DownloadsPopover />
        {#if authStore.username}
          <div class="user-profile-badge">
            <button
              type="button"
              class="user-profile-btn"
              onclick={() => selectSettingsTab('conta')}
              title={t('common.manage_account')}
              aria-label={t('common.manage_account')}
            >
              <span class="material-symbols-outlined user-avatar-icon">account_circle</span>
              <span class="username-text">{authStore.username}</span>
            </button>
            <button
              type="button"
              class="logout-btn"
              onclick={() => authStore.logout()}
              title={t('common.sign_out')}
              aria-label={t('common.sign_out')}
            >
              <span class="material-symbols-outlined logout-icon">logout</span>
            </button>
          </div>
        {/if}
      </div>
    </header>

    <main>
      {#if currentPath === '/configuracoes'}
        <LazyPage loader={loadSources} props={{ activeTab: settingsTab, onTabChange: (tab: string) => selectSettingsTab(tab) }} />
      {:else if currentPath === '/' || currentPath === '/watch' || currentPath.startsWith('/watch/') || currentPath.startsWith('/video/')}
        {#key homeKey}
          <MediaCatalog isFeed={true} isShort={false} />
        {/key}
      {:else if currentPath === '/shorts'}
        <LazyPage loader={loadShorts} />
      {:else if currentPath === '/historico'}
        <LazyPage loader={loadHistory} />
      {:else if currentPath === '/favoritos'}
        <LazyPage loader={loadFavorites} />
      {:else if currentPath === '/explorar'}
        <LazyPage loader={loadSearch} props={{ externalQuery: searchQuery, onQueryChange: (query: string) => (searchQuery = query) }} />
      {:else if currentPath === '/canais'}
        <LazyPage loader={loadChannels} />
      {:else if currentPath.startsWith('/canal/')}
        {@const channelRoute = parseChannelRoute(currentPath)}
        {#if channelRoute}
          {#key channelRoute.channelId}
            <LazyPage
              loader={loadChannelDetail}
              props={{ channelId: channelRoute.channelId, initialTab: channelRoute.tab, onBack: () => goTo('/canais') }}
            />
          {/key}
        {/if}
      {:else if currentPath === '/playlists' || currentPath.startsWith('/playlists/') || currentPath.startsWith('/playlist/')}
        <LazyPage loader={loadPlaylists} />
      {:else if currentPath === '/stats'}
        <LazyPage loader={loadStats} />
      {:else if currentPath === '/biblioteca' || currentPath === '/videos'}
        <MediaCatalog isFeed={false} />
      {:else}
        <section class="empty-state">
          <div class="empty-art" aria-hidden="true">
            <span class="film-frame"></span>
            <span class="material-symbols-outlined play-symbol">play_arrow</span>
          </div>
          <h2>{t('app.ready_title')}</h2>
          <p>{t('app.ready_desc')}</p>
          <a href="/configuracoes" class="primary-action" onclick={(event) => navigate(event, '/configuracoes')}>
            {t('nav.settings')}
            <span class="material-symbols-outlined primary-action-icon">arrow_forward</span>
          </a>
        </section>
      {/if}
    </main>

    <VideoModal hidden={currentPath === '/shorts'} />
    <ConfirmDialog
      open={availableUpdate !== null}
      title={t('updates.popup_title')}
      description={t('updates.popup_description', { version: availableUpdate?.available_version ?? '' })}
      confirmLabel={t('updates.popup_action')}
      cancelLabel={t('updates.popup_later')}
      onConfirm={openAvailableUpdate}
      onCancel={() => (availableUpdate = null)}
    />
  </div>
</div>
{/if}

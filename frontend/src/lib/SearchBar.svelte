<script lang="ts">
  import { apiFetch } from './api';
  import { player, type MediaItem } from './player.svelte';
  import { t } from './i18n/i18n.svelte';
  import { selectChannelSuggestions, type ChannelSuggestion } from './searchSuggestions';

  interface Props {
    placeholder?: string;
    initialQuery?: string;
    onSearchSubmit?: (query: string) => void;
    onSelectVideo?: (item: MediaItem) => void;
  }

  let {
    placeholder,
    initialQuery = '',
    onSearchSubmit,
    onSelectVideo
  }: Props = $props();

  let effectivePlaceholder = $derived(placeholder ?? t('search.search_library_placeholder'));

  let query = $state('');
  let syncedInitialQuery = $state<string | undefined>(undefined);
  let suggestions = $state<MediaItem[]>([]);
  let channelSuggestions = $state<ChannelSuggestion[]>([]);
  let isOpen = $state(false);
  let isLoading = $state(false);
  let selectedIndex = $state(-1);
  let totalSuggestions = $derived(suggestions.length + channelSuggestions.length);

  let containerEl = $state<HTMLElement | null>(null);
  let inputEl = $state<HTMLInputElement | null>(null);

  let debounceTimer: number | null = null;
  let currentFetchController: AbortController | null = null;

  $effect(() => {
    if (initialQuery !== syncedInitialQuery) {
      syncedInitialQuery = initialQuery;
      query = initialQuery;
      suggestions = [];
      channelSuggestions = [];
      isOpen = false;
      isLoading = false;
      selectedIndex = -1;
      if (debounceTimer) clearTimeout(debounceTimer);
      if (currentFetchController) currentFetchController.abort();
    }
  });

  // Fecha as sugestões ao clicar fora do componente
  $effect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (containerEl && !containerEl.contains(event.target as Node)) {
        isOpen = false;
      }
    }
    document.addEventListener('click', handleClickOutside);
    return () => {
      document.removeEventListener('click', handleClickOutside);
    };
  });

  // Atalho global Ctrl+K / Cmd+K para focar a barra de pesquisa
  $effect(() => {
    function handleGlobalKeys(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
        const activeEl = document.activeElement;
        if (activeEl === inputEl) {
          inputEl?.select();
          return;
        }
        if (inputEl && inputEl.offsetParent !== null) {
          event.preventDefault();
          inputEl.focus();
          inputEl.select();
        }
      }
    }
    window.addEventListener('keydown', handleGlobalKeys);
    return () => {
      window.removeEventListener('keydown', handleGlobalKeys);
    };
  });

  function handleInput(e: Event) {
    const val = (e.currentTarget as HTMLInputElement).value;
    query = val;

    if (debounceTimer) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }

    if (currentFetchController) {
      currentFetchController.abort();
      currentFetchController = null;
    }

    const trimmed = val.trim();
    if (!trimmed) {
      suggestions = [];
      channelSuggestions = [];
      isOpen = false;
      isLoading = false;
      return;
    }

    isLoading = true;
    debounceTimer = window.setTimeout(async () => {
      const controller = new AbortController();
      currentFetchController = controller;
      try {
        const [videoRes, channelRes, settingsRes] = await Promise.all([
          apiFetch(`/api/search?q=${encodeURIComponent(trimmed)}&limit=5&exclude_home_hidden=true`, {
            signal: controller.signal
          }),
          apiFetch(`/api/channels?q=${encodeURIComponent(trimmed)}&limit=3&exclude_home_hidden=true`, {
            signal: controller.signal
          }),
          apiFetch('/api/settings/home', {
            signal: controller.signal
          })
        ]);
        if (!videoRes.ok || !channelRes.ok) throw new Error('Search failed');
        const [videoData, channelData, settingsData] = await Promise.all([
          videoRes.json() as Promise<MediaItem[]>,
          channelRes.json() as Promise<ChannelSuggestion[]>,
          settingsRes.ok
            ? (settingsRes.json() as Promise<{ hidden_tags?: string[] }>)
            : Promise.resolve({ hidden_tags: [] })
        ]);
        if (query.trim() === trimmed) {
          suggestions = videoData;
          channelSuggestions = selectChannelSuggestions(
            channelData,
            trimmed,
            3,
            settingsData.hidden_tags ?? []
          );
          isOpen = true;
          selectedIndex = -1;
        }
      } catch (err: any) {
        if (err.name !== 'AbortError') {
          suggestions = [];
          channelSuggestions = [];
        }
      } finally {
        if (query.trim() === trimmed) {
          isLoading = false;
        }
      }
    }, 180);
  }

  function handleFocus() {
    if (query.trim() && totalSuggestions > 0) {
      isOpen = true;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    const maxOptions = totalSuggestions + (query.trim() ? 1 : 0);

    if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (!isOpen && query.trim()) {
        isOpen = true;
        selectedIndex = 0;
      } else if (maxOptions > 0) {
        selectedIndex = (selectedIndex + 1) % maxOptions;
      }
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (!isOpen && query.trim()) {
        isOpen = true;
        selectedIndex = maxOptions - 1;
      } else if (maxOptions > 0) {
        selectedIndex = (selectedIndex - 1 + maxOptions) % maxOptions;
      }
    } else if (e.key === 'Escape') {
      e.preventDefault();
      isOpen = false;
      inputEl?.blur();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (selectedIndex >= 0 && selectedIndex < suggestions.length) {
        selectVideo(suggestions[selectedIndex]);
      } else if (selectedIndex >= suggestions.length && selectedIndex < totalSuggestions) {
        selectChannel(channelSuggestions[selectedIndex - suggestions.length]);
      } else {
        submitSearch();
      }
    }
  }

  function clearQuery() {
    query = '';
    suggestions = [];
    channelSuggestions = [];
    isOpen = false;
    isLoading = false;
    if (debounceTimer) clearTimeout(debounceTimer);
    if (currentFetchController) currentFetchController.abort();
    inputEl?.focus();
  }

  function selectVideo(item: MediaItem) {
    query = '';
    suggestions = [];
    channelSuggestions = [];
    isOpen = false;
    isLoading = false;
    if (debounceTimer) clearTimeout(debounceTimer);
    if (currentFetchController) currentFetchController.abort();
    inputEl?.blur();
    player.playlistContext = null;
    void player.playMedia(item, true);
    if (onSelectVideo) onSelectVideo(item);
  }

  function selectChannel(channel: ChannelSuggestion) {
    query = '';
    suggestions = [];
    channelSuggestions = [];
    isOpen = false;
    isLoading = false;
    if (debounceTimer) clearTimeout(debounceTimer);
    if (currentFetchController) currentFetchController.abort();
    inputEl?.blur();

    if (player.isExpanded) {
      player.closeExpanded(false);
    }

    history.pushState({}, '', `/canal/${encodeURIComponent(channel.id)}`);
    window.dispatchEvent(new PopStateEvent('popstate'));
  }

  function submitSearch() {
    const trimmed = query.trim();
    if (!trimmed) return;
    isOpen = false;
    inputEl?.blur();

    if (onSearchSubmit) {
      onSearchSubmit(trimmed);
    }

    if (player.isExpanded) {
      player.closeExpanded(false);
    }

    const target = `/explorar?q=${encodeURIComponent(trimmed)}`;
    history.pushState({}, '', target);
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
</script>

<div bind:this={containerEl} class="search-bar-wrapper">
  <form class="search-bar-form" onsubmit={(e) => { e.preventDefault(); submitSearch(); }}>
    <span class="material-symbols-outlined search-bar-icon" aria-hidden="true">search</span>

    <input
      bind:this={inputEl}
      type="search"
      class="search-bar-input"
      placeholder={effectivePlaceholder}
      value={query}
      oninput={handleInput}
      onfocus={handleFocus}
      onkeydown={handleKeydown}
      autocomplete="off"
      spellcheck="false"
      aria-label={effectivePlaceholder}
      aria-autocomplete="list"
    />

    {#if isLoading}
      <div class="search-spinner" aria-hidden="true"></div>
    {:else if query}
      <button
        type="button"
        class="search-clear-btn"
        onclick={clearQuery}
        title={t('search.clear_search')}
        aria-label={t('search.clear_search')}
      >
        <span class="material-symbols-outlined">close</span>
      </button>
    {/if}

    <button
      type="submit"
      class="search-submit-btn"
      title={t('common.search')}
      aria-label={t('common.search')}
    >
      <span class="material-symbols-outlined">search</span>
    </button>
  </form>

  <!-- Dropdown de Sugestões Inteligentes -->
  {#if isOpen && (totalSuggestions > 0 || (query.trim() && !isLoading))}
    <div class="search-suggestions-popover" role="listbox">
      {#if suggestions.length > 0}
        <div class="suggestions-header">
          <span>{t('search.video_suggestions')}</span>
        </div>

        {#each suggestions as item, idx (item.id)}
          {@const isSelected = selectedIndex === idx}
          <div
            class="suggestion-item"
            class:selected={isSelected}
            role="button"
            tabindex="0"
            onclick={() => selectVideo(item)}
            onmouseenter={() => (selectedIndex = idx)}
            onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && selectVideo(item)}
          >
            <!-- Miniatura do vídeo -->
            <div class="suggestion-thumb-wrap">
              {#if item.thumbnail_url}
                <img src={item.thumbnail_url} alt="" class="suggestion-thumb-img" loading="eager" decoding="sync" fetchpriority="high" />
              {:else}
                <div class="suggestion-thumb-fallback">
                  <span class="material-symbols-outlined">movie</span>
                </div>
              {/if}
              {#if item.duration_ms}
                <span class="suggestion-thumb-duration">{formatDuration(item.duration_ms)}</span>
              {/if}
            </div>

            <!-- Dados do vídeo -->
            <div class="suggestion-info">
              <span class="suggestion-title" title={item.title}>
                {item.title}
              </span>
              <div class="suggestion-meta">
                {#if item.channel}
                  <span class="suggestion-channel" title={item.channel.name}>{item.channel.name}</span>
                {/if}
                {#if item.is_short}
                  <span class="suggestion-short-badge">Short</span>
                {/if}
              </div>
            </div>

            <!-- Ícone de ação rápida (play) -->
            <div class="suggestion-action-icon" aria-hidden="true">
              <span class="material-symbols-outlined">play_circle</span>
            </div>
          </div>
        {/each}
      {/if}

      {#if channelSuggestions.length > 0}
        <div class="suggestions-header channel-suggestions-header">
          <span>{t('search.channel_suggestions')}</span>
        </div>

        {#each channelSuggestions as channel, channelIdx (channel.id)}
          {@const optionIndex = suggestions.length + channelIdx}
          {@const isSelected = selectedIndex === optionIndex}
          <div
            class="suggestion-item channel-suggestion-item"
            class:selected={isSelected}
            role="button"
            tabindex="0"
            onclick={() => selectChannel(channel)}
            onmouseenter={() => (selectedIndex = optionIndex)}
            onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && selectChannel(channel)}
          >
            <div class="suggestion-channel-avatar">
              <span>{channel.name.replace(/^@/, '').trim().charAt(0).toUpperCase() || '?'}</span>
              {#if channel.avatar_url}
                <img src={channel.avatar_url} alt="" loading="eager" decoding="async" />
              {/if}
            </div>

            <div class="suggestion-info">
              <span class="suggestion-title" title={channel.name}>{channel.name}</span>
              <div class="suggestion-meta">
                {#if channel.handle}<span>{channel.handle}</span>{/if}
                <span>{t('channels.videos_count', { count: channel.media_count })}</span>
              </div>
            </div>

            <div class="suggestion-action-icon" aria-hidden="true">
              <span class="material-symbols-outlined">arrow_forward</span>
            </div>
          </div>
        {/each}
      {/if}

      <!-- Opção de pesquisar termo completo no Explorar -->
      {#if query.trim()}
        <div
          class="suggestion-full-search"
          class:selected={selectedIndex === totalSuggestions}
          onclick={submitSearch}
          onmouseenter={() => (selectedIndex = totalSuggestions)}
          role="button"
          tabindex="0"
          onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && submitSearch()}
        >
          <span class="material-symbols-outlined search-action-icon">travel_explore</span>
          <span class="search-action-text">
            {t('search.view_all_results', { query: query.trim() })}
          </span>
          <span class="search-action-hint">Enter ↵</span>
        </div>
      {/if}
    </div>
  {/if}
</div>

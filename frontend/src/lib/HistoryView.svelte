<script lang="ts">
  import { onMount } from 'svelte';
  import { apiFetch } from './api';
  import { player, type MediaItem } from './player.svelte';
  import PageHeader from './ui/PageHeader.svelte';
  import Button from './ui/Button.svelte';
  import PageState from './ui/PageState.svelte';
  import { t } from './i18n/i18n.svelte';

  type HistoryItem = MediaItem & {
    watched_at: string;
  };

  type DayGroup = {
    dayLabel: string;
    items: HistoryItem[];
  };

  let historyItems = $state<HistoryItem[]>([]);
  let loading = $state(true);
  let errorMessage = $state('');
  let searchQuery = $state('');
  let showConfirmClear = $state(false);
  let isClearing = $state(false);

  async function loadHistory() {
    loading = true;
    errorMessage = '';
    try {
      const res = await apiFetch('/api/history');
      if (!res.ok) {
        throw new Error(`Falha ao carregar histórico (${res.status})`);
      }
      historyItems = (await res.json()) as HistoryItem[];
    } catch (err) {
      errorMessage = err instanceof Error ? err.message : String(err);
    } finally {
      loading = false;
    }
  }

  function formatDayLabel(dateStr: string): string {
    const d = new Date(dateStr);
    if (isNaN(d.getTime())) return t('history.older');
    const now = new Date();

    const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
    const startOfYesterday = startOfToday - 86400000;
    const itemDate = new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();

    if (itemDate >= startOfToday) {
      return t('history.today');
    } else if (itemDate >= startOfYesterday) {
      return t('history.yesterday');
    } else {
      const months = [
        'janeiro', 'fevereiro', 'março', 'abril', 'maio', 'junho',
        'julho', 'agosto', 'setembro', 'outubro', 'novembro', 'dezembro'
      ];
      if (d.getFullYear() === now.getFullYear()) {
        return `${d.getDate()} de ${months[d.getMonth()]}`;
      }
      return `${d.getDate()} de ${months[d.getMonth()]} de ${d.getFullYear()}`;
    }
  }

  const filteredItems = $derived.by(() => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return historyItems;
    return historyItems.filter((item) => {
      const titleMatch = item.title.toLowerCase().includes(q);
      const channelMatch = item.channel?.name.toLowerCase().includes(q) ?? false;
      const descMatch = item.description?.toLowerCase().includes(q) ?? false;
      const tagMatch = item.tags.some((t) => t.display_name.toLowerCase().includes(q));
      return titleMatch || channelMatch || descMatch || tagMatch;
    });
  });

  const groupedHistory = $derived.by(() => {
    const groups: DayGroup[] = [];
    const map = new Map<string, HistoryItem[]>();

    for (const item of filteredItems) {
      const label = formatDayLabel(item.watched_at);
      if (!map.has(label)) {
        const list: HistoryItem[] = [];
        map.set(label, list);
        groups.push({ dayLabel: label, items: list });
      }
      map.get(label)!.push(item);
    }

    return groups;
  });

  async function removeItem(event: MouseEvent, mediaId: number) {
    event.stopPropagation();
    try {
      const res = await apiFetch(`/api/history/${mediaId}`, { method: 'DELETE' });
      if (res.ok) {
        historyItems = historyItems.filter((item) => item.id !== mediaId);
      }
    } catch {
      // Ignora erro de rede
    }
  }

  async function clearAllHistory() {
    isClearing = true;
    try {
      const res = await apiFetch('/api/history', { method: 'DELETE' });
      if (res.ok) {
        historyItems = [];
        showConfirmClear = false;
      }
    } catch (err) {
      errorMessage = err instanceof Error ? err.message : String(err);
    } finally {
      isClearing = false;
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

  onMount(() => {
    void loadHistory();
  });
</script>

<div class="history-page-layout">
  <!-- Coluna Principal (Esquerda): Lista agrupada por dia -->
  <div class="history-main-content">
    <PageHeader title={t('history.title')} description={t('nav.history_desc')}>
      {#snippet meta()}
        {#if historyItems.length > 0}
          <span class="history-count-badge">
            {historyItems.length} {historyItems.length === 1 ? 'vídeo assistido' : 'vídeos assistidos'}
          </span>
        {/if}
      {/snippet}
    </PageHeader>

    {#if loading}
      <PageState kind="loading" title={t('history.loading')} />
    {:else if errorMessage}
      {#snippet retryHistory()}
        <Button variant="secondary" onclick={() => void loadHistory()}>{t('common.retry')}</Button>
      {/snippet}
      <PageState kind="error" icon="error" title={t('history.load_error')} description={errorMessage} actions={retryHistory} />
    {:else if historyItems.length === 0}
      {#snippet startWatching()}
        <a href="/" class="primary-action">
          <span class="material-symbols-outlined icon-18">home</span>
          Começar a assistir
        </a>
      {/snippet}
      <PageState kind="empty" icon="history" title={t('history.empty_title')} description={t('history.empty_desc')} actions={startWatching} />
    {:else if groupedHistory.length === 0}
      {#snippet clearHistorySearch()}
        <Button variant="secondary" onclick={() => (searchQuery = '')}>Limpar busca</Button>
      {/snippet}
      <PageState
        kind="empty"
        icon="search_off"
        title={t('common.empty')}
        description={`Nenhum vídeo no histórico corresponde à busca “${searchQuery}”.`}
        actions={clearHistorySearch}
      />
    {:else}
      <div class="history-groups-list">
        {#each groupedHistory as group (group.dayLabel)}
          <section class="history-day-section">
            <h3 class="history-day-title">{group.dayLabel}</h3>

            <div class="history-items-list">
              {#each group.items as item, itemIndex (item.id)}
                {@const playback = player.getPlayback(item.id) ?? item.playback}
                <div
                  class="history-row-card"
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
                  <!-- Thumbnail com links e badges -->
                  <div class="history-thumb-wrap">
                    <a
                      href={`/watch?v=${item.id}`}
                      class="history-thumb-link"
                      onclick={(e) => handleMediaClick(e, item)}
                      aria-label={item.title}
                    >
                      <img
                        src={item.thumbnail_url}
                        alt={item.title}
                        loading={itemIndex < 8 ? 'eager' : 'lazy'}
                        decoding={itemIndex < 6 ? 'sync' : 'async'}
                        fetchpriority={itemIndex < 3 ? 'high' : 'auto'}
                        class="history-thumb-img"
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

                    {#if !playback?.completed && playback?.position_ms && item.duration_ms && item.duration_ms > 0}
                      <div class="playback-progress-track">
                        <div
                          class="playback-progress-bar"
                          style="width: {Math.min(100, Math.round((playback.position_ms / item.duration_ms) * 100))}%"
                        ></div>
                      </div>
                    {/if}
                  </div>

                  <!-- Detalhes do vídeo (Título, Canal, Descrição) -->
                  <div class="history-item-details">
                    <div class="history-item-top-row">
                      <a
                        href={`/watch?v=${item.id}`}
                        class="history-item-title-link"
                        onclick={(e) => handleMediaClick(e, item)}
                      >
                        <h4 class="history-item-title" title={item.title}>{item.title}</h4>
                      </a>

                      <!-- Botão remover no canto direito -->
                      <div class="history-row-actions">
                        <button
                          type="button"
                          class="history-action-btn remove-btn"
                          onclick={(e) => void removeItem(e, item.id)}
                          title={t('history.remove_item')}
                        >
                          <span class="material-symbols-outlined">close</span>
                        </button>
                      </div>
                    </div>

                    <div class="history-channel-row">
                      {#if item.channel?.avatar_url}
                        <img src={item.channel.avatar_url} alt="" class="history-channel-avatar" />
                      {/if}
                      <span class="history-channel-name">
                        {item.channel?.name ?? 'Canal desconhecido'}
                      </span>
                    </div>

                    {#if item.description}
                      <p class="history-item-description">
                        {item.description}
                      </p>
                    {/if}

                    {#if item.tags.length > 0}
                      <div class="history-tags-row">
                        {#each item.tags.slice(0, 3) as tag (tag.id)}
                          <span class="history-tag-pill">#{tag.display_name}</span>
                        {/each}
                      </div>
                    {/if}
                  </div>
                </div>
              {/each}
            </div>
          </section>
        {/each}
      </div>
    {/if}
  </div>

  <!-- Coluna Lateral (Direita): Painel estilo YouTube com busca e limpar histórico -->
  <aside class="history-side-panel">
    <div class="history-side-sticky">
      <!-- Campo de busca dentro do histórico -->
      <div class="history-search-box">
        <span class="material-symbols-outlined history-search-icon" aria-hidden="true">search</span>
        <input
          type="text"
          class="history-search-input"
          placeholder={t('history.search_placeholder')}
          bind:value={searchQuery}
          aria-label={t('history.search_placeholder')}
        />
        {#if searchQuery}
          <button
            type="button"
            class="history-search-clear"
            onclick={() => (searchQuery = '')}
            title={t('history.clear_search')}
          >
            <span class="material-symbols-outlined icon-16">close</span>
          </button>
        {/if}
      </div>

      <div class="history-side-divider"></div>

      <!-- Ações do painel lateral -->
      <div class="history-side-actions">
        <button
          type="button"
          class="history-side-btn"
          onclick={() => (showConfirmClear = true)}
          disabled={historyItems.length === 0}
        >
          <span class="material-symbols-outlined side-btn-icon">delete</span>
          <span>{t('history.clear_btn')}</span>
        </button>
      </div>

      <div class="history-side-divider"></div>

      <div class="history-side-info">
        <span class="material-symbols-outlined info-icon">info</span>
        <p>O histórico armazena automaticamente o ponto exato de onde você parou em cada vídeo.</p>
      </div>
    </div>
  </aside>
</div>

<!-- Modal de confirmação para limpar todo o histórico -->
{#if showConfirmClear}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="confirm-modal-backdrop" onclick={() => (showConfirmClear = false)}>
    <div class="confirm-modal-box" onclick={(e) => e.stopPropagation()}>
      <div class="confirm-modal-header">
        <span class="material-symbols-outlined warning-icon">delete_forever</span>
        <h3>{t('history.confirm_clear_title')}</h3>
      </div>
      <p class="confirm-modal-desc">{t('history.confirm_clear_desc')}</p>
      <div class="confirm-modal-buttons">
        <button
          type="button"
          class="cancel-btn"
          onclick={() => (showConfirmClear = false)}
          disabled={isClearing}
        >
          {t('common.cancel')}
        </button>
        <button
          type="button"
          class="danger-btn"
          onclick={() => void clearAllHistory()}
          disabled={isClearing}
        >
          {isClearing ? t('common.loading') : t('history.clear_btn')}
        </button>
      </div>
    </div>
  </div>
{/if}

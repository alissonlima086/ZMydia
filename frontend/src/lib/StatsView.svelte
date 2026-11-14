<script lang="ts">
  import { onMount } from 'svelte';
  import { apiFetch } from './api';
  import { t } from './i18n/i18n.svelte';
  import { player, type MediaItem } from './player.svelte';
  import Button from './ui/Button.svelte';
  import PageHeader from './ui/PageHeader.svelte';
  import PageState from './ui/PageState.svelte';

  type Period = 'week' | 'month' | 'year';

  type TopVideo = {
    id: number;
    media_id?: number;
    title: string;
    thumbnail_url: string;
    duration_ms: number | null;
    is_short: boolean;
    channel_id: string | null;
    channel_name: string | null;
    channel_avatar_url: string | null;
    play_count: number;
    total_watch_time_ms: number;
  };

  type TopChannel = {
    id: string;
    channel_id?: string;
    name: string;
    channel_name?: string;
    avatar_url: string | null;
    handle: string | null;
    play_count: number;
    unique_videos_count: number;
    unique_videos?: number;
  };

  type TopTag = {
    id: number;
    tag_id?: number;
    display_name: string;
    normalized_name: string;
    tag_name?: string;
    play_count: number;
    unique_videos_count: number;
    video_count?: number;
  };

  type StatsResponse = {
    period: string;
    total_scrobbles: number;
    total_watch_time_ms: number;
    top_videos: TopVideo[];
    top_channels: TopChannel[];
    top_tags: TopTag[];
  };

  let selectedPeriod = $state<Period>('month');
  let loading = $state(true);
  let errorMessage = $state<string | null>(null);
  let stats = $state<StatsResponse | null>(null);

  const periods = $derived<{ id: Period; label: string }[]>([
    { id: 'week', label: t('stats.period_week') },
    { id: 'month', label: t('stats.period_month') },
    { id: 'year', label: t('stats.period_year') }
  ]);

  async function loadStats(period: Period) {
    loading = true;
    errorMessage = null;
    try {
      const res = await apiFetch(`/api/stats/playback?period=${period}`);
      if (!res.ok) {
        throw new Error(`Erro ao carregar estatísticas (${res.status})`);
      }
      stats = (await res.json()) as StatsResponse;
    } catch (err) {
      console.error('Failed to load statistics:', err);
      errorMessage = err instanceof Error ? err.message : String(err);
    } finally {
      loading = false;
    }
  }

  function changePeriod(newPeriod: Period) {
    if (selectedPeriod === newPeriod) return;
    selectedPeriod = newPeriod;
    void loadStats(newPeriod);
  }

  onMount(() => {
    void loadStats(selectedPeriod);
  });

  function formatDuration(ms: number | null): string {
    if (!ms || ms <= 0) return '--:--';
    const totalSecs = Math.floor(ms / 1000);
    const hours = Math.floor(totalSecs / 3600);
    const mins = Math.floor((totalSecs % 3600) / 60);
    const secs = totalSecs % 60;
    if (hours > 0) {
      return `${hours}:${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`;
    }
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  function formatWatchTime(ms: number): string {
    if (!ms || ms <= 0) return '0 min';
    const totalSecs = Math.floor(ms / 1000);
    const hours = Math.floor(totalSecs / 3600);
    const mins = Math.floor((totalSecs % 3600) / 60);
    const secs = totalSecs % 60;
    if (hours > 0) {
      return `${hours}h ${mins}m`;
    }
    if (mins > 0) {
      return `${mins}m ${secs}s`;
    }
    return `${secs}s`;
  }

  function formatPlays(count: number): string {
    if (count === 1) {
      return t('stats.plays_singular');
    }
    return t('stats.plays', { count });
  }

  // Gradientes determinísticos para canais sem avatar
  const GRADIENTS = [
    'linear-gradient(135deg, #f43f5e 0%, #fb7185 100%)',
    'linear-gradient(135deg, #8b5cf6 0%, #a78bfa 100%)',
    'linear-gradient(135deg, #06b6d4 0%, #22d3ee 100%)',
    'linear-gradient(135deg, #10b981 0%, #34d399 100%)',
    'linear-gradient(135deg, #f59e0b 0%, #fbbf24 100%)',
    'linear-gradient(135deg, #ec4899 0%, #f472b6 100%)',
    'linear-gradient(135deg, #3b82f6 0%, #60a5fa 100%)',
    'linear-gradient(135deg, #6366f1 0%, #818cf8 100%)'
  ];

  function getMonogram(name: string): string {
    if (!name) return '?';
    const clean = name.replace(/^@/, '').trim();
    return clean.charAt(0).toUpperCase();
  }

  function getGradient(name: string): string {
    let hash = 0;
    for (let i = 0; i < name.length; i++) {
      hash = name.charCodeAt(i) + ((hash << 5) - hash);
    }
    return GRADIENTS[Math.abs(hash) % GRADIENTS.length];
  }

  async function handleVideoClick(e: MouseEvent, videoId: number) {
    if (e.ctrlKey || e.metaKey || e.button === 1) return;
    e.preventDefault();
    try {
      const res = await apiFetch(`/api/media/${videoId}`);
      if (res.ok) {
        const item = (await res.json()) as MediaItem;
        void player.playMedia(item, true);
        return;
      }
    } catch {}
    window.history.pushState({}, '', `/watch?v=${videoId}`);
    window.dispatchEvent(new PopStateEvent('popstate'));
  }

  function handleChannelClick(e: MouseEvent, channelId: string) {
    if (e.ctrlKey || e.metaKey || e.button === 1) return;
    e.preventDefault();
    window.history.pushState({}, '', `/canal/${encodeURIComponent(channelId)}`);
    window.dispatchEvent(new PopStateEvent('popstate'));
  }

  function handleTagClick(e: MouseEvent, tagName: string) {
    if (e.ctrlKey || e.metaKey || e.button === 1) return;
    e.preventDefault();
    window.history.pushState({}, '', `/explorar?q=%23${encodeURIComponent(tagName)}`);
    window.dispatchEvent(new PopStateEvent('popstate'));
  }
</script>

<div class="stats-container">
  <PageHeader title={t('stats.title')} description={t('stats.subtitle')}>
    {#snippet actions()}<div class="period-pills" role="radiogroup" aria-label="Filtro de período">{#each periods as p}<button type="button" class="period-pill" class:active={selectedPeriod === p.id} onclick={() => changePeriod(p.id)} role="radio" aria-checked={selectedPeriod === p.id}>{p.label}</button>{/each}</div>{/snippet}
  </PageHeader>

  {#if loading}
    <PageState kind="loading" title={t('common.loading')} />
  {:else if errorMessage}
    {#snippet retry()}<Button onclick={() => loadStats(selectedPeriod)}>{t('common.retry')}</Button>{/snippet}
    <PageState kind="error" icon="error_outline" title={t('common.error')} description={errorMessage} actions={retry} />
  {:else if stats}
    <!-- Resumo numérico do período -->
    <div class="summary-cards-row">
      <div class="summary-card">
        <div class="summary-icon-wrap scrobble-icon">
          <span class="material-symbols-outlined">play_arrow</span>
        </div>
        <div class="summary-data">
          <span class="summary-label">{t('stats.total_scrobbles')}</span>
          <span class="summary-value">{stats.total_scrobbles}</span>
        </div>
      </div>

      <div class="summary-card">
        <div class="summary-icon-wrap time-icon">
          <span class="material-symbols-outlined">schedule</span>
        </div>
        <div class="summary-data">
          <span class="summary-label">{t('stats.total_watch_time')}</span>
          <span class="summary-value">{formatWatchTime(stats.total_watch_time_ms)}</span>
        </div>
      </div>

    </div>

    {#if stats.total_scrobbles === 0}
      {#snippet startWatching()}<a href="/" class="btn-primary-link"><span class="material-symbols-outlined">play_circle</span><span>{t('nav.home')}</span></a>{/snippet}
      <PageState kind="empty" icon="insights" title={t('stats.empty_title')} description={t('stats.empty_desc')} actions={startWatching} />
    {:else}
      <!-- Grid dos 3 Cards: Vídeos, Canais e Tags -->
      <div class="rankings-grid">
        <!-- CARD 1: TOP 10 VÍDEOS -->
        <section class="ranking-card video-ranking-card">
          <div class="card-header">
            <div class="card-header-title">
              <span class="material-symbols-outlined card-title-icon video-accent">smart_display</span>
              <h2>{t('stats.top_videos')}</h2>
            </div>
          </div>

          <div class="card-body">
            {#if stats.top_videos.length === 0}
              <div class="empty-ranking-msg">{t('common.empty')}</div>
            {:else}
              <ol class="ranking-list">
                {#each stats.top_videos as video, i (video.id ?? video.media_id ?? i)}
                  {@const vidId = video.id ?? video.media_id ?? 0}
                  <li class="ranking-item">
                    <span class="rank-pos" class:rank-first={i === 0} class:rank-top3={i < 3}>
                      #{i + 1}
                    </span>

                    <a
                      href={`/watch?v=${vidId}`}
                      class="video-thumb-link"
                      onclick={(e) => handleVideoClick(e, vidId)}
                      title={video.title}
                    >
                      <img
                        src={video.thumbnail_url}
                        alt={video.title}
                        class="video-thumb-img"
                        loading={i < 8 ? 'eager' : 'lazy'}
                        decoding={i < 6 ? 'sync' : 'async'}
                        fetchpriority={i < 3 ? 'high' : 'auto'}
                      />
                      {#if video.duration_ms}
                        <span class="duration-tag">{formatDuration(video.duration_ms)}</span>
                      {/if}
                      <div class="play-overlay">
                        <span class="material-symbols-outlined play-icon">play_arrow</span>
                      </div>
                    </a>

                    <div class="item-info">
                      <a
                        href={`/watch?v=${vidId}`}
                        class="item-title video-title"
                        onclick={(e) => handleVideoClick(e, vidId)}
                        title={video.title}
                      >
                        {video.title}
                      </a>

                      {#if video.channel_name && video.channel_id}
                        <a
                          href={`/canal/${encodeURIComponent(video.channel_id)}`}
                          class="channel-subtext"
                          onclick={(e) => video.channel_id && handleChannelClick(e, video.channel_id)}
                          title={video.channel_name}
                        >
                          {#if video.channel_avatar_url}
                            <img
                              src={video.channel_avatar_url}
                              alt=""
                              class="micro-avatar"
                              loading={i < 8 ? 'eager' : 'lazy'}
                              decoding={i < 6 ? 'sync' : 'async'}
                              onerror={(e) => {
                                (e.currentTarget as HTMLImageElement).style.display = 'none';
                              }}
                            />
                          {/if}
                          <span>{video.channel_name}</span>
                        </a>
                      {:else if video.channel_name}
                        <span class="channel-subtext muted-channel">
                          <span>{video.channel_name}</span>
                        </span>
                      {/if}

                      <div class="stats-badges-row">
                        <span class="play-count-badge">
                          <span class="material-symbols-outlined micro-icon">visibility</span>
                          {formatPlays(video.play_count)}
                        </span>
                        {#if video.total_watch_time_ms > 0}
                          <span class="time-spent-badge" title={t('stats.total_watch_time')}>
                            {formatWatchTime(video.total_watch_time_ms)}
                          </span>
                        {/if}
                      </div>
                    </div>
                  </li>
                {/each}
              </ol>
            {/if}
          </div>
        </section>

        <!-- CARD 2: TOP 10 CANAIS -->
        <section class="ranking-card channel-ranking-card">
          <div class="card-header">
            <div class="card-header-title">
              <span class="material-symbols-outlined card-title-icon channel-accent">subscriptions</span>
              <h2>{t('stats.top_channels')}</h2>
            </div>
          </div>

          <div class="card-body">
            {#if stats.top_channels.length === 0}
              <div class="empty-ranking-msg">{t('common.empty')}</div>
            {:else}
              <ol class="ranking-list">
                {#each stats.top_channels as channel, i (channel.id ?? channel.channel_id ?? i)}
                  {@const chId = channel.id ?? channel.channel_id ?? ''}
                  {@const chName = channel.name ?? channel.channel_name ?? t('stats.unknown_channel')}
                  {@const videoCount = channel.unique_videos_count ?? channel.unique_videos ?? 0}
                  <li class="ranking-item channel-item">
                    <span class="rank-pos" class:rank-first={i === 0} class:rank-top3={i < 3}>
                      #{i + 1}
                    </span>

                    <a
                      href={`/canal/${encodeURIComponent(chId)}`}
                      class="channel-avatar-link"
                      onclick={(e) => handleChannelClick(e, chId)}
                    >
                      <div
                        class="channel-avatar-fallback"
                        style="background: {getGradient(chName)}"
                      >
                        {getMonogram(chName)}
                      </div>
                      {#if channel.avatar_url}
                        <img
                          src={channel.avatar_url}
                          alt={chName}
                          class="channel-avatar-img"
                          loading={i < 8 ? 'eager' : 'lazy'}
                          decoding={i < 6 ? 'sync' : 'async'}
                          fetchpriority={i < 3 ? 'high' : 'auto'}
                          onerror={(e) => {
                            (e.currentTarget as HTMLImageElement).style.display = 'none';
                          }}
                        />
                      {/if}
                    </a>

                    <div class="item-info">
                      <a
                        href={`/canal/${encodeURIComponent(chId)}`}
                        class="item-title"
                        onclick={(e) => handleChannelClick(e, chId)}
                        title={chName}
                      >
                        {chName}
                      </a>

                      {#if channel.handle}
                        <span class="item-handle">{channel.handle}</span>
                      {/if}

                      <div class="stats-badges-row">
                        <span class="play-count-badge">
                          <span class="material-symbols-outlined micro-icon">visibility</span>
                          {formatPlays(channel.play_count)}
                        </span>
                        <span class="subtext-badge">
                          {t('stats.unique_videos', { count: videoCount })}
                        </span>
                      </div>
                    </div>
                  </li>
                {/each}
              </ol>
            {/if}
          </div>
        </section>

        <!-- CARD 3: TOP 10 TAGS -->
        <section class="ranking-card tag-ranking-card">
          <div class="card-header">
            <div class="card-header-title">
              <span class="material-symbols-outlined card-title-icon tag-accent">local_offer</span>
              <h2>{t('stats.top_tags')}</h2>
            </div>
          </div>

          <div class="card-body">
            {#if stats.top_tags.length === 0}
              <div class="empty-ranking-msg">{t('common.empty')}</div>
            {:else}
              <ol class="ranking-list">
                {#each stats.top_tags as tag, i (tag.id ?? tag.tag_id ?? i)}
                  {@const normTag = tag.normalized_name ?? tag.tag_name ?? tag.display_name}
                  {@const tagVidCount = tag.unique_videos_count ?? tag.video_count ?? 0}
                  <li class="ranking-item tag-item">
                    <span class="rank-pos" class:rank-first={i === 0} class:rank-top3={i < 3}>
                      #{i + 1}
                    </span>

                    <a
                      href={`/explorar?q=%23${encodeURIComponent(normTag)}`}
                      class="tag-pill-badge"
                      onclick={(e) => handleTagClick(e, normTag)}
                    >
                      <span class="tag-hash">#</span>
                      <span class="tag-name">{tag.display_name}</span>
                    </a>

                    <div class="item-info">
                      <div class="stats-badges-row">
                        <span class="play-count-badge">
                          <span class="material-symbols-outlined micro-icon">visibility</span>
                          {formatPlays(tag.play_count)}
                        </span>
                        <span class="subtext-badge">
                          {t('stats.unique_videos', { count: tagVidCount })}
                        </span>
                      </div>
                    </div>
                  </li>
                {/each}
              </ol>
            {/if}
          </div>
        </section>
      </div>
    {/if}
  {/if}
</div>

<style>
  .stats-container {
    padding: 24px 32px 48px;
    max-width: 1400px;
    margin: 0 auto;
    color: #e4e4e7;
  }

  .period-pills {
    display: inline-flex;
    background: var(--card-bg, #101114);
    border: 1px solid var(--card-border, rgba(255, 255, 255, 0.065));
    padding: 3px;
    border-radius: 8px;
    gap: 3px;
  }

  .period-pill {
    background: transparent;
    border: none;
    color: #a1a1aa;
    padding: 6px 14px;
    font-size: 12px;
    font-weight: 500;
    border-radius: 6px;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .period-pill:hover {
    color: #ffffff;
    background: rgba(255, 255, 255, 0.05);
  }

  .period-pill.active {
    background: var(--accent);
    color: #ffffff;
    font-weight: 600;
    box-shadow: 0 2px 8px var(--accent-glow);
  }

  /* Summary Row */
  .summary-cards-row {
    display: grid;
    grid-template-columns: repeat(2, minmax(240px, 1fr));
    gap: 14px;
    margin-bottom: 24px;
  }

  .summary-card {
    background: var(--card-bg, #101114);
    border: 1px solid var(--card-border, rgba(255, 255, 255, 0.065));
    border-radius: 10px;
    padding: 14px 16px;
    display: flex;
    align-items: center;
    gap: 14px;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.12);
  }

  .summary-icon-wrap {
    width: 44px;
    height: 44px;
    border-radius: 10px;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .summary-icon-wrap .material-symbols-outlined {
    font-size: 24px;
  }

  .scrobble-icon {
    background: var(--accent-soft);
    color: var(--accent);
  }

  .time-icon {
    background: rgba(59, 130, 246, 0.15);
    color: #3b82f6;
  }

  .summary-data {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .summary-label {
    font-size: 12px;
    color: #71717a;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .summary-value {
    font-size: 20px;
    font-weight: 700;
    color: #fafafa;
  }

  /* O ranking de vídeos usa a largura toda; canais e tags formam uma segunda linha. */
  .rankings-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 18px;
    align-items: start;
  }

  .video-ranking-card {
    grid-column: 1 / -1;
  }

  @media (max-width: 1100px) {
    .rankings-grid {
      grid-template-columns: 1fr;
    }
  }

  .ranking-card {
    background: var(--card-bg, #101114);
    border: 1px solid var(--card-border, rgba(255, 255, 255, 0.065));
    border-radius: 10px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.15);
  }

  .card-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.07);
    background: rgba(255, 255, 255, 0.015);
  }

  .card-header-title {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .card-title-icon {
    font-size: 22px;
  }

  .video-accent {
    color: var(--accent);
  }

  .channel-accent {
    color: #3b82f6;
  }

  .tag-accent {
    color: #10b981;
  }

  .card-header h2 {
    font-size: 16px;
    font-weight: 600;
    color: #fafafa;
    margin: 0;
  }

  .card-body {
    padding: 12px;
  }

  .video-ranking-card .ranking-list {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px 18px;
  }

  .empty-ranking-msg {
    padding: 32px 16px;
    text-align: center;
    color: #71717a;
    font-size: 14px;
  }

  .ranking-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .ranking-item {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 10px;
    border-radius: 10px;
    background: transparent;
    transition: background 0.15s ease;
  }

  .ranking-item:hover {
    background: rgba(255, 255, 255, 0.04);
  }

  /* Rank Position Badge */
  .rank-pos {
    width: 26px;
    text-align: center;
    font-size: 13px;
    font-weight: 700;
    color: #71717a;
    flex-shrink: 0;
  }

  .rank-pos.rank-top3 {
    color: #e4e4e7;
  }

  .rank-pos.rank-first {
    color: #fbbf24;
    font-size: 14px;
  }

  /* Video Thumbnail */
  .video-thumb-link {
    position: relative;
    width: 96px;
    height: 54px;
    border-radius: 6px;
    overflow: hidden;
    flex-shrink: 0;
    background: #09090b;
    display: block;
  }

  .video-thumb-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .duration-tag {
    position: absolute;
    bottom: 3px;
    right: 3px;
    background: rgba(0, 0, 0, 0.8);
    color: #ffffff;
    font-size: 10px;
    font-weight: 600;
    padding: 1px 4px;
    border-radius: 3px;
  }

  .play-overlay {
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.4);
    display: flex;
    align-items: center;
    justify-content: center;
    opacity: 0;
    transition: opacity 0.15s ease;
  }

  .video-thumb-link:hover .play-overlay {
    opacity: 1;
  }

  .play-icon {
    color: #ffffff;
    font-size: 26px;
  }

  /* Channel Avatar */
  .channel-avatar-link {
    width: 44px;
    height: 44px;
    border-radius: 50%;
    overflow: hidden;
    flex-shrink: 0;
    display: block;
    text-decoration: none;
    position: relative;
  }

  .channel-avatar-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 50%;
    position: absolute;
    inset: 0;
  }

  .channel-avatar-fallback {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #ffffff;
    font-weight: 700;
    font-size: 18px;
    border-radius: 50%;
  }

  /* Tag pill */
  .tag-pill-badge {
    background: rgba(16, 185, 129, 0.12);
    border: 1px solid rgba(16, 185, 129, 0.25);
    color: #34d399;
    padding: 6px 12px;
    border-radius: 8px;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    text-decoration: none;
    font-size: 13px;
    font-weight: 600;
    max-width: 140px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    transition: background 0.15s ease;
    flex-shrink: 0;
  }

  .tag-pill-badge:hover {
    background: rgba(16, 185, 129, 0.22);
  }

  .tag-hash {
    opacity: 0.6;
    font-weight: 700;
  }

  .tag-name {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Item Info */
  .item-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .item-title {
    font-size: 13px;
    font-weight: 600;
    color: #f4f4f5;
    text-decoration: none;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    display: block;
    transition: color 0.15s ease;
  }

  .item-title:hover {
    color: var(--accent);
  }

  .item-handle {
    font-size: 12px;
    color: #71717a;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .channel-subtext {
    font-size: 12px;
    color: #a1a1aa;
    text-decoration: none;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .channel-subtext:hover:not(.muted-channel) {
    color: #f4f4f5;
  }

  .micro-avatar {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    object-fit: cover;
  }

  .stats-badges-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 2px;
  }

  .play-count-badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    font-weight: 600;
    color: #d4d4d8;
  }

  .micro-icon {
    font-size: 13px;
    color: var(--accent);
  }

  .time-spent-badge,
  .subtext-badge {
    font-size: 11px;
    color: #71717a;
  }

  .btn-primary-link {
    margin-top: 12px;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    background: var(--accent);
    color: #ffffff;
    font-size: 13.5px;
    font-weight: 600;
    padding: 8px 18px;
    border-radius: 8px;
    text-decoration: none;
    transition: background 0.15s ease;
  }

  .btn-primary-link:hover {
    background: var(--accent-hover);
  }

  @media (max-width: 720px) {
    .stats-container {
      padding: 18px 16px 40px;
    }

    .summary-cards-row,
    .video-ranking-card .ranking-list {
      grid-template-columns: 1fr;
    }
  }

</style>

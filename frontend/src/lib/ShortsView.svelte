<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { apiFetch } from './api';
  import { player, type MediaItem } from './player.svelte';
  import SaveToPlaylistModal from './SaveToPlaylistModal.svelte';
  import Button from './ui/Button.svelte';
  import PageState from './ui/PageState.svelte';
  import { t } from './i18n/i18n.svelte';

  let shorts = $state<MediaItem[]>([]);
  let activeIndex = $state<number>(0);
  let nextCursor = $state<string | null>(null);
  let sessionSeed = $state<number>(Math.floor(Math.random() * 2147483647));
  let loading = $state(true);
  let loadingMore = $state(false);
  let errorMessage = $state<string | null>(null);
  let isMuted = $state(false);

  // Playback & seek state
  let currentTime = $state(0);
  let duration = $state(0);
  let isPaused = $state(false);
  let isScrubbing = $state(false);
  let hoverTime = $state<number | null>(null);
  let hoverPercent = $state(0);
  let feedbackAction = $state<'play' | 'pause' | 'fwd' | 'rwd' | null>(null);
  let feedbackTimer: number | null = null;
  let copiedShortId = $state<number | null>(null);

  let containerEl = $state<HTMLElement | null>(null);
  let videoEls = $state<(HTMLVideoElement | null)[]>([]);
  let scrubberTrackEl = $state<HTMLElement | null>(null);
  let playlistModalMedia = $state<MediaItem | null>(null);
  let destroyed = false;
  let autoplayFrame: number | null = null;
  let initialScrollTimer: number | null = null;

  function getUrlParams() {
    if (typeof window === 'undefined') return { v: null, channel: null };
    const params = new URLSearchParams(window.location.search);
    const vStr = params.get('v');
    const v = vStr ? parseInt(vStr, 10) : null;
    const channel = params.get('channel');
    return { v: isNaN(v as number) ? null : v, channel };
  }

  onMount(() => {
    destroyed = false;
    void loadShorts();
    window.addEventListener('keydown', handleKeydown);
    window.addEventListener('popstate', handlePopState);
    window.addEventListener('routechange', handleRouteChange);
    window.addEventListener('pagehide', handlePageHide);
  });

  onDestroy(() => {
    destroyed = true;
    window.removeEventListener('keydown', handleKeydown);
    window.removeEventListener('popstate', handlePopState);
    window.removeEventListener('routechange', handleRouteChange);
    window.removeEventListener('pagehide', handlePageHide);
    window.removeEventListener('mousemove', handleScrubberMouseMove);
    window.removeEventListener('mouseup', handleScrubberMouseUp);
    if (feedbackTimer) clearTimeout(feedbackTimer);
    if (initialScrollTimer) clearTimeout(initialScrollTimer);
    if (autoplayFrame !== null) cancelAnimationFrame(autoplayFrame);
    stopAllVideos(true);
  });

  function handleRouteChange() {
    if (window.location.pathname !== '/shorts') {
      stopAllVideos(true);
      return;
    }
    handleShortLocationChange();
  }

  function handlePageHide() {
    stopAllVideos();
  }

  function stopAllVideos(release = false) {
    const videos = new Set<HTMLVideoElement>();
    for (const video of videoEls) {
      if (video) videos.add(video);
    }
    if (typeof document !== 'undefined') {
      document.querySelectorAll<HTMLVideoElement>('.yt-short-video').forEach((video) => videos.add(video));
    }

    for (const video of videos) {
      if (!video) continue;
      video.pause();
      if (release) {
        video.removeAttribute('src');
        video.load();
      }
    }
    isPaused = true;
  }

  function handlePopState() {
    if (window.location.pathname !== '/shorts') {
      stopAllVideos(true);
      return;
    }
    handleShortLocationChange();
  }

  function handleShortLocationChange() {
    stopAllVideos();
    const { v } = getUrlParams();
    if (v && shorts.length > 0) {
      const idx = shorts.findIndex((s) => s.id === v);
      if (idx >= 0 && idx !== activeIndex) {
        scrollToIndex(idx);
      } else if (idx === activeIndex) {
        void playActiveVideo();
      } else {
        void loadShorts();
      }
    }
  }

  async function loadShorts() {
    stopAllVideos();
    loading = true;
    errorMessage = null;
    const { v, channel } = getUrlParams();

    try {
      let url: string;
      if (channel) {
        url = `/api/media?channel=${encodeURIComponent(channel)}&is_short=true&limit=60`;
      } else {
        url = `/api/feed?type=short&seed=${sessionSeed}&limit=12`;
      }

      const res = await apiFetch(url);
      if (!res.ok) {
        throw new Error(`HTTP ${res.status}`);
      }

      let items: MediaItem[] = [];
      if (channel) {
        items = (await res.json()) as MediaItem[];
        nextCursor = null;
      } else {
        const data = (await res.json()) as { items: MediaItem[]; next_cursor: string | null };
        items = data.items;
        nextCursor = data.next_cursor;
      }

      if (v && !items.some((it) => it.id === v)) {
        try {
          const singleRes = await apiFetch(`/api/media/${v}`);
          if (singleRes.ok) {
            const single = (await singleRes.json()) as MediaItem;
            items = [single, ...items];
          }
        } catch {
          // Ignora se não encontrar vídeo avulso
        }
      }

      shorts = items;

      if (v && items.length > 0) {
        const targetIdx = items.findIndex((it) => it.id === v);
        if (targetIdx >= 0) {
          activeIndex = targetIdx;
          initialScrollTimer = window.setTimeout(() => scrollToIndex(targetIdx), 50);
        }
      }
    } catch (err) {
      errorMessage = err instanceof Error ? err.message : String(err);
    } finally {
      loading = false;
      await tick();
      if (destroyed || window.location.pathname !== '/shorts') return;
      if (autoplayFrame !== null) cancelAnimationFrame(autoplayFrame);
      autoplayFrame = requestAnimationFrame(() => {
        autoplayFrame = null;
        void playActiveVideo();
      });
    }
  }

  async function loadMore() {
    const { channel } = getUrlParams();
    if (channel || !nextCursor || loadingMore) return;
    loadingMore = true;
    try {
      const res = await apiFetch(
        `/api/feed?type=short&seed=${sessionSeed}&cursor=${nextCursor}&limit=12`
      );
      if (res.ok) {
        const data = (await res.json()) as { items: MediaItem[]; next_cursor: string | null };
        shorts = [...shorts, ...data.items];
        nextCursor = data.next_cursor;
      }
    } catch {
      // Ignorar falha de paginação pontual
    } finally {
      loadingMore = false;
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    const target = event.target as HTMLElement | null;
    if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable)) {
      return;
    }

    if (event.key === 'ArrowDown') {
      event.preventDefault();
      scrollToIndex(activeIndex + 1);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      scrollToIndex(activeIndex - 1);
    } else if (event.key === 'ArrowLeft') {
      event.preventDefault();
      seekDelta(-5);
    } else if (event.key === 'ArrowRight') {
      event.preventDefault();
      seekDelta(5);
    } else if (event.key === 'j' || event.key === 'J') {
      event.preventDefault();
      seekDelta(-10);
    } else if (event.key === 'l' || event.key === 'L') {
      event.preventDefault();
      seekDelta(10);
    } else if (event.key === 'm' || event.key === 'M') {
      event.preventDefault();
      toggleMute();
    } else if (event.key === ' ' || event.key === 'k' || event.key === 'K') {
      event.preventDefault();
      toggleCurrentPlay();
    }
  }

  function scrollToIndex(index: number) {
    if (index < 0 || index >= shorts.length) return;
    const targetSlide = containerEl?.children[index] as HTMLElement | undefined;
    if (targetSlide) {
      targetSlide.scrollIntoView({ behavior: 'smooth' });
    }
  }

  function toggleMute() {
    isMuted = !isMuted;
    for (const video of videoEls) {
      if (video) video.muted = isMuted;
    }
  }

  function showFeedback(action: 'play' | 'pause' | 'fwd' | 'rwd') {
    feedbackAction = action;
    if (feedbackTimer) clearTimeout(feedbackTimer);
    feedbackTimer = window.setTimeout(() => {
      feedbackAction = null;
    }, 600);
  }

  function toggleCurrentPlay() {
    const activeVideo = videoEls[activeIndex];
    if (!activeVideo) return;
    if (activeVideo.paused) {
      void activeVideo.play();
      isPaused = false;
      showFeedback('play');
    } else {
      activeVideo.pause();
      isPaused = true;
      showFeedback('pause');
    }
  }

  function seekDelta(seconds: number) {
    const activeVideo = videoEls[activeIndex];
    if (!activeVideo || !duration) return;
    const target = Math.max(0, Math.min(activeVideo.currentTime + seconds, duration));
    activeVideo.currentTime = target;
    currentTime = target;
    showFeedback(seconds > 0 ? 'fwd' : 'rwd');
  }

  function handleScrubberMouseDown(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    isScrubbing = true;
    updateSeekFromEvent(e);
    window.addEventListener('mousemove', handleScrubberMouseMove);
    window.addEventListener('mouseup', handleScrubberMouseUp);
  }

  function updateSeekFromEvent(e: MouseEvent) {
    if (!scrubberTrackEl || !duration) return;
    const rect = scrubberTrackEl.getBoundingClientRect();
    const x = Math.max(0, Math.min(e.clientX - rect.left, rect.width));
    const ratio = rect.width > 0 ? x / rect.width : 0;
    const newTime = ratio * duration;
    currentTime = newTime;
    const activeVideo = videoEls[activeIndex];
    if (activeVideo) {
      activeVideo.currentTime = newTime;
    }
  }

  function handleScrubberMouseMove(e: MouseEvent) {
    if (!isScrubbing) return;
    updateSeekFromEvent(e);
  }

  function handleScrubberMouseUp(e: MouseEvent) {
    if (!isScrubbing) return;
    updateSeekFromEvent(e);
    isScrubbing = false;
    window.removeEventListener('mousemove', handleScrubberMouseMove);
    window.removeEventListener('mouseup', handleScrubberMouseUp);
  }

  function handleScrubberHover(e: MouseEvent) {
    if (!scrubberTrackEl || !duration) return;
    const rect = scrubberTrackEl.getBoundingClientRect();
    const x = Math.max(0, Math.min(e.clientX - rect.left, rect.width));
    hoverPercent = rect.width > 0 ? (x / rect.width) * 100 : 0;
    hoverTime = rect.width > 0 ? (x / rect.width) * duration : 0;
  }

  function handleScrubberLeave() {
    hoverTime = null;
  }

  function handleScrubberKeydown(event: KeyboardEvent) {
    if (!duration) return;
    if (event.key === 'ArrowLeft' || event.key === 'ArrowDown') {
      event.preventDefault();
      seekDelta(-5);
    } else if (event.key === 'ArrowRight' || event.key === 'ArrowUp') {
      event.preventDefault();
      seekDelta(5);
    } else if (event.key === 'Home' || event.key === 'End') {
      event.preventDefault();
      const target = event.key === 'Home' ? 0 : duration;
      const activeVideo = videoEls[activeIndex];
      if (activeVideo) activeVideo.currentTime = target;
      currentTime = target;
    }
  }

  function onScroll() {
    if (!containerEl) return;
    const containerTop = containerEl.scrollTop;
    const itemHeight = containerEl.clientHeight;
    if (itemHeight <= 0) return;

    const newIndex = Math.round(containerTop / itemHeight);
    if (newIndex !== activeIndex && newIndex >= 0 && newIndex < shorts.length) {
      activeIndex = newIndex;
      updateVideoPlayback();

      const currentShort = shorts[newIndex];
      if (currentShort) {
        const url = new URL(window.location.href);
        url.searchParams.set('v', currentShort.id.toString());
        window.history.replaceState({}, '', url.toString());
      }

      if (activeIndex >= shorts.length - 3) {
        void loadMore();
      }
    }
  }

  function updateVideoPlayback() {
    currentTime = 0;
    duration = 0;
    isPaused = false;

    videoEls.forEach((video, idx) => {
      if (!video) return;
      if (idx === activeIndex) {
        video.muted = isMuted;
        void playActiveVideo(video);
        currentTime = video.currentTime || 0;
        duration = video.duration || 0;
        isPaused = video.paused;
      } else {
        video.pause();
        if (Math.abs(idx - activeIndex) > 1) {
          video.currentTime = 0;
        }
      }
    });
  }

  $effect(() => {
    const activeShort = shorts[activeIndex];
    if (activeShort && window.location.pathname === '/shorts') {
      document.title = `${activeShort.title} | ZMydia`;
    }
  });

  async function playActiveVideo(candidate?: HTMLVideoElement) {
    if (destroyed || window.location.pathname !== '/shorts') return;
    const video = candidate ?? videoEls[activeIndex];
    if (!video || video !== videoEls[activeIndex]) return;
    video.muted = isMuted;
    try {
      await video.play();
    } catch {
      // Navegadores podem perder a ativação do clique enquanto o feed é buscado.
      // Nesse caso, iniciar mudo é melhor do que deixar o Short parado.
      if (!isMuted) {
        isMuted = true;
        for (const item of videoEls) if (item) item.muted = true;
        await video.play().catch(() => {});
      }
    }
    isPaused = video.paused;
  }

  function formatTime(secs: number): string {
    if (isNaN(secs) || secs < 0) return '0:00';
    const totalSecs = Math.floor(secs);
    const m = Math.floor(totalSecs / 60);
    const s = totalSecs % 60;
    return `${m}:${s.toString().padStart(2, '0')}`;
  }

  function isSquareish(item: MediaItem): boolean {
    if (!item.width || !item.height) return false;
    let width = item.width;
    let height = item.height;
    if (Math.abs(item.rotation ?? 0) % 180 === 90) {
      [width, height] = [height, width];
    }
    return width >= height && width * 10 <= height * 11;
  }

  async function toggleFav(item: MediaItem) {
    await player.toggleFavorite(item);
  }

  function copyShortLink(id: number) {
    const url = `${window.location.origin}/shorts?v=${id}`;
    void navigator.clipboard.writeText(url);
    copiedShortId = id;
    setTimeout(() => {
      if (copiedShortId === id) copiedShortId = null;
    }, 2000);
  }

  function toggleFullscreen(idx: number) {
    const video = videoEls[idx];
    if (!video) return;
    if (document.fullscreenElement) {
      void document.exitFullscreen();
    } else {
      void video.requestFullscreen();
    }
  }
</script>

<div class="yt-shorts-container">
  {#if loading}
    <PageState kind="loading" title={t('shorts.loading')} />
  {:else if errorMessage}
    {#snippet retryShorts()}
      <Button variant="secondary" onclick={() => void loadShorts()}>{t('common.retry')}</Button>
    {/snippet}
    <PageState kind="error" icon="error" title={t('shorts.load_error')} description={errorMessage} actions={retryShorts} />
  {:else if shorts.length === 0}
    <PageState kind="empty" icon="smartphone" title={t('shorts.empty_desc')} description={t('shorts.empty_hint')} />
  {:else}
    <div
      class="yt-shorts-feed"
      bind:this={containerEl}
      onscroll={onScroll}
    >
      {#each shorts as item, idx (item.id)}
        <div class="yt-short-unit" class:active={idx === activeIndex}>
          <!-- Painel Lateral Esquerdo: Informações do Criador e Título (Tal qual YouTube Desktop) -->
          <div class="yt-short-info-panel">
            {#if item.channel}
              <div class="yt-short-channel-row">
                <a
                  href={`/canal/${item.channel.id}`}
                  class="yt-channel-link"
                  onclick={(e) => {
                    e.stopPropagation();
                    window.history.pushState({}, '', `/canal/${item.channel!.id}`);
                    window.dispatchEvent(new CustomEvent('routechange'));
                  }}
                >
                  <div class="yt-channel-avatar">
                    {#if item.channel.avatar_url}
                      <img src={item.channel.avatar_url} alt={item.channel.name} />
                    {:else}
                      <span class="material-symbols-outlined">subscriptions</span>
                    {/if}
                  </div>
                  <span class="yt-channel-handle">
                    {item.channel.handle ? `@${item.channel.handle.replace(/^@/, '')}` : item.channel.name}
                  </span>
                </a>

                <a
                  href={`/canal/${item.channel.id}`}
                  class="yt-channel-action-pill"
                  onclick={(e) => {
                    e.stopPropagation();
                    window.history.pushState({}, '', `/canal/${item.channel!.id}`);
                    window.dispatchEvent(new CustomEvent('routechange'));
                  }}
                >
                  Ver canal
                </a>
              </div>
            {/if}

            <h2 class="yt-short-title" title={item.title}>{item.title}</h2>

            {#if item.tags.length > 0}
              <div class="yt-short-tags">
                {#each item.tags.slice(0, 3) as tag (tag.id)}
                  <span class="yt-short-tag">#{tag.display_name}</span>
                {/each}
              </div>
            {/if}
          </div>

          <!-- Centro: Card do Player de Vídeo (9:16 com cantos arredondados) -->
          <div class="yt-short-player-card">
            {#if Math.abs(idx - activeIndex) <= 1}
              <video
                bind:this={videoEls[idx]}
                src={`/api/media/${item.id}/stream`}
                class="yt-short-video"
                class:square-letterbox={isSquareish(item)}
                loop
                playsinline
                autoplay={idx === activeIndex}
                preload={idx === activeIndex ? 'auto' : 'metadata'}
                muted={isMuted}
                onclick={toggleCurrentPlay}
                ontimeupdate={(e) => {
                  if (idx === activeIndex && !isScrubbing) {
                    currentTime = e.currentTarget.currentTime;
                  }
                }}
                onloadedmetadata={(e) => {
                  if (idx === activeIndex) {
                    duration = e.currentTarget.duration || 0;
                    currentTime = e.currentTarget.currentTime || 0;
                  }
                }}
                oncanplay={(e) => {
                  if (idx === activeIndex && e.currentTarget.paused) void playActiveVideo(e.currentTarget);
                }}
                onplay={() => {
                  if (idx === activeIndex) isPaused = false;
                }}
                onpause={() => {
                  if (idx === activeIndex) isPaused = true;
                }}
              ></video>
            {:else}
              <div class="yt-short-placeholder">
                <img
                  src={item.thumbnail_url}
                  alt={item.title}
                  class="yt-short-thumb-img"
                  class:square-letterbox={isSquareish(item)}
                />
              </div>
            {/if}

            <!-- Barra Superior do Vídeo (Play/Pause e Áudio no topo esquerdo) -->
            <div class="yt-short-top-bar">
              <div class="top-bar-left">
                <button
                  type="button"
                  class="yt-top-icon-btn"
                  onclick={toggleCurrentPlay}
                  title={isPaused ? t('shorts.play') : t('shorts.pause')}
                  aria-label={isPaused ? t('shorts.play') : t('shorts.pause')}
                >
                  <span class="material-symbols-outlined">{isPaused ? 'play_arrow' : 'pause'}</span>
                </button>

                <button
                  type="button"
                  class="yt-top-icon-btn"
                  onclick={toggleMute}
                  title={isMuted ? t('shorts.sound_unmuted') : t('shorts.sound_muted')}
                  aria-label={isMuted ? t('shorts.sound_unmuted') : t('shorts.sound_muted')}
                >
                  <span class="material-symbols-outlined">{isMuted ? 'volume_off' : 'volume_up'}</span>
                </button>
              </div>

              <div class="top-bar-right">
                <button
                  type="button"
                  class="yt-top-icon-btn"
                  onclick={() => toggleFullscreen(idx)}
                  title="Tela cheia"
                  aria-label="Tela cheia"
                >
                  <span class="material-symbols-outlined">fullscreen</span>
                </button>
              </div>
            </div>

            <!-- Efeito Ripple ao Centro -->
            {#if idx === activeIndex && feedbackAction}
              <div class="yt-short-ripple-badge">
                {#if feedbackAction === 'play'}
                  <span class="material-symbols-outlined">play_arrow</span>
                {:else if feedbackAction === 'pause'}
                  <span class="material-symbols-outlined">pause</span>
                {:else if feedbackAction === 'fwd'}
                  <span class="material-symbols-outlined">forward_5</span>
                {:else if feedbackAction === 'rwd'}
                  <span class="material-symbols-outlined">replay_5</span>
                {/if}
              </div>
            {/if}

            <!-- Linha Vermelha de Seek na Borda Inferior (Idêntica ao Print do YouTube) -->
            {#if idx === activeIndex}
              <div
                class="yt-short-scrubber-bar"
                bind:this={scrubberTrackEl}
                onmousedown={handleScrubberMouseDown}
                onmousemove={handleScrubberHover}
                onmouseleave={handleScrubberLeave}
                onkeydown={handleScrubberKeydown}
                role="slider"
                tabindex="0"
                aria-label={t('shorts.seek')}
                aria-valuenow={currentTime}
                aria-valuemin="0"
                aria-valuemax={duration}
              >
                <div class="yt-scrubber-track">
                  <div
                    class="yt-scrubber-progress"
                    style="width: {duration > 0 ? Math.min(100, (currentTime / duration) * 100) : 0}%"
                  ></div>
                  <div
                    class="yt-scrubber-handle"
                    style="left: {duration > 0 ? Math.min(100, (currentTime / duration) * 100) : 0}%"
                  ></div>
                </div>

                {#if hoverTime !== null}
                  <div class="yt-scrubber-tooltip" style="left: {hoverPercent}%">
                    {formatTime(hoverTime)}
                  </div>
                {/if}
              </div>
            {/if}
          </div>

          <!-- Coluna Lateral Direita de Ações (Tal qual YouTube Desktop) -->
          <div class="yt-short-actions-column">
            <!-- Favorito / Like -->
            <div class="yt-action-item">
              <button
                type="button"
                class="yt-action-circle-btn"
                class:active-fav={item.is_favorite}
                onclick={() => void toggleFav(item)}
                title={item.is_favorite ? t('common.unfavorite') : t('common.favorite')}
              >
                <span class="material-symbols-outlined">{item.is_favorite ? 'favorite' : 'favorite_border'}</span>
              </button>
              <span class="yt-action-label">{item.is_favorite ? 'Salvo' : 'Gostei'}</span>
            </div>

            <!-- Salvar em Playlist -->
            <div class="yt-action-item">
              <button
                type="button"
                class="yt-action-circle-btn"
                onclick={() => (playlistModalMedia = item)}
                title={t('common.save_to_playlist')}
              >
                <span class="material-symbols-outlined">playlist_add</span>
              </button>
              <span class="yt-action-label">Salvar</span>
            </div>

            <!-- Compartilhar / Link -->
            <div class="yt-action-item">
              <button
                type="button"
                class="yt-action-circle-btn"
                onclick={() => copyShortLink(item.id)}
                title={copiedShortId === item.id ? 'Link copiado!' : 'Compartilhar'}
              >
                <span class="material-symbols-outlined">{copiedShortId === item.id ? 'check' : 'share'}</span>
              </button>
              <span class="yt-action-label">{copiedShortId === item.id ? 'Copiado!' : 'Compartilhar'}</span>
            </div>

            <!-- Som -->
            <div class="yt-action-item">
              <button
                type="button"
                class="yt-action-circle-btn"
                onclick={toggleMute}
                title={isMuted ? t('shorts.sound_unmuted') : t('shorts.sound_muted')}
              >
                <span class="material-symbols-outlined">{isMuted ? 'volume_off' : 'volume_up'}</span>
              </button>
              <span class="yt-action-label">Som</span>
            </div>
          </div>

          <!-- Botões Circulares de Próximo / Anterior na extrema direita -->
          <div class="yt-short-nav-column">
            <button
              type="button"
              class="yt-nav-circle-btn"
              disabled={idx === 0}
              onclick={() => scrollToIndex(idx - 1)}
              title={t('shorts.previous')}
            >
              <span class="material-symbols-outlined">keyboard_arrow_up</span>
            </button>

            <button
              type="button"
              class="yt-nav-circle-btn"
              disabled={idx === shorts.length - 1}
              onclick={() => scrollToIndex(idx + 1)}
              title={t('shorts.next')}
            >
              <span class="material-symbols-outlined">keyboard_arrow_down</span>
            </button>
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

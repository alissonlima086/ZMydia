<script lang="ts">
  import { onMount } from 'svelte';
  import { apiFetch } from './api';
  import { player, type MediaItem } from './player.svelte';
  import { formatCueHtml, subtitleStyleStore, hexToRgba } from './subtitleStyle.svelte';
  import SaveToPlaylistModal from './SaveToPlaylistModal.svelte';
  import IdentifyMusicModal from './IdentifyMusicModal.svelte';
  import { musicStore } from './music.svelte';
  import VideoInfoTabs from './VideoInfoTabs.svelte';
  import { parseChapters, type Chapter } from './chapters';
  import ChaptersPanel from './ChaptersPanel.svelte';
  import { t } from './i18n/i18n.svelte';
  import PlaylistPanel from './PlaylistPanel.svelte';
  import SearchBar from './SearchBar.svelte';

  let { hidden = false }: { hidden?: boolean } = $props();

  let containerEl = $state<HTMLElement | null>(null);
  let videoStageEl = $state<HTMLElement | null>(null);
  let videoStageHeight = $state<number>(0);
  let videoEl = $state<HTMLVideoElement | null>(null);
  let isFullscreen = $state(false);

  let relatedVideos = $state<MediaItem[]>([]);
  let loadingRelated = $state(false);
  let showSaveModal = $state(false);
  let showMusicModal = $state(false);
  let playlistModalMedia = $state<MediaItem | null>(null);
  let showContextMenu = $state(false);
  let contextMenuPos = $state({ x: 0, y: 0 });
  let copyFeedback = $state<string | null>(null);
  let showChapters = $state(false);

  const originalChapters = $derived.by(() => {
    return parseChapters(player.currentMedia?.description, player.duration);
  });

  const notesChapters = $derived.by(() => {
    return parseChapters(player.currentMedia?.user_notes, player.duration);
  });

  const chapters = $derived.by(() => {
    if (originalChapters.length > 0) {
      return originalChapters;
    }
    return notesChapters;
  });

  const isUsingOriginalChapters = $derived(originalChapters.length > 0);
  const isUsingNotesChapters = $derived(originalChapters.length === 0 && notesChapters.length > 0);

  const activeChapter = $derived.by(() => {
    if (chapters.length === 0) return null;
    const current = player.currentTime;
    return chapters.find((ch) => current >= ch.startTime && current < ch.endTime) ?? chapters[0];
  });

  let showSubtitleMenu = $state(false);
  let subtitleSubmenu = $state<'list' | 'settings'>('list');
  let isControlsActive = $state(false);
  let controlsHideTimeout: number | null = null;
  let lastMouseX = -1;
  let lastMouseY = -1;

  function handleMouseMove(e?: MouseEvent) {
    if (e) {
      // Ignora eventos sintéticos gerados por re-renders do player quando o cursor não mudou de posição
      if (e.clientX === lastMouseX && e.clientY === lastMouseY) {
        return;
      }
      lastMouseX = e.clientX;
      lastMouseY = e.clientY;
    }

    isControlsActive = true;
    if (controlsHideTimeout) clearTimeout(controlsHideTimeout);
    if (player.isPlaying && !showSubtitleMenu && !showContextMenu && !showChapters) {
      controlsHideTimeout = window.setTimeout(() => {
        isControlsActive = false;
      }, 2500);
    }
  }

  function togglePlayFromStage(e?: MouseEvent) {
    if (e) {
      e.stopPropagation();
      handleMouseMove(e);
    }
    void player.togglePlay();
  }

  function handleStageClick(e?: MouseEvent) {
    if (!player.isExpanded) {
      if (e) e.stopPropagation();
      player.openExpanded();
      return;
    }
    togglePlayFromStage(e);
  }

  function handleStageDblClick(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (!player.isExpanded) {
      player.openExpanded();
      return;
    }
    void toggleFullscreen();
  }

  $effect(() => {
    if (player.isPlaying && !showSubtitleMenu && !showContextMenu && !showChapters) {
      if (controlsHideTimeout) clearTimeout(controlsHideTimeout);
      controlsHideTimeout = window.setTimeout(() => {
        isControlsActive = false;
      }, 2500);
    } else {
      isControlsActive = true;
      if (controlsHideTimeout) clearTimeout(controlsHideTimeout);
    }
  });

  function toggleSubtitles() {
    if (player.selectedSubtitleLang !== null) {
      player.setSubtitleLang(null);
    } else if (player.availableSubtitles.length > 0) {
      const pt = player.availableSubtitles.find((s) => s.lang_code.startsWith('pt'));
      player.setSubtitleLang(pt ? pt.lang_code : player.availableSubtitles[0].lang_code);
    } else if (player.currentMedia?.youtube_id) {
      void player.downloadSubtitles(player.currentMedia.id);
    }
  }

  $effect(() => {
    if (videoEl && player.availableSubtitles) {
      const timer = setTimeout(() => {
        player.syncActiveSubtitleTrack();
      }, 50);
      return () => clearTimeout(timer);
    }
  });

  onMount(() => {
    if (videoEl && !player.videoElement) {
      player.attach(videoEl);
    }
  });

  $effect(() => {
    if (videoEl && !player.videoElement) {
      player.attach(videoEl);
    }
  });

  $effect(() => {
    const currentId = player.currentMedia?.id;
    if (currentId) {
      void loadRelated(currentId);
    }
  });

  async function loadRelated(mediaId: number) {
    loadingRelated = true;
    try {
      const res = await apiFetch(`/api/media/${mediaId}/related`);
      if (res.ok) {
        relatedVideos = (await res.json()) as MediaItem[];
      } else {
        relatedVideos = [];
      }
    } catch {
      relatedVideos = [];
    } finally {
      loadingRelated = false;
    }
  }

  async function toggleFavorite() {
    if (!player.currentMedia) return;
    await player.toggleFavorite(player.currentMedia);
  }

  function openSaveModal() {
    if (!player.currentMedia) return;
    showSaveModal = true;
  }

  function handleContextMenu(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    const menuWidth = 240;
    const menuHeight = 160;
    let x = e.clientX;
    let y = e.clientY;
    if (x + menuWidth > window.innerWidth) x = Math.max(10, window.innerWidth - menuWidth - 10);
    if (y + menuHeight > window.innerHeight) y = Math.max(10, window.innerHeight - menuHeight - 10);
    contextMenuPos = { x, y };
    showContextMenu = true;
  }

  function copyVideoUrl(withTimestamp = false) {
    if (!player.currentMedia) return;
    let url = `${window.location.origin}/watch?v=${player.currentMedia.id}`;
    if (withTimestamp && player.currentTime > 0) {
      url += `&t=${Math.floor(player.currentTime)}`;
    }
    void navigator.clipboard.writeText(url);
    copyFeedback = withTimestamp ? t('player.url_copied_time') : t('player.url_copied');
    setTimeout(() => {
      copyFeedback = null;
      showContextMenu = false;
    }, 1200);
  }

  function navigateToChannel(channelId: string | undefined) {
    if (!channelId) return;
    player.isExpanded = false;
    window.history.pushState({}, '', `/canal/${channelId}`);
    window.dispatchEvent(new PopStateEvent('popstate'));
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!player.currentMedia) return;
    const target = e.target as HTMLElement;
    if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') return;

    if (e.code === 'Space' || e.key === 'k') {
      e.preventDefault();
      player.togglePlay();
    } else if (e.code === 'ArrowLeft') {
      e.preventDefault();
      player.seekRelative(-5);
    } else if (e.code === 'ArrowRight') {
      e.preventDefault();
      player.seekRelative(5);
    } else if (e.key === 'j') {
      e.preventDefault();
      player.seekRelative(-10);
    } else if (e.key === 'l') {
      e.preventDefault();
      player.seekRelative(10);
    } else if (e.code === 'ArrowUp') {
      e.preventDefault();
      player.setVolume(player.volume + 0.1);
    } else if (e.code === 'ArrowDown') {
      e.preventDefault();
      player.setVolume(player.volume - 0.1);
    } else if (e.key === 'm') {
      e.preventDefault();
      player.toggleMute();
    } else if (e.key === 'f') {
      e.preventDefault();
      void toggleFullscreen();
    } else if (e.key === 'c' || e.key === 'C') {
      e.preventDefault();
      toggleSubtitles();
    } else if (e.shiftKey && (e.key === 'N' || e.key === 'n')) {
      e.preventDefault();
      void player.playNext();
    } else if (e.shiftKey && (e.key === 'P' || e.key === 'p')) {
      e.preventDefault();
      void player.playPrevious();
    } else if (e.key === 'q' || (e.shiftKey && e.code === 'Escape')) {
      e.preventDefault();
      void player.stop();
    } else if (e.code === 'Escape') {
      if (showContextMenu) {
        showContextMenu = false;
      } else if (showSubtitleMenu) {
        showSubtitleMenu = false;
      } else if (showChapters) {
        showChapters = false;
      } else if (isFullscreen) {
        void document.exitFullscreen();
      } else if (showSaveModal) {
        showSaveModal = false;
      } else if (player.isExpanded) {
        player.closeExpanded();
      }
    }
  }

  async function toggleFullscreen() {
    const targetEl = videoStageEl ?? containerEl;
    if (!targetEl) return;
    if (!document.fullscreenElement) {
      try {
        await targetEl.requestFullscreen();
        isFullscreen = true;
      } catch (err) {
        console.error('Fullscreen request failed:', err);
      }
    } else {
      try {
        await document.exitFullscreen();
        isFullscreen = false;
      } catch (err) {
        console.error('Exit fullscreen failed:', err);
      }
    }
  }

  function handleMediaClick(event: MouseEvent, item: MediaItem) {
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || event.button !== 0) {
      return;
    }
    event.preventDefault();
    player.playlistContext = null;
    void player.playMedia(item, true);
  }

  function handleAuxClick(event: MouseEvent, item: MediaItem) {
    if (event.button === 1) {
      event.preventDefault();
      window.open(`/watch?v=${item.id}`, '_blank');
    }
  }

  function formatTime(seconds: number): string {
    if (!seconds || isNaN(seconds) || seconds <= 0) return '0:00';
    const total = Math.floor(seconds);
    const h = Math.floor(total / 3600);
    const m = Math.floor((total % 3600) / 60);
    const s = total % 60;
    const pad = (n: number) => n.toString().padStart(2, '0');
    if (h > 0) return `${h}:${pad(m)}:${pad(s)}`;
    return `${m}:${pad(s)}`;
  }

  function formatDuration(ms: number | null): string {
    if (!ms || ms <= 0) return '';
    const totalSecs = Math.floor(ms / 1000);
    const h = Math.floor(totalSecs / 3600);
    const m = Math.floor((totalSecs % 3600) / 60);
    const s = totalSecs % 60;
    const pad = (n: number) => n.toString().padStart(2, '0');
    if (h > 0) return `${h}:${pad(m)}:${pad(s)}`;
    return `${m}:${pad(s)}`;
  }
</script>

<svelte:window
  onkeydown={handleKeydown}
  onclick={() => {
    if (showSubtitleMenu) showSubtitleMenu = false;
  }}
  onfullscreenchange={() => {
    isFullscreen = document.fullscreenElement !== null;
    lastMouseX = -1;
    lastMouseY = -1;
    handleMouseMove();
  }}
/>

<!-- Container do Player persistente -->
<div
  bind:this={containerEl}
  class="video-player-container"
  class:route-hidden={hidden}
  class:expanded={player.isExpanded && player.currentMedia !== null}
  class:minimized={!player.isExpanded && player.currentMedia !== null}
  class:has-media={player.currentMedia !== null}
>
  <div class="watch-page-wrapper">
    <!-- Topbar no modo expandido -->
      <header class="watch-topbar">
        <div class="watch-topbar-left">
          <button
            type="button"
            class="control-btn back-btn"
            onclick={() => player.closeExpanded()}
            title={t('player.minimize')}
          >
            <span class="material-symbols-outlined">arrow_back</span>
            <span class="back-text">{t('common.back')}</span>
          </button>
        </div>

        <div class="watch-topbar-center">
          <SearchBar
            placeholder={t('search.placeholder')}
            onSearchSubmit={() => player.closeExpanded(false)}
          />
        </div>

        <div class="watch-topbar-right">
          <button
            type="button"
            class="control-btn close-btn"
            onclick={() => void player.stop()}
            title={t('player.stop')}
          >
            <span class="material-symbols-outlined">close</span>
          </button>
        </div>
      </header>

      <div
        class="watch-body"
        style="--video-stage-height: {videoStageHeight > 0 ? `${videoStageHeight}px` : 'calc(100vh - 125px)'};"
      >
        <!-- Coluna Principal (Esquerda): Vídeo + Metadados + Ações -->
        <div class="watch-main-column">
          <div
            bind:this={videoStageEl}
            bind:clientHeight={videoStageHeight}
            class="video-stage"
            class:fit-cover={player.videoFit === 'cover'}
            class:controls-active={isControlsActive || !player.isPlaying || showSubtitleMenu || showContextMenu || showChapters}
            onmousemove={(e) => handleMouseMove(e)}
            onmouseleave={() => {
              if (player.isPlaying && !showSubtitleMenu && !showContextMenu && !showChapters) isControlsActive = false;
            }}
            oncontextmenu={handleContextMenu}
            role="region"
            aria-label={t('player.stage_aria')}
          >
            <!-- Painel de Capítulos em TELA CHEIA (Drawer lateral translúcido) -->
            {#if isFullscreen && showChapters && chapters.length > 0 && player.currentMedia}
              <div class="fullscreen-chapters-drawer">
                <ChaptersPanel
                  {chapters}
                  {activeChapter}
                  media={player.currentMedia}
                  source={isUsingNotesChapters ? 'notes' : 'description'}
                  onSeek={(timeSec) => player.seek(timeSec)}
                  onClose={() => (showChapters = false)}
                />
              </div>
            {:else if isFullscreen && !showChapters && player.playlistContext && player.isPlaylistExpanded}
              <div class="fullscreen-chapters-drawer">
                <PlaylistPanel
                  playlist={player.playlistContext}
                  currentIndex={player.currentIndex}
                  isExpanded={player.isPlaylistExpanded}
                  onToggleExpand={() => player.togglePlaylistExpand()}
                  onSelectVideo={(idx) => player.playFromPlaylist(idx)}
                  onToggleLoop={() => player.togglePlaylistLoop()}
                  onToggleShuffle={() => player.togglePlaylistShuffle()}
                />
              </div>
            {/if}
            <!-- svelte-ignore a11y_media_has_caption -->
            <video
              bind:this={videoEl}
              class="native-video"
              playsinline
              preload="auto"
            >
              {#each player.availableSubtitles as sub (sub.id)}
                <track
                  kind="subtitles"
                  src={sub.url}
                  srclang={sub.lang_code}
                  label={sub.label}
                  default={player.selectedSubtitleLang === sub.lang_code}
                  onload={() => {
                    player.syncActiveSubtitleTrack();
                  }}
                />
              {/each}
            </video>

            <!-- Camada transparente de clique que cobre o palco do vídeo: no miniplayer abre a janela do vídeo; expandido alterna play/pause e fullscreen -->
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="video-click-layer"
              onclick={handleStageClick}
              ondblclick={handleStageDblClick}
              role="button"
              tabindex="-1"
              aria-label={player.isExpanded ? "Alternar reprodução do vídeo" : "Abrir janela do vídeo"}
            ></div>

            <!-- Legenda customizada com alta legibilidade e adaptação dinâmica à barra de controles -->
            {#if player.activeCueText}
              <div class="custom-subtitle-layer">
                <div
                  class="custom-subtitle-bubble"
                  class:no-bg={subtitleStyleStore.current.bgOpacity <= 0}
                >
                  {@html formatCueHtml(player.activeCueText)}
                </div>
              </div>
            {/if}

            {#if player.isLoading}
              <div class="video-overlay-state">
                <div class="spinner-large"></div>
              </div>
            {/if}

            {#if player.errorMessage}
              <div class="video-overlay-error">
                <span class="material-symbols-outlined icon-gap-after-md">warning</span>
                <span>{player.errorMessage}</span>
              </div>
            {/if}

            <!-- Controles quando EXPANDIDO -->
            <div class="video-controls-overlay">
              <div class="video-bottom-bar">
                <div class="seek-row">
                  <span class="time-label">{formatTime(player.currentTime)}</span>
                  <div class="seek-slider-container">
                    <input
                      type="range"
                      class="seek-slider"
                      min="0"
                      max={player.duration || 100}
                      step="0.1"
                      value={player.currentTime}
                      oninput={(e) => player.seek(Number((e.currentTarget as HTMLInputElement).value))}
                    />
                    {#if chapters.length > 1 && player.duration > 0}
                      <div class="seek-chapter-ticks" aria-hidden="true">
                        {#each chapters.slice(1) as ch (ch.id)}
                          <div
                            class="seek-chapter-tick"
                            style="left: {(ch.startTime / player.duration) * 100}%;"
                            title="{ch.title} ({ch.timeFormatted})"
                          ></div>
                        {/each}
                      </div>
                    {/if}
                  </div>
                  <span class="time-label">{formatTime(player.duration)}</span>
                </div>

                <div class="controls-bottom">
                  <div class="left-controls">
                    {#if player.playlistContext}
                      <button
                        type="button"
                        class="control-btn"
                        onclick={() => player.playPrevious()}
                        title={t('player.previous')}
                      >
                        <span class="material-symbols-outlined">skip_previous</span>
                      </button>
                    {/if}

                    <button
                      type="button"
                      class="control-btn play-toggle"
                      onclick={() => player.togglePlay()}
                      title={player.isPlaying ? 'Pausar (Espaço)' : 'Reproduzir (Espaço)'}
                    >
                      <span class="material-symbols-outlined">{player.isPlaying ? 'pause' : 'play_arrow'}</span>
                    </button>

                    {#if player.playlistContext}
                      <button
                        type="button"
                        class="control-btn"
                        onclick={() => player.playNext()}
                        title={t('player.next')}
                      >
                        <span class="material-symbols-outlined">skip_next</span>
                      </button>
                    {/if}

                    <button
                      type="button"
                      class="control-btn"
                      onclick={() => player.seekRelative(-10)}
                      title={t('player.rewind_10')}
                    >
                      <span class="material-symbols-outlined icon-18 icon-gap-after">replay_10</span>
                      10s
                    </button>
                    <button
                      type="button"
                      class="control-btn"
                      onclick={() => player.seekRelative(10)}
                      title={t('player.forward_10')}
                    >
                      <span class="material-symbols-outlined icon-18 icon-gap-after">forward_10</span>
                      10s
                    </button>

                    <!-- Volume -->
                    <div class="volume-group">
                      <button
                        type="button"
                        class="control-btn"
                        onclick={() => player.toggleMute()}
                        title={player.isMuted ? 'Desmutar (M)' : 'Mutar (M)'}
                      >
                        <span class="material-symbols-outlined">{player.isMuted || player.volume === 0 ? 'volume_off' : 'volume_up'}</span>
                      </button>
                      <input
                        type="range"
                        class="volume-slider"
                        min="0"
                        max="1"
                        step="0.05"
                        value={player.isMuted ? 0 : player.volume}
                        oninput={(e) => player.setVolume(Number((e.currentTarget as HTMLInputElement).value))}
                      />
                    </div>

                    <!-- Botão de Capítulo Ativo -->
                    {#if chapters.length > 0 && activeChapter}
                      <button
                        type="button"
                        class="control-chapter-badge"
                        class:active={showChapters}
                        onclick={() => {
                          showChapters = !showChapters;
                          if (showChapters && player.playlistContext) {
                            player.isPlaylistExpanded = false;
                          }
                        }}
                        title={t('player.chapter_badge_tip')}
                        aria-label={t('player.open_chapter_selection')}
                      >
                        <span class="chapter-dot">•</span>
                        <span class="chapter-badge-title" title={activeChapter.title}>{activeChapter.title}</span>
                        <span class="material-symbols-outlined chapter-badge-chevron">chevron_right</span>
                      </button>
                    {/if}
                  </div>

                  <div class="right-controls">
                    <div class="speed-group">
                      {#each [0.75, 1.0, 1.25, 1.5, 2.0] as rate}
                        <button
                          type="button"
                          class="speed-chip"
                          class:active={player.playbackRate === rate}
                          onclick={() => player.setSpeed(rate)}
                        >
                          {rate}x
                        </button>
                      {/each}
                    </div>

                    <button
                      type="button"
                      class="autoplay-toggle"
                      class:active={player.isAutoplayEnabled}
                      aria-pressed={player.isAutoplayEnabled}
                      onclick={() => player.toggleAutoplay()}
                      title={player.isAutoplayEnabled ? t('player.autoplay_disable') : t('player.autoplay_enable')}
                    >
                      <span class="material-symbols-outlined">autoplay</span>
                      <span>{t('player.autoplay')}</span>
                    </button>

                    <!-- Ajuste de Proporção (Normal 16:9 vs Zoom 21:9 Ultrawide) -->
                    <button
                      type="button"
                      class="control-btn fit-btn"
                      class:active={player.videoFit === 'cover'}
                      onclick={(e) => {
                        player.toggleFit();
                        (e.currentTarget as HTMLElement)?.blur();
                      }}
                      title={player.videoFit === 'cover' ? t('player.fit_cover_tip') : t('player.fit_normal_tip')}
                      aria-label={t('player.adjust_aspect_ratio')}
                    >
                      <span class="material-symbols-outlined">
                        {player.videoFit === 'cover' ? 'fit_screen' : 'aspect_ratio'}
                      </span>
                    </button>

                    <!-- Legendas / Subtitles Menu Trigger -->
                    <div class="subtitles-control-group">
                      <button
                        type="button"
                        class="control-btn cc-btn"
                        class:active={player.selectedSubtitleLang !== null}
                        onclick={(e) => {
                          e.stopPropagation();
                          showSubtitleMenu = !showSubtitleMenu;
                          if (showSubtitleMenu) subtitleSubmenu = 'list';
                        }}
                        title={t('player.subtitles')}
                        aria-label={t('player.subtitles')}
                      >
                        <span class="material-symbols-outlined">
                          {player.selectedSubtitleLang !== null ? 'closed_caption' : 'closed_caption_disabled'}
                        </span>
                        {#if player.availableSubtitles.length > 0}
                          <span class="cc-badge">{player.availableSubtitles.length}</span>
                        {/if}
                      </button>

                      {#if showSubtitleMenu}
                        <div
                          class="subtitles-popover"
                          onclick={(e) => e.stopPropagation()}
                          onkeydown={(e) => { if (e.key === 'Escape') showSubtitleMenu = false; }}
                          role="dialog"
                          tabindex="-1"
                          aria-label={t('player.subtitles_aria')}
                        >
                          {#if subtitleSubmenu === 'list'}
                            <div class="subtitles-popover-header">
                              <div class="subtitles-popover-title">
                                <span class="material-symbols-outlined icon-18 icon-gap-after-md">closed_caption</span>
                                <strong>{t('subtitles.title')}</strong>
                              </div>
                              <div class="popover-header-actions">
                                <button
                                  type="button"
                                  class="popover-icon-btn"
                                  onclick={() => (subtitleSubmenu = 'settings')}
                                  title={t('subtitles.adjust_style')}
                                  aria-label={t('subtitles.adjust_style')}
                                >
                                  <span class="material-symbols-outlined icon-17">tune</span>
                                </button>
                                <button
                                  type="button"
                                  class="popover-close-btn"
                                  onclick={() => (showSubtitleMenu = false)}
                                  aria-label={t('common.close')}
                                >
                                  <span class="material-symbols-outlined icon-16">close</span>
                                </button>
                              </div>
                            </div>

                            <div class="subtitles-options-list">
                              <button
                                type="button"
                                class="subtitle-option-row"
                                class:selected={player.selectedSubtitleLang === null}
                                onclick={() => {
                                  player.setSubtitleLang(null);
                                  showSubtitleMenu = false;
                                }}
                              >
                                <span class="option-check material-symbols-outlined">
                                  {player.selectedSubtitleLang === null ? 'check' : ''}
                                </span>
                                <span class="option-label">{t('subtitles.disabled')}</span>
                              </button>

                              {#each player.availableSubtitles as sub (sub.id)}
                                <button
                                  type="button"
                                  class="subtitle-option-row"
                                  class:selected={player.selectedSubtitleLang === sub.lang_code}
                                  onclick={() => {
                                    player.setSubtitleLang(sub.lang_code);
                                    showSubtitleMenu = false;
                                  }}
                                >
                                  <span class="option-check material-symbols-outlined">
                                    {player.selectedSubtitleLang === sub.lang_code ? 'check' : ''}
                                  </span>
                                  <span class="option-label">{sub.label}</span>
                                  {#if sub.source === 'youtube_auto'}
                                    <span class="option-badge">{t('subtitles.auto')}</span>
                                  {/if}
                                  <span class="option-code">{sub.lang_code}</span>
                                </button>
                              {/each}
                            </div>

                            {#if player.availableSubtitles.length === 0}
                              <div class="subtitles-empty-tip">
                                {t('subtitles.no_subs_available')}
                              </div>
                            {/if}

                            <div class="subtitles-popover-footer">
                              <button
                                type="button"
                                class="sub-quick-settings-btn"
                                onclick={() => (subtitleSubmenu = 'settings')}
                              >
                                <span class="material-symbols-outlined icon-16 icon-gap-after-md">tune</span>
                                <span>{t('subtitles.quick_adjust_btn')}</span>
                                <span class="material-symbols-outlined icon-16 icon-push-end">chevron_right</span>
                              </button>

                              {#if player.currentMedia?.youtube_id}
                                <button
                                  type="button"
                                  class="download-yt-subs-btn"
                                  onclick={async () => {
                                    if (!player.currentMedia) return;
                                    await player.downloadSubtitles(player.currentMedia.id);
                                  }}
                                  disabled={player.isDownloadingSubtitles}
                                >
                                  <span class="material-symbols-outlined icon-16 icon-gap-after-md">
                                    {player.isDownloadingSubtitles ? 'sync' : 'download'}
                                  </span>
                                  <span>
                                    {player.isDownloadingSubtitles
                                      ? t('subtitles.search_youtube_btn')
                                      : player.availableSubtitles.length === 0
                                      ? t('subtitles.search_youtube')
                                      : t('subtitles.update_youtube')}
                                  </span>
                                </button>
                              {/if}
                            </div>
                          {:else}
                            <!-- Mini Menu de Configurações Básicas de Estilo -->
                            <div class="subtitles-popover-header">
                              <div class="subtitles-header-left">
                                <button
                                  type="button"
                                  class="popover-back-btn"
                                  onclick={() => (subtitleSubmenu = 'list')}
                                  title={t('subtitles.back_to_list')}
                                  aria-label={t('common.back')}
                                >
                                  <span class="material-symbols-outlined icon-18">arrow_back</span>
                                </button>
                                <strong>{t('subtitles.style_header')}</strong>
                              </div>
                              <button
                                type="button"
                                class="popover-close-btn"
                                onclick={() => (showSubtitleMenu = false)}
                                aria-label={t('common.close_menu')}
                              >
                                <span class="material-symbols-outlined icon-16">close</span>
                              </button>
                            </div>

                            <div class="subtitles-quick-settings-body">
                              <!-- TAMANHO -->
                              <div class="quick-setting-section">
                                <span class="quick-setting-label">{t('subtitles.size_label')}</span>
                                <div class="quick-pills-row">
                                  {#each [75, 100, 125, 150, 200] as sizePercent}
                                    <button
                                      type="button"
                                      class="quick-pill-btn"
                                      class:active={subtitleStyleStore.current.fontSizePercent === sizePercent}
                                      onclick={() => subtitleStyleStore.update({ fontSizePercent: sizePercent })}
                                    >
                                      {sizePercent}%
                                    </button>
                                  {/each}
                                </div>
                              </div>

                              <!-- COR DO TEXTO (Branco / Amarelo) -->
                              <div class="quick-setting-section">
                                <span class="quick-setting-label">{t('subtitles.color_label')}</span>
                                <div class="quick-pills-row">
                                  <button
                                    type="button"
                                    class="quick-pill-btn color-pill"
                                    class:active={subtitleStyleStore.current.textColor.toLowerCase() === '#ffffff'}
                                    onclick={() => subtitleStyleStore.update({ textColor: '#ffffff' })}
                                  >
                                    <span class="color-dot color-dot--white"></span>
                                    <span>{t('subtitles.white')}</span>
                                  </button>
                                  <button
                                    type="button"
                                    class="quick-pill-btn color-pill"
                                    class:active={subtitleStyleStore.current.textColor.toLowerCase() !== '#ffffff'}
                                    onclick={() => subtitleStyleStore.update({ textColor: '#ffff00' })}
                                  >
                                    <span class="color-dot color-dot--yellow"></span>
                                    <span>{t('subtitles.yellow')}</span>
                                  </button>
                                </div>
                              </div>

                              <!-- FUNDO (Sem fundo / Com fundo) -->
                              <div class="quick-setting-section">
                                <span class="quick-setting-label">{t('subtitles.background_label')}</span>
                                <div class="quick-pills-row">
                                  <button
                                    type="button"
                                    class="quick-pill-btn"
                                    class:active={subtitleStyleStore.current.bgOpacity === 0}
                                    onclick={() => subtitleStyleStore.update({ bgOpacity: 0 })}
                                  >
                                    {t('subtitles.no_background')}
                                  </button>
                                  <button
                                    type="button"
                                    class="quick-pill-btn"
                                    class:active={subtitleStyleStore.current.bgOpacity > 0}
                                    onclick={() => subtitleStyleStore.update({ bgOpacity: 75, bgColor: '#000000' })}
                                  >
                                    {t('subtitles.with_background')}
                                  </button>
                                </div>
                              </div>

                              <!-- PRÉ-VISUALIZAÇÃO DA LEGENDA -->
                              <div class="quick-sub-preview">
                                <div
                                  class="preview-box-text"
                                  style="
                                    color: {subtitleStyleStore.current.textColor};
                                    background: {subtitleStyleStore.current.bgOpacity > 0 ? hexToRgba(subtitleStyleStore.current.bgColor, subtitleStyleStore.current.bgOpacity) : 'transparent'};
                                    font-size: {(subtitleStyleStore.current.fontSizePercent / 100) * 0.9}rem;
                                    text-shadow: {subtitleStyleStore.current.textShadow === 'outline' ? '-1.5px -1.5px 0 #000, 1.5px -1.5px 0 #000, -1.5px 1.5px 0 #000, 1.5px 1.5px 0 #000, 0 2px 4px rgba(0,0,0,0.9)' : 'none'};
                                  "
                                >
                                  {t('subtitles.preview_sample')}
                                </div>
                              </div>
                            </div>

                            <div class="subtitles-popover-footer">
                              <a
                                href="/configuracoes?tab=legendas"
                                class="sub-advanced-link"
                                onclick={(e) => {
                                  e.preventDefault();
                                  showSubtitleMenu = false;
                                  if (player.isExpanded) player.closeExpanded(false);
                                  window.history.pushState({}, '', '/configuracoes?tab=legendas');
                                  window.dispatchEvent(new PopStateEvent('popstate'));
                                }}
                              >
                                <span>{t('subtitles.advanced_link')}</span>
                                <span class="material-symbols-outlined icon-14">open_in_new</span>
                              </a>
                            </div>
                          {/if}
                        </div>
                      {/if}
                    </div>

                    <button
                      type="button"
                      class="control-btn"
                      onclick={() => void toggleFullscreen()}
                      title={t('player.fullscreen_tip')}
                    >
                      <span class="material-symbols-outlined">{isFullscreen ? 'fullscreen_exit' : 'fullscreen'}</span>
                    </button>
                  </div>
                </div>
              </div>
            </div>

            <!-- Controles quando MINIMIZADO (Miniplayer overlay no canto) -->
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="miniplayer-controls-overlay"
              onclick={(e) => {
                e.stopPropagation();
                player.openExpanded();
              }}
              role="button"
              tabindex="-1"
              aria-label={t('player.expand_tip')}
            >
              <button
                type="button"
                class="mini-btn expand-btn"
                onclick={(e) => {
                  e.stopPropagation();
                  player.openExpanded();
                }}
                title={t('player.expand_tip')}
                aria-label={t('player.expand_tip')}
              >
                <span class="material-symbols-outlined icon-20">open_in_full</span>
              </button>

              <button
                type="button"
                class="mini-btn play-btn"
                onclick={(e) => {
                  e.stopPropagation();
                  void player.togglePlay();
                }}
                title={player.isPlaying ? t('player.pause') : t('player.play')}
                aria-label={player.isPlaying ? t('player.pause') : t('player.play')}
              >
                <span class="material-symbols-outlined icon-32">
                  {player.isPlaying ? 'pause' : 'play_arrow'}
                </span>
              </button>

              <button
                type="button"
                class="mini-btn close-btn"
                onclick={(e) => {
                  e.stopPropagation();
                  void player.stop();
                }}
                title={t('player.stop')}
                aria-label={t('player.stop')}
              >
                <span class="material-symbols-outlined icon-20">close</span>
              </button>

              <div class="mini-progress-track">
                <div
                  class="mini-progress-fill"
                  style="width: {player.duration ? (player.currentTime / player.duration) * 100 : 0}%"
                ></div>
              </div>
            </div>
          </div>

          <!-- Rodapé do miniplayer (visível apenas quando minimizado) -->
          <div
            class="miniplayer-footer"
            onclick={() => player.openExpanded()}
            role="button"
            tabindex="0"
            onkeydown={(e) => e.key === 'Enter' && player.openExpanded()}
          >
            <span class="mini-title" title={player.currentMedia?.title ?? ''}>{player.currentMedia?.title ?? ''}</span>
            {#if player.currentMedia?.channel}
              <button
                type="button"
                class="mini-channel mini-channel-btn"
                onclick={(e) => {
                  e.stopPropagation();
                  navigateToChannel(player.currentMedia?.channel?.id);
                }}
                title={t('channels.access_channel')}
              >
                <span class="material-symbols-outlined icon-13 icon-gap-after">subscriptions</span>
                {player.currentMedia.channel.name}
              </button>
            {/if}
          </div>

          <!-- Metadados e Ações abaixo do vídeo quando EXPANDIDO -->
          <div class="watch-meta-area">
            <h1 class="watch-title">{player.currentMedia?.title ?? ''}</h1>

            <div class="watch-actions-row">
              {#if player.currentMedia?.channel?.id}
                <button
                  type="button"
                  class="watch-channel-badge watch-channel-clickable"
                  onclick={() => navigateToChannel(player.currentMedia?.channel?.id)}
                  title={t('channels.access_channel')}
                >
                  {#if player.currentMedia?.channel?.avatar_url}
                    <img src={player.currentMedia.channel.avatar_url} alt="" class="watch-channel-avatar" />
                  {:else}
                    <span class="material-symbols-outlined channel-icon">subscriptions</span>
                  {/if}
                  <div class="channel-info-stack">
                    <span class="channel-name">{player.currentMedia?.channel?.name ?? 'Biblioteca Pessoal'}</span>
                    {#if player.currentMedia?.channel?.handle}
                      <span class="channel-handle">{player.currentMedia.channel.handle}</span>
                    {/if}
                  </div>
                </button>
              {:else}
                <div class="watch-channel-badge">
                  {#if player.currentMedia?.channel?.avatar_url}
                    <img src={player.currentMedia.channel.avatar_url} alt="" class="watch-channel-avatar" />
                  {:else}
                    <span class="material-symbols-outlined channel-icon">subscriptions</span>
                  {/if}
                  <div class="channel-info-stack">
                    <span class="channel-name">{player.currentMedia?.channel?.name ?? 'Biblioteca Pessoal'}</span>
                    {#if player.currentMedia?.channel?.handle}
                      <span class="channel-handle">{player.currentMedia.channel.handle}</span>
                    {/if}
                  </div>
                </div>
              {/if}

              <div class="watch-action-buttons">
                <button
                  type="button"
                  class="watch-action-btn"
                  class:active={player.currentMedia?.is_favorite}
                  onclick={toggleFavorite}
                  title={player.currentMedia?.is_favorite ? t('common.unfavorite') : t('common.favorite')}
                >
                  <span class="material-symbols-outlined">
                    {player.currentMedia?.is_favorite ? 'favorite' : 'favorite_border'}
                  </span>
                  <span>{player.currentMedia?.is_favorite ? t('common.favorited') : t('common.favorite')}</span>
                </button>

                <button
                  type="button"
                  class="watch-action-btn"
                  onclick={openSaveModal}
                  title={t('common.save_to_playlist')}
                >
                  <span class="material-symbols-outlined">playlist_add</span>
                  <span>{t('common.save')}</span>
                </button>

                {#if musicStore.settings.enabled}
                  {#if musicStore.currentTrack}
                    <button
                      type="button"
                      class="watch-action-btn music-badge-btn"
                      onclick={() => (showMusicModal = true)}
                      title={`${musicStore.currentTrack.artist_name} - ${musicStore.currentTrack.track_name} (${t('music.click_to_manage')})`}
                    >
                      <span class="material-symbols-outlined">music_note</span>
                      <span class="music-badge-label">
                        {musicStore.currentTrack.artist_name} - {musicStore.currentTrack.track_name}
                      </span>
                    </button>
                  {:else}
                    <button
                      type="button"
                      class="watch-action-btn"
                      onclick={() => (showMusicModal = true)}
                      title={t('music.identify_button')}
                    >
                      <span class="material-symbols-outlined">music_note</span>
                      <span>{t('music.identify')}</span>
                    </button>
                  {/if}
                {/if}
              </div>
            </div>

            {#if player.currentMedia?.tags && player.currentMedia.tags.length > 0}
              <div class="watch-tags-row">
                {#each player.currentMedia.tags as tag (tag.id)}
                  <span class="watch-tag-chip">#{tag.display_name}</span>
                {/each}
              </div>
            {/if}

            {#if player.currentMedia}
              <VideoInfoTabs
                media={player.currentMedia}
                onSeek={(timeSec) => player.seek(timeSec)}
                isOriginalChaptersActive={isUsingOriginalChapters}
                isNotesChaptersActive={isUsingNotesChapters}
                hasNotesChapters={notesChapters.length > 0}
                onNotesSaved={(updatedNotes) => {
                  player.updateCurrentMediaNotes(updatedNotes);
                }}
              />
            {/if}
          </div>
        </div>

        <!-- Coluna Lateral (Direita): Capítulos, Playlist e Vídeos Relacionados quando EXPANDIDO -->
        <aside class="watch-sidebar">
          {#if showChapters && !isFullscreen && chapters.length > 0 && player.currentMedia}
            <div class="sidebar-chapters-container">
              <ChaptersPanel
                {chapters}
                {activeChapter}
                media={player.currentMedia}
                source={isUsingNotesChapters ? 'notes' : 'description'}
                onSeek={(timeSec) => player.seek(timeSec)}
                onClose={() => (showChapters = false)}
              />
            </div>
          {/if}

          {#if player.playlistContext && !isFullscreen}
            <div class="sidebar-playlist-container" class:collapsed={!player.isPlaylistExpanded}>
              <PlaylistPanel
                playlist={player.playlistContext}
                currentIndex={player.currentIndex}
                isExpanded={player.isPlaylistExpanded}
                onToggleExpand={() => player.togglePlaylistExpand()}
                onSelectVideo={(idx) => player.playFromPlaylist(idx)}
                onToggleLoop={() => player.togglePlaylistLoop()}
                onToggleShuffle={() => player.togglePlaylistShuffle()}
              />
            </div>
          {/if}

          <div class="watch-sidebar-header">
            <h3>{t('player.related_videos')}</h3>
          </div>

          {#if loadingRelated}
            <div class="sidebar-loading">
              <div class="spinner-small"></div>
              <span>{t('player.loading_suggestions')}</span>
            </div>
          {:else if relatedVideos.length === 0}
            <div class="sidebar-empty">
              <p>{t('player.no_related_videos')}</p>
            </div>
          {:else}
            <div class="related-list">
              {#each relatedVideos as rel, index (rel.id)}
                {@const playback = player.getPlayback(rel.id) ?? rel.playback}
                <div
                  class="related-card"
                  role="button"
                  tabindex="0"
                  onclick={(e) => handleMediaClick(e, rel)}
                  onauxclick={(e) => handleAuxClick(e, rel)}
                  onkeydown={(e) => {
                    if (e.target !== e.currentTarget || (e.key !== 'Enter' && e.key !== ' ')) return;
                    e.preventDefault();
                    void player.playMedia(rel, true);
                  }}
                >
                  <div class="related-thumb-wrap">
                    <a
                      href={`/watch?v=${rel.id}`}
                      class="related-thumb-link"
                      onclick={(e) => handleMediaClick(e, rel)}
                      aria-label={rel.title}
                    >
                    <img src={rel.thumbnail_url} alt={rel.title} class="related-thumb" loading={index < 6 ? 'eager' : 'lazy'} decoding={index < 4 ? 'sync' : 'async'} fetchpriority={index < 2 ? 'high' : 'auto'} />
                    </a>

                    <div class="thumb-top-left-badges">
                      {#if rel.is_short}
                        <span class="badge short-badge">Short</span>
                      {/if}
                      {#if rel.is_official_thumbnail}
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
                    {#if rel.duration_ms}
                      <span class="badge duration-badge">
                        {formatDuration(rel.duration_ms)}
                      </span>
                    {/if}

                    <div class="card-top-right-actions">
                      <button
                        type="button"
                        class="card-action-btn card-playlist-btn"
                        onclick={(e) => {
                          e.stopPropagation();
                          playlistModalMedia = rel;
                        }}
                        title={t('common.save_to_playlist')}
                        aria-label={t('common.save_to_playlist')}
                      >
                        <span class="material-symbols-outlined">playlist_add</span>
                      </button>
                      <button
                        type="button"
                        class="card-action-btn card-fav-btn"
                        class:favorited={rel.is_favorite}
                        onclick={(e) => {
                          e.stopPropagation();
                          void player.toggleFavorite(rel);
                        }}
                        title={rel.is_favorite ? t('common.unfavorite') : t('common.favorite')}
                        aria-label={rel.is_favorite ? t('common.unfavorite') : t('common.favorite')}
                      >
                        <span class="material-symbols-outlined">
                          {rel.is_favorite ? 'favorite' : 'favorite_border'}
                        </span>
                      </button>
                    </div>

                    {#if !playback?.completed && playback?.position_ms && rel.duration_ms && rel.duration_ms > 0}
                      <div class="playback-progress-track">
                        <div
                          class="playback-progress-bar"
                          style="width: {Math.min(100, Math.round((playback.position_ms / rel.duration_ms) * 100))}%"
                        ></div>
                      </div>
                    {/if}
                  </div>
                  <div class="related-info">
                    <a
                      href={`/watch?v=${rel.id}`}
                      class="related-title-link"
                      onclick={(e) => handleMediaClick(e, rel)}
                    >
                      <h4 class="related-title" title={rel.title}>{rel.title}</h4>
                    </a>
                    {#if rel.channel}
                      <span class="related-channel">
                        <span class="material-symbols-outlined icon-13 icon-gap-after">subscriptions</span>
                        {rel.channel.name}
                      </span>
                    {/if}
                  </div>
                </div>
              {/each}
            </div>
          {/if}
        </aside>
      </div>
    </div>
</div>

<!-- Modal para salvar em playlist -->
{#if showSaveModal && player.currentMedia}
  <SaveToPlaylistModal
    media={player.currentMedia}
    onClose={() => (showSaveModal = false)}
  />
{/if}

{#if playlistModalMedia}
  <SaveToPlaylistModal
    media={playlistModalMedia}
    onClose={() => (playlistModalMedia = null)}
  />
{/if}

{#if showMusicModal && player.currentMedia}
  <IdentifyMusicModal
    media={player.currentMedia}
    isOpen={showMusicModal}
    onClose={() => (showMusicModal = false)}
  />
{/if}

<!-- Context Menu do Player (botão direito: Loop, Copiar URL) -->
{#if showContextMenu}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="video-context-menu-backdrop"
    onclick={() => (showContextMenu = false)}
    oncontextmenu={(e) => {
      e.preventDefault();
      showContextMenu = false;
    }}
  >
    <div
      class="video-context-menu"
      style="left: {contextMenuPos.x}px; top: {contextMenuPos.y}px;"
      onclick={(e) => e.stopPropagation()}
    >
      <button
        type="button"
        class="context-menu-item"
        class:active={player.isLooping}
        onclick={() => {
          player.toggleLoop();
          showContextMenu = false;
        }}
      >
        <span class="context-item-icon material-symbols-outlined">
          {player.isLooping ? 'check' : 'repeat'}
        </span>
        <span class="context-item-label">{t('player.loop')}</span>
        {#if player.isLooping}
          <span class="context-item-badge">{t('common.active')}</span>
        {/if}
      </button>

      <div class="context-menu-divider"></div>

      <button
        type="button"
        class="context-menu-item"
        onclick={() => copyVideoUrl(false)}
      >
        <span class="context-item-icon material-symbols-outlined">link</span>
        <span class="context-item-label">{t('player.copy_url')}</span>
      </button>

      <button
        type="button"
        class="context-menu-item"
        onclick={() => copyVideoUrl(true)}
      >
        <span class="context-item-icon material-symbols-outlined">schedule</span>
        <span class="context-item-label">{t('player.copy_url_at_time')}</span>
      </button>

      {#if copyFeedback}
        <div class="context-menu-feedback">
          <span class="material-symbols-outlined icon-14">check_circle</span>
          <span>{copyFeedback}</span>
        </div>
      {/if}
    </div>
  </div>
{/if}

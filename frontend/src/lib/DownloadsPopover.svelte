<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { apiFetch } from './api';
  import { player, type MediaItem } from './player.svelte';
  import { t } from './i18n/i18n.svelte';
  import Notice from './ui/Notice.svelte';

  type ExistingLibraryMedia = {
    media_id: number;
    title: string;
    resolution: string;
    height: number | null;
  };

  type VideoProbeResponse = {
    youtube_id: string;
    title: string;
    channel_name: string;
    thumbnail_url: string | null;
    duration_ms: number | null;
    is_short: boolean;
    available_qualities: string[];
    existing_media?: ExistingLibraryMedia | null;
  };

  type DownloadJob = {
    id: string;
    url: string;
    youtube_id: string | null;
    title: string | null;
    channel_name: string | null;
    thumbnail_url: string | null;
    duration_ms: number | null;
    format_container: string;
    quality: string;
    status: 'queued' | 'probing' | 'downloading' | 'organizing' | 'awaiting_index' | 'completed' | 'failed' | 'cancelled';
    progress_percent: number;
    speed_text: string | null;
    eta_text: string | null;
    output_path: string | null;
    media_id: number | null;
    error_message: string | null;
    created_at: string;
    updated_at: string;
  };

  type DownloadSettings = {
    target_source_id: number | null;
    preferred_container: string;
  };

  type DownloadPlaylistSummary = {
    id: string;
    title: string;
    channel_name: string | null;
    thumbnail_url: string | null;
    status: 'importing' | 'ready' | 'running' | 'paused' | 'paused_rate_limit' | 'completed' | 'completed_with_errors' | 'cancelled' | 'failed';
    quality: string;
    format_container: 'mkv' | 'mp4';
    create_local_playlist: boolean;
    local_playlist_id: string | null;
    download_profile: 'conservative';
    cooldown_until: string | null;
    protection_reason: string | null;
    item_count: number;
    selected_count: number;
    completed_count: number;
    failed_count: number;
    unavailable_count: number;
    pending_count: number;
    active_count: number;
    skipped_existing_count: number;
    cancelled_count: number;
    created_at: string;
    updated_at: string;
  };

  type ManagerTab = 'videos' | 'playlists';

  type DownloadPlaylistItem = {
    position: number;
    youtube_id: string | null;
    title: string;
    channel_name: string | null;
    thumbnail_url: string | null;
    duration_ms: number | null;
    availability: string;
    selected: boolean;
    status: string;
    download_job_id: string | null;
    media_id: number | null;
    error_message: string | null;
    progress_percent: number;
    speed_text: string | null;
    eta_text: string | null;
  };

  type DownloadPlaylistDetail = DownloadPlaylistSummary & {
    items: DownloadPlaylistItem[];
    limit: number;
    offset: number;
  };

  let isOpen = $state(false);
  let triggerButton = $state<HTMLButtonElement | null>(null);
  let closeButton = $state<HTMLButtonElement | null>(null);
  let dialogElement = $state<HTMLDivElement | null>(null);
  let urlInput = $state('');
  let probing = $state(false);
  let probeError = $state<string | null>(null);
  let probedVideo = $state<VideoProbeResponse | null>(null);
  let activeTab = $state<ManagerTab>('videos');
  let createLocalPlaylist = $state(true);

  let selectedQuality = $state('best');
  let selectedContainer = $state<'mkv' | 'mp4'>('mkv');
  let startingDownload = $state(false);

  let jobs = $state<DownloadJob[]>([]);
  let downloadPlaylists = $state<DownloadPlaylistSummary[]>([]);
  let playlistActionBusy = $state<string | null>(null);
  let selectedPlaylistDetail = $state<DownloadPlaylistDetail | null>(null);
  let loadingPlaylistDetail = $state(false);
  let updatingPlaylistSelection = $state(false);
  let playlistItemActionBusy = $state<string | null>(null);
  let pollInterval: number | null = null;

  let activeCount = $derived(
    jobs.filter((j) => ['downloading', 'queued', 'organizing', 'awaiting_index'].includes(j.status)).length
  );
  let activePlaylistCount = $derived(
    downloadPlaylists.filter((playlist) => ['importing', 'running'].includes(playlist.status)).length
  );
  let attentionPlaylistCount = $derived(
    downloadPlaylists.filter((playlist) => playlist.status === 'paused_rate_limit').length
  );
  let totalActiveCount = $derived(activeCount + activePlaylistCount);

  onMount(() => {
    void loadJobs();
    void loadPlaylists();
    void loadSettings();
    const savedTab = window.localStorage.getItem('zmydia-download-manager-tab');
    if (savedTab === 'videos' || savedTab === 'playlists') activeTab = savedTab;
    startPolling();
    return () => stopPolling();
  });

  function startPolling() {
    stopPolling();
    pollInterval = window.setInterval(() => {
      // Poll every 1.2s if popover is open or if there are active downloads
      if (isOpen || totalActiveCount > 0) {
        void loadJobs();
        void loadPlaylists();
        if (selectedPlaylistDetail) void refreshSelectedPlaylistDetail();
      }
    }, 1200);
  }

  function stopPolling() {
    if (pollInterval !== null) {
      clearInterval(pollInterval);
      pollInterval = null;
    }
  }

  async function loadSettings() {
    try {
      const res = await apiFetch('/api/settings/downloads');
      if (res.ok) {
        const s = (await res.json()) as DownloadSettings;
        if (s.preferred_container === 'mp4' || s.preferred_container === 'mkv') {
          selectedContainer = s.preferred_container;
        }
      }
    } catch {
      // Ignora erro
    }
  }

  async function loadJobs() {
    try {
      const res = await apiFetch('/api/downloads');
      if (res.ok) {
        jobs = (await res.json()) as DownloadJob[];
      }
    } catch {
      // Falha silenciosa de polling
    }
  }

  async function loadPlaylists() {
    try {
      const res = await apiFetch('/api/download-playlists');
      if (res.ok) downloadPlaylists = (await res.json()) as DownloadPlaylistSummary[];
    } catch {
      // Falha silenciosa de polling
    }
  }

  function selectTab(tab: ManagerTab) {
    activeTab = tab;
    window.localStorage.setItem('zmydia-download-manager-tab', tab);
  }

  async function togglePopover() {
    isOpen = !isOpen;
    if (isOpen) {
      void loadJobs();
      void loadPlaylists();
      await tick();
      closeButton?.focus();
    }
  }

  function closePopover(restoreFocus = true) {
    isOpen = false;
    if (restoreFocus) void tick().then(() => triggerButton?.focus());
  }

  function handleWindowKeydown(event: KeyboardEvent) {
    if (isOpen && event.key === 'Escape') {
      event.preventDefault();
      closePopover();
      return;
    }
    if (isOpen && event.key === 'Tab' && dialogElement) {
      const focusable = Array.from(
        dialogElement.querySelectorAll<HTMLElement>('button:not(:disabled), a[href], input:not(:disabled), select:not(:disabled), [tabindex]:not([tabindex="-1"])')
      ).filter((element) => !element.hasAttribute('hidden'));
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }
  }

  async function handlePaste() {
    try {
      const text = await navigator.clipboard.readText();
      if (text && (text.includes('youtube.com') || text.includes('youtu.be'))) {
        urlInput = text.trim();
        await handleProbe();
      }
    } catch {
      // Clipboard sem permissão
    }
  }

  async function handleProbe() {
    const raw = urlInput.trim();
    if (!raw) return;
    probing = true;
    probeError = null;
    probedVideo = null;
    try {
      if (isDirectPlaylistUrl(raw)) {
        selectTab('playlists');
        const playlistRes = await apiFetch('/api/download-playlists', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({
            url: raw,
            quality: selectedQuality,
            container: selectedContainer,
            create_local_playlist: createLocalPlaylist
          })
        });
        if (!playlistRes.ok) {
          const errText = await playlistRes.text();
          throw new Error(errText || t('downloads.playlist_import_failed'));
        }
        urlInput = '';
        await loadPlaylists();
        return;
      }
      selectTab('videos');
      const res = await apiFetch('/api/downloads/probe', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ url: raw })
      });
      if (!res.ok) {
        const errText = await res.text();
        throw new Error(errText || 'Falha ao consultar vídeo');
      }
      probedVideo = (await res.json()) as VideoProbeResponse;
      selectedQuality = 'best';
    } catch (e) {
      probeError = e instanceof Error ? e.message : String(e);
    } finally {
      probing = false;
    }
  }

  function isDirectPlaylistUrl(raw: string): boolean {
    try {
      const parsed = new URL(raw);
      return parsed.protocol === 'https:' && parsed.pathname === '/playlist' && parsed.searchParams.has('list');
    } catch {
      return false;
    }
  }

  function playlistStatusLabel(status: DownloadPlaylistSummary['status']): string {
    return t(`downloads.playlist_status_${status}`);
  }

  function playlistDoneCount(playlist: DownloadPlaylistSummary): number {
    return playlist.completed_count + playlist.skipped_existing_count;
  }

  function playlistActivityText(playlist: DownloadPlaylistSummary): string | null {
    if (playlist.active_count > 0) {
      return t('downloads.playlist_activity_active', { count: playlist.active_count });
    }
    if (playlist.status === 'running' && playlist.cooldown_until) {
      return t('downloads.playlist_activity_cooldown', {
        time: parseSqliteUtc(playlist.cooldown_until).toLocaleString()
      });
    }
    if (playlist.status === 'running' && playlist.pending_count > 0) {
      return t('downloads.playlist_activity_preparing');
    }
    return null;
  }

  function parseSqliteUtc(value: string): Date {
    return new Date(`${value.replace(' ', 'T')}Z`);
  }

  function displayDownloadError(message: string | null): string {
    if (!message) return t('downloads.unknown_error');
    const clean = message.replace(/\u001b\[[0-?]*[ -/]*[@-~]/g, '').trim();
    const normalized = clean.toLowerCase();
    if (
      normalized === 'download_stalled' ||
      normalized === 'download encerrado pelo watchdog por falta de progresso'
    ) {
      return t('downloads.download_stalled');
    }
    if (
      normalized.includes('addondisposecallback') ||
      normalized.includes('challenge solver failed') ||
      normalized.includes('n challenge solving failed')
    ) {
      return t('downloads.javascript_solver_failed');
    }
    const relevant = [...clean.split('\n')].reverse().find((line) => line.trim().startsWith('ERROR:')) ?? clean;
    return relevant.length > 500 ? `${relevant.slice(0, 497)}…` : relevant;
  }

  function isExistingDownloadResult(message: string | null): boolean {
    if (!message) return false;
    const normalized = message.toLowerCase();
    return (
      normalized === 'already_exists' ||
      normalized.includes('destino já existe') ||
      normalized.includes('já existe na biblioteca')
    );
  }

  async function runPlaylistAction(playlist: DownloadPlaylistSummary, action: 'start' | 'pause' | 'cancel' | 'retry-failed' | 'skip-cooldown') {
    playlistActionBusy = playlist.id;
    probeError = null;
    try {
      const response = await apiFetch(`/api/download-playlists/${playlist.id}/${action}`, { method: 'POST' });
      if (!response.ok) throw new Error(t('downloads.playlist_action_failed'));
      const updated = (await response.json()) as DownloadPlaylistSummary;
      downloadPlaylists = downloadPlaylists.map((item) => (item.id === updated.id ? updated : item));
      if (selectedPlaylistDetail?.id === updated.id) await refreshSelectedPlaylistDetail();
    } catch (error) {
      probeError = error instanceof Error ? error.message : String(error);
    } finally {
      playlistActionBusy = null;
    }
  }

  async function refreshSelectedPlaylistDetail() {
    if (!selectedPlaylistDetail) return;
    const id = selectedPlaylistDetail.id;
    const limit = Math.max(100, selectedPlaylistDetail.items.length);
    try {
      const response = await apiFetch(`/api/download-playlists/${id}?limit=${limit}&offset=0`);
      if (response.ok && selectedPlaylistDetail?.id === id) {
        selectedPlaylistDetail = (await response.json()) as DownloadPlaylistDetail;
      }
    } catch {
      // Polling keeps the last usable state visible.
    }
  }

  async function runPlaylistItemAction(item: DownloadPlaylistItem, action: 'retry' | 'cancel') {
    if (!selectedPlaylistDetail) return;
    const playlistId = selectedPlaylistDetail.id;
    playlistItemActionBusy = `${item.position}:${action}`;
    probeError = null;
    try {
      const response = await apiFetch(
        `/api/download-playlists/${playlistId}/items/${item.position}/${action}`,
        { method: 'POST' }
      );
      if (!response.ok) throw new Error(t('downloads.playlist_item_action_failed'));
      const summary = (await response.json()) as DownloadPlaylistSummary;
      downloadPlaylists = downloadPlaylists.map((playlist) => (playlist.id === summary.id ? summary : playlist));
      await refreshSelectedPlaylistDetail();
    } catch (error) {
      probeError = error instanceof Error ? error.message : String(error);
    } finally {
      playlistItemActionBusy = null;
    }
  }

  async function openPlaylistDetail(playlist: DownloadPlaylistSummary) {
    loadingPlaylistDetail = true;
    probeError = null;
    try {
      const response = await apiFetch(`/api/download-playlists/${playlist.id}?limit=100&offset=0`);
      if (!response.ok) throw new Error(t('downloads.playlist_detail_failed'));
      selectedPlaylistDetail = (await response.json()) as DownloadPlaylistDetail;
    } catch (error) {
      probeError = error instanceof Error ? error.message : String(error);
    } finally {
      loadingPlaylistDetail = false;
    }
  }

  async function loadMorePlaylistItems() {
    if (!selectedPlaylistDetail) return;
    loadingPlaylistDetail = true;
    try {
      const offset = selectedPlaylistDetail.items.length;
      const response = await apiFetch(`/api/download-playlists/${selectedPlaylistDetail.id}?limit=100&offset=${offset}`);
      if (!response.ok) throw new Error(t('downloads.playlist_detail_failed'));
      const next = (await response.json()) as DownloadPlaylistDetail;
      selectedPlaylistDetail = { ...next, items: [...selectedPlaylistDetail.items, ...next.items] };
    } catch (error) {
      probeError = error instanceof Error ? error.message : String(error);
    } finally {
      loadingPlaylistDetail = false;
    }
  }

  async function updatePlaylistSelection(selected: boolean, positions?: number[]) {
    if (!selectedPlaylistDetail) return;
    updatingPlaylistSelection = true;
    try {
      const response = await apiFetch(`/api/download-playlists/${selectedPlaylistDetail.id}/items/selection`, {
        method: 'PUT',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ selected, positions })
      });
      if (!response.ok) throw new Error(t('downloads.playlist_selection_failed'));
      const summary = (await response.json()) as DownloadPlaylistSummary;
      downloadPlaylists = downloadPlaylists.map((item) => (item.id === summary.id ? summary : item));
      selectedPlaylistDetail = {
        ...selectedPlaylistDetail,
        ...summary,
        items: selectedPlaylistDetail.items.map((item) =>
          (!positions || positions.includes(item.position)) && item.status === 'pending' ? { ...item, selected } : item
        )
      };
    } catch (error) {
      probeError = error instanceof Error ? error.message : String(error);
    } finally {
      updatingPlaylistSelection = false;
    }
  }

  async function startDownload() {
    const raw = urlInput.trim();
    if (!raw) return;
    startingDownload = true;
    probeError = null;
    try {
      const res = await apiFetch('/api/downloads', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          url: raw,
          quality: selectedQuality,
          container: selectedContainer
        })
      });
      if (!res.ok) {
        const errText = await res.text();
        throw new Error(errText || 'Falha ao iniciar download');
      }
      // Limpa formulário de entrada para novos downloads
      urlInput = '';
      probedVideo = null;
      await loadJobs();
    } catch (e) {
      probeError = e instanceof Error ? e.message : String(e);
    } finally {
      startingDownload = false;
    }
  }

  async function cancelOrRemoveJob(jobId: string) {
    try {
      await apiFetch(`/api/downloads/${jobId}`, { method: 'DELETE' });
      await loadJobs();
    } catch {
      // Ignora erro
    }
  }

  async function playDownloadedMedia(mediaId: number) {
    try {
      const res = await apiFetch(`/api/media/${mediaId}`);
      if (res.ok) {
        const item = (await res.json()) as MediaItem;
        void player.playMedia(item, true);
        closePopover();
      }
    } catch {
      // Ignora erro
    }
  }

  function formatDuration(ms: number | null): string {
    if (!ms || ms <= 0) return '';
    const totalSecs = Math.floor(ms / 1000);
    const m = Math.floor(totalSecs / 60);
    const s = totalSecs % 60;
    return `${m}:${s < 10 ? '0' : ''}${s}`;
  }

  function qualityLabel(q: string): string {
    switch (q) {
      case 'best':
        return 'Melhor qualidade (Automático)';
      case '1440p':
        return '1440p (2K)';
      case '1080p':
        return '1080p (Full HD)';
      case '720p':
        return '720p (HD)';
      case '480p':
        return '480p';
      case '360p':
        return '360p';
      default:
        return q;
    }
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} />

<div class="downloads-popover-anchor">
  <!-- Botão Disparador no Topbar -->
  <button
    type="button"
    class="downloads-toggle-btn"
    class:active={isOpen}
    class:has-active={totalActiveCount > 0}
    bind:this={triggerButton}
    onclick={togglePopover}
    title={totalActiveCount > 0 ? t('downloads.active_count_tip', { count: totalActiveCount }) : t('downloads.title')}
    aria-label={t('downloads.open_manager')}
    aria-expanded={isOpen}
    aria-haspopup="dialog"
    aria-controls="downloads-manager"
  >
    <span class="material-symbols-outlined icon-20">
      {totalActiveCount > 0 ? 'downloading' : 'download'}
    </span>
    {#if totalActiveCount > 0}
      <span class="downloads-badge">{totalActiveCount}</span>
    {/if}
  </button>

  <!-- Popover / Mini Janela Flutuante -->
  {#if isOpen}
    <button
      type="button"
      class="popover-backdrop"
      onclick={() => closePopover()}
      aria-label={t('downloads.close_window')}
    ></button>

    <div bind:this={dialogElement} id="downloads-manager" class="downloads-dropdown-card" role="dialog" aria-modal="true" aria-label={t('downloads.manager')} aria-busy={probing || startingDownload}>
      <div class="popover-header">
        <div class="popover-title-row">
          <span class="material-symbols-outlined popover-icon">download</span>
          <strong>{t('downloads.download_from_yt')}</strong>
        </div>
        <button bind:this={closeButton} type="button" class="popover-close-btn" onclick={() => closePopover()} title={t('common.close')}>
          <span class="material-symbols-outlined icon-18">close</span>
        </button>
      </div>

      <div class="manager-tabs" role="tablist" aria-label={t('downloads.manager_sections')}>
        <button
          type="button"
          role="tab"
          aria-selected={activeTab === 'videos'}
          class:active={activeTab === 'videos'}
          onclick={() => selectTab('videos')}
        >
          <span class="material-symbols-outlined icon-18">movie</span>
          {t('downloads.videos_tab')}
          {#if activeCount > 0}<span class="tab-count">{activeCount}</span>{/if}
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={activeTab === 'playlists'}
          class:active={activeTab === 'playlists'}
          class:attention={attentionPlaylistCount > 0}
          onclick={() => selectTab('playlists')}
        >
          <span class="material-symbols-outlined icon-18">playlist_play</span>
          {t('downloads.playlists_tab')}
          {#if activePlaylistCount + attentionPlaylistCount > 0}
            <span class="tab-count">{activePlaylistCount + attentionPlaylistCount}</span>
          {/if}
        </button>
      </div>

      <!-- Formulário para colar link -->
      <div class="download-form-section">
        <form
          onsubmit={(e) => {
            e.preventDefault();
            void handleProbe();
          }}
          class="url-input-form"
        >
          <div class="url-input-wrap">
            <span class="material-symbols-outlined link-icon">link</span>
            <input
              type="url"
              bind:value={urlInput}
              placeholder={t('downloads.paste_placeholder')}
              autocomplete="off"
              spellcheck="false"
              disabled={probing || startingDownload}
            />
            <button
              type="button"
              class="paste-btn"
              onclick={() => void handlePaste()}
              title={t('downloads.paste_btn_title')}
            >
              <span class="material-symbols-outlined icon-16">content_paste</span>
            </button>
          </div>

          <button
            type="submit"
            class="probe-btn"
            disabled={!urlInput.trim() || probing || startingDownload}
          >
            {#if probing}
              <span class="spinner-small"></span>
            {:else}
              <span class="material-symbols-outlined icon-16 icon-gap-after">search</span>
              {t('downloads.probe_btn')}
            {/if}
          </button>
        </form>

        {#if activeTab === 'playlists'}
          <label class="create-playlist-option">
            <input type="checkbox" bind:checked={createLocalPlaylist} />
            <span>
              <strong>{t('downloads.create_library_playlist')}</strong>
              <small>{t('downloads.create_library_playlist_desc')}</small>
            </span>
          </label>
        {/if}

        {#if probeError}
          <Notice tone="error">{probeError}</Notice>
        {/if}

        <!-- Card de Prévia do Vídeo Consultado -->
        {#if activeTab === 'videos' && probedVideo}
          <div class="probe-preview-card">
            {#if probedVideo.existing_media}
              <div class="existing-media-alert">
                <span class="material-symbols-outlined icon-18 icon-warning no-shrink">info</span>
                <div class="existing-media-text">
                  <strong>{t('downloads.video_exists_in_lib', { res: probedVideo.existing_media.resolution })}</strong>
                  <span>{t('downloads.video_exists_desc')}</span>
                </div>
              </div>
            {/if}

            <div class="probe-video-row">
              <div class="probe-thumb-wrap">
                {#if probedVideo.thumbnail_url}
                  <img
                    src={probedVideo.thumbnail_url}
                    alt={probedVideo.title}
                    class="probe-thumb"
                    loading="eager"
                    decoding="sync"
                    fetchpriority="high"
                  />
                {/if}
                {#if probedVideo.duration_ms}
                  <span class="probe-duration-badge">{formatDuration(probedVideo.duration_ms)}</span>
                {/if}
              </div>

              <div class="probe-details">
                <strong class="probe-title" title={probedVideo.title}>{probedVideo.title}</strong>
                <div class="probe-meta">
                  <span class="probe-channel">{probedVideo.channel_name}</span>
                  {#if probedVideo.is_short}
                    <span class="short-pill">Short</span>
                  {/if}
                </div>
              </div>
            </div>

            <!-- Opções de Qualidade e Formato -->
            <div class="probe-controls-box">
              <div class="probe-field">
                <label for="quality-select">{t('downloads.resolution')}</label>
                <select id="quality-select" bind:value={selectedQuality} class="options-select">
                  {#each probedVideo.available_qualities as q}
                    <option value={q}>{qualityLabel(q)}</option>
                  {/each}
                </select>
              </div>

              <div class="probe-field format-field">
                <span class="field-label">{t('downloads.format')}</span>
                <div class="container-toggle-row">
                  <button
                    type="button"
                    class="container-btn"
                    class:active={selectedContainer === 'mkv'}
                    onclick={() => (selectedContainer = 'mkv')}
                  >
                    MKV
                  </button>
                  <button
                    type="button"
                    class="container-btn"
                    class:active={selectedContainer === 'mp4'}
                    onclick={() => (selectedContainer = 'mp4')}
                  >
                    MP4
                  </button>
                </div>
              </div>
            </div>

            <button
              type="button"
              class="start-download-btn"
              onclick={() => void startDownload()}
              disabled={startingDownload}
            >
              {#if startingDownload}
                <span class="spinner-small"></span> {t('downloads.starting_download')}
              {:else}
                <span class="material-symbols-outlined icon-19 icon-gap-after-md">download</span>
                {t('downloads.start_btn')}
              {/if}
            </button>
          </div>
        {/if}
      </div>

      <div class="popover-divider"></div>

      <!-- Lista dos Últimos Downloads -->
      {#if activeTab === 'videos'}
        <div class="downloads-list-section" aria-live="polite">
        <div class="downloads-list-header">
          <span>{t('downloads.recent_downloads', { count: jobs.length })}</span>
          {#if activeCount > 0}
            <span class="active-pulse-badge">
              <span class="spinner-tiny"></span> {t('downloads.active_count', { count: activeCount })}
            </span>
          {/if}
        </div>

        {#if jobs.length === 0}
          <div class="downloads-empty">
            <span class="material-symbols-outlined icon-28 icon-muted">cloud_download</span>
            <p>{t('downloads.no_downloads')}</p>
          </div>
        {:else}
          <div class="jobs-list">
            {#each jobs as job (job.id)}
              <div
                class="job-card"
                class:status-completed={job.status === 'completed' || (job.status === 'failed' && isExistingDownloadResult(job.error_message))}
                class:status-failed={job.status === 'failed' && !isExistingDownloadResult(job.error_message)}
              >
                {#if job.thumbnail_url}
                  <img src={job.thumbnail_url} alt="" class="job-thumb" />
                {:else}
                  <div class="job-thumb-placeholder">
                    <span class="material-symbols-outlined icon-18">movie</span>
                  </div>
                {/if}

                <div class="job-info">
                  <strong class="job-title" title={job.title ?? job.url}>
                    {job.title ?? job.url}
                  </strong>
                  <div class="job-meta-row">
                    <span>{job.channel_name ?? 'YouTube'}</span>
                    <span class="bullet">·</span>
                    <span class="format-badge">{job.format_container.toUpperCase()}</span>
                    <span class="bullet">·</span>
                    <span>{job.quality}</span>
                  </div>

                  <!-- Status e Progresso -->
                  <div class="job-status-row">
                    {#if job.status === 'downloading'}
                      <div class="job-progress-bar-wrap">
                        <div class="job-progress-fill" class:active-progress={job.status === 'downloading'} style="width: {Math.max(job.progress_percent, 2)}%"></div>
                      </div>
                      <div class="job-progress-metrics">
                        <span>{job.progress_percent.toFixed(1)}%</span>
                        {#if job.speed_text}<span>· {job.speed_text}</span>{/if}
                        {#if job.eta_text}<span>· ETA {job.eta_text}</span>{/if}
                      </div>
                    {:else if job.status === 'organizing'}
                      <div class="job-status-badge organizing">
                        <span class="spinner-tiny"></span>
                        <span>{t('downloads.status_organizing')}</span>
                      </div>
                    {:else if job.status === 'awaiting_index'}
                      <div class="job-status-badge organizing">
                        <span class="material-symbols-outlined spin icon-14">sync</span>
                        <span>{t('downloads.status_indexing')}</span>
                      </div>
                    {:else if job.status === 'queued' || job.status === 'probing'}
                      <div class="job-status-badge queued">
                        <span class="spinner-tiny"></span>
                        <span>{t('downloads.status_queued')}</span>
                      </div>
                    {:else if job.status === 'completed'}
                      <div class="job-status-badge completed">
                        <span class="material-symbols-outlined icon-14">check_circle</span>
                        <span>{t('downloads.completed')}</span>
                      </div>
                    {:else if job.status === 'failed' && isExistingDownloadResult(job.error_message)}
                      <div class="job-status-badge completed">
                        <span class="material-symbols-outlined icon-14">library_add_check</span>
                        <span>{t('downloads.item_status_skipped_existing')}</span>
                      </div>
                    {:else if job.status === 'failed'}
                      <div class="job-status-badge failed" title={job.error_message ?? t('downloads.unknown_error')}>
                        <span class="material-symbols-outlined icon-14">cancel</span>
                        <span>{t('downloads.failed')}</span>
                      </div>
                      <p class="download-error-message">{displayDownloadError(job.error_message)}</p>
                    {:else if job.status === 'cancelled'}
                      <div class="job-status-badge cancelled">
                        <span>{t('downloads.cancelled')}</span>
                      </div>
                    {/if}
                  </div>
                </div>

                <!-- Ações do Job -->
                <div class="job-actions">
                  {#if job.status === 'completed'}
                    {#if job.media_id}
                      <button
                        type="button"
                        class="job-action-btn play"
                        onclick={() => void playDownloadedMedia(job.media_id!)}
                        title={t('downloads.watch_in_player')}
                      >
                        <span class="material-symbols-outlined icon-16">play_arrow</span>
                        {t('downloads.watch_now')}
                      </button>
                    {:else if job.output_path}
                      <a
                        href={`/api/downloads/${job.id}/file`}
                        download
                        class="job-action-btn download"
                        title={t('downloads.save_file_tip')}
                      >
                        <span class="material-symbols-outlined icon-15">save_alt</span>
                        {t('downloads.save_file')}
                      </a>
                    {/if}
                  {:else if job.status === 'downloading' || job.status === 'queued'}
                    <button
                      type="button"
                      class="job-action-btn cancel"
                      onclick={() => void cancelOrRemoveJob(job.id)}
                      title={t('downloads.cancel_download')}
                    >
                      <span class="material-symbols-outlined icon-16">close</span>
                    </button>
                  {:else}
                    <button
                      type="button"
                      class="job-action-btn remove"
                      onclick={() => void cancelOrRemoveJob(job.id)}
                      title={t('downloads.remove_from_list')}
                    >
                      <span class="material-symbols-outlined icon-16">delete</span>
                    </button>
                  {/if}
                </div>
              </div>
            {/each}
          </div>
        {/if}
        </div>
      {:else}
        <div class="downloads-list-section playlist-list-section" aria-live="polite">
          {#if selectedPlaylistDetail}
            <div class="playlist-detail-header">
              <button type="button" class="detail-back" onclick={() => (selectedPlaylistDetail = null)}>
                <span class="material-symbols-outlined icon-18">arrow_back</span>
                {t('downloads.all_playlists')}
              </button>
              <div>
                <strong>{selectedPlaylistDetail.title}</strong>
                <span>{playlistStatusLabel(selectedPlaylistDetail.status)}</span>
              </div>
            </div>

            <div class="playlist-detail-overview">
              <div class="playlist-detail-summary">
                <span>{t('downloads.playlist_selected_count', { selected: selectedPlaylistDetail.selected_count, total: selectedPlaylistDetail.item_count })}</span>
                <span>{t('downloads.playlist_completed_count', { count: selectedPlaylistDetail.completed_count })}</span>
                {#if selectedPlaylistDetail.skipped_existing_count > 0}
                  <span class="existing-summary">{t('downloads.playlist_existing_count', { count: selectedPlaylistDetail.skipped_existing_count })}</span>
                {/if}
              </div>

              <div class="playlist-detail-controls">
                {#if selectedPlaylistDetail.failed_count + selectedPlaylistDetail.cancelled_count > 0}
                  <button
                    type="button"
                    class="primary"
                    disabled={playlistActionBusy === selectedPlaylistDetail.id || selectedPlaylistDetail.status === 'paused_rate_limit'}
                    onclick={() => void runPlaylistAction(selectedPlaylistDetail!, 'retry-failed')}
                  >
                    <span class="material-symbols-outlined icon-16">refresh</span>
                    {t('downloads.retry_incomplete_items', { count: selectedPlaylistDetail.failed_count + selectedPlaylistDetail.cancelled_count })}
                  </button>
                {/if}
                {#if selectedPlaylistDetail.status === 'running' && selectedPlaylistDetail.cooldown_until && !selectedPlaylistDetail.protection_reason && parseSqliteUtc(selectedPlaylistDetail.cooldown_until) > new Date()}
                  <button type="button" class="primary" disabled={playlistActionBusy === selectedPlaylistDetail.id} onclick={() => void runPlaylistAction(selectedPlaylistDetail!, 'skip-cooldown')}>
                    <span class="material-symbols-outlined icon-16">skip_next</span>{t('downloads.skip_cooldown')}
                  </button>
                {/if}
                {#if ['ready', 'paused'].includes(selectedPlaylistDetail.status)}
                  <button type="button" class="primary" disabled={playlistActionBusy === selectedPlaylistDetail.id} onclick={() => void runPlaylistAction(selectedPlaylistDetail!, 'start')}>
                    <span class="material-symbols-outlined icon-16">play_arrow</span>{t('downloads.start_playlist')}
                  </button>
                {:else if selectedPlaylistDetail.status === 'running'}
                  <button type="button" disabled={playlistActionBusy === selectedPlaylistDetail.id} onclick={() => void runPlaylistAction(selectedPlaylistDetail!, 'pause')}>
                    <span class="material-symbols-outlined icon-16">pause</span>{t('downloads.pause_playlist')}
                  </button>
                {/if}
                {#if ['ready', 'running', 'paused', 'paused_rate_limit'].includes(selectedPlaylistDetail.status)}
                  <button type="button" class="danger" disabled={playlistActionBusy === selectedPlaylistDetail.id} onclick={() => void runPlaylistAction(selectedPlaylistDetail!, 'cancel')}>
                    <span class="material-symbols-outlined icon-16">stop_circle</span>{t('downloads.stop_playlist')}
                  </button>
                {/if}
              </div>
            </div>

            {#if playlistActivityText(selectedPlaylistDetail)}
              <div class="playlist-activity-banner" class:cooldown={selectedPlaylistDetail.active_count === 0 && !!selectedPlaylistDetail.cooldown_until}>
                <span class="material-symbols-outlined icon-16">{selectedPlaylistDetail.active_count > 0 ? 'downloading' : 'schedule'}</span>
                <strong>{playlistActivityText(selectedPlaylistDetail)}</strong>
              </div>
            {/if}

            {#if selectedPlaylistDetail.status === 'ready' || selectedPlaylistDetail.status === 'paused'}
              <div class="playlist-selection-actions">
                <button type="button" disabled={updatingPlaylistSelection} onclick={() => void updatePlaylistSelection(true)}>
                  {t('downloads.select_all')}
                </button>
                <button type="button" disabled={updatingPlaylistSelection} onclick={() => void updatePlaylistSelection(false)}>
                  {t('downloads.select_none')}
                </button>
              </div>
            {/if}

            <div class="playlist-item-list">
              {#each selectedPlaylistDetail.items as item (item.position)}
                <div class="playlist-item-row" class:unavailable={item.status === 'unavailable'}>
                  <input
                    type="checkbox"
                    checked={item.selected}
                    disabled={updatingPlaylistSelection || item.status !== 'pending' || !['ready', 'paused'].includes(selectedPlaylistDetail.status)}
                    onchange={(event) => void updatePlaylistSelection(event.currentTarget.checked, [item.position])}
                    aria-label={item.title}
                  />
                  <span class="playlist-item-position">{item.position + 1}</span>
                  <div class="playlist-item-thumb">
                    {#if item.thumbnail_url}<img src={item.thumbnail_url} alt="" />{:else}<span class="material-symbols-outlined icon-18">movie</span>{/if}
                  </div>
                  <div class="playlist-item-copy">
                    <strong title={item.title}>{item.title}</strong>
                    <span>
                      {item.channel_name ?? selectedPlaylistDetail.channel_name ?? 'YouTube'}
                      {#if item.duration_ms} · {formatDuration(item.duration_ms)}{/if}
                    </span>
                    {#if item.status === 'failed'}
                      <span class="playlist-item-error">{displayDownloadError(item.error_message)}</span>
                    {/if}
                    {#if item.status === 'downloading'}
                      <div class="playlist-item-progress">
                        <div class="job-progress-bar-wrap"><div class="job-progress-fill active-progress" style={`width: ${Math.max(item.progress_percent, 2)}%`}></div></div>
                        <span>{item.progress_percent.toFixed(1)}%{item.speed_text ? ` · ${item.speed_text}` : ''}{item.eta_text ? ` · ETA ${item.eta_text}` : ''}</span>
                      </div>
                    {/if}
                  </div>
                  <span class={`playlist-item-status status-${item.status}`}>{t(`downloads.item_status_${item.status}`)}</span>
                  <div class="playlist-item-actions">
                    {#if ['failed', 'cancelled'].includes(item.status)}
                      <button type="button" disabled={playlistItemActionBusy !== null || selectedPlaylistDetail.status === 'paused_rate_limit'} onclick={() => void runPlaylistItemAction(item, 'retry')} title={t('downloads.retry_item')}>
                        <span class="material-symbols-outlined icon-16">refresh</span>
                      </button>
                    {:else if ['pending', 'queued', 'downloading'].includes(item.status)}
                      <button type="button" class="danger" disabled={playlistItemActionBusy !== null} onclick={() => void runPlaylistItemAction(item, 'cancel')} title={t('downloads.cancel_item')}>
                        <span class="material-symbols-outlined icon-16">close</span>
                      </button>
                    {/if}
                  </div>
                </div>
              {/each}
            </div>

            {#if selectedPlaylistDetail.items.length < selectedPlaylistDetail.item_count}
              <button type="button" class="load-more-playlist" disabled={loadingPlaylistDetail} onclick={() => void loadMorePlaylistItems()}>
                {loadingPlaylistDetail ? t('common.loading') : t('downloads.load_more_items')}
              </button>
            {/if}
          {:else}
          <div class="downloads-list-header">
            <span>{t('downloads.playlist_groups', { count: downloadPlaylists.length })}</span>
            <span class="conservative-badge">
              <span class="material-symbols-outlined icon-14">shield</span>
              {t('downloads.conservative_mode')}
            </span>
          </div>

          {#if downloadPlaylists.length === 0}
            <div class="downloads-empty playlist-empty">
              <span class="material-symbols-outlined icon-28 icon-muted">playlist_add</span>
              <p>{t('downloads.no_download_playlists')}</p>
              <small>{t('downloads.no_download_playlists_desc')}</small>
            </div>
          {:else}
            <div class="playlist-cards">
              {#each downloadPlaylists as playlist (playlist.id)}
                <article class="playlist-download-card" class:needs-attention={playlist.status === 'paused_rate_limit'}>
                  <div class="playlist-cover">
                    {#if playlist.thumbnail_url}
                      <img src={playlist.thumbnail_url} alt="" />
                    {:else}
                      <span class="material-symbols-outlined">playlist_play</span>
                    {/if}
                  </div>
                  <div class="playlist-card-content">
                    <div class="playlist-card-heading">
                      <strong title={playlist.title}>{playlist.title}</strong>
                      <span class:warning={playlist.status === 'paused_rate_limit'}>{playlistStatusLabel(playlist.status)}</span>
                    </div>
                    <div class="playlist-card-meta">
                      <span>{playlist.channel_name ?? 'YouTube'}</span>
                      <span>·</span>
                      <span>{t('downloads.playlist_item_count', { count: playlist.item_count })}</span>
                      {#if playlist.unavailable_count > 0}
                        <span>·</span>
                        <span>{t('downloads.playlist_unavailable_count', { count: playlist.unavailable_count })}</span>
                      {/if}
                    </div>
                    <div class="playlist-progress-row">
                      <div class="job-progress-bar-wrap">
                        <div
                          class="job-progress-fill"
                          class:active-progress={playlist.status === 'running'}
                          style={`width: ${playlist.selected_count > 0 ? (playlistDoneCount(playlist) / playlist.selected_count) * 100 : 0}%`}
                        ></div>
                      </div>
                      <span>{playlistDoneCount(playlist)}/{playlist.selected_count}</span>
                    </div>
                    {#if playlistActivityText(playlist)}
                      <div class="playlist-card-activity">
                        <span class="spinner-tiny" class:hidden={playlist.active_count === 0}></span>
                        <span>{playlistActivityText(playlist)}</span>
                      </div>
                    {/if}
                    <div class="playlist-card-footer">
                      <span class="format-badge">{playlist.format_container.toUpperCase()}</span>
                      <span>{playlist.quality}</span>
                      {#if playlist.create_local_playlist}
                        <span class="library-playlist-mark">
                          <span class="material-symbols-outlined icon-14">library_add</span>
                          {t('downloads.library_playlist_enabled')}
                        </span>
                      {/if}
                    </div>
                  </div>
                  <div class="playlist-card-actions">
                    <button
                      type="button"
                      class="playlist-action"
                      disabled={loadingPlaylistDetail}
                      onclick={() => void openPlaylistDetail(playlist)}
                      title={t('downloads.view_playlist_details')}
                    >
                      <span class="material-symbols-outlined icon-18">list</span>
                    </button>
                    {#if playlist.status === 'ready' || playlist.status === 'paused' || (playlist.status === 'paused_rate_limit' && playlist.cooldown_until && parseSqliteUtc(playlist.cooldown_until) <= new Date())}
                      <button
                        type="button"
                        class="playlist-action primary"
                        disabled={playlistActionBusy === playlist.id}
                        onclick={() => void runPlaylistAction(playlist, 'start')}
                        title={t('downloads.start_playlist')}
                      >
                        <span class="material-symbols-outlined icon-18">play_arrow</span>
                      </button>
                    {:else if playlist.status === 'running'}
                      {#if playlist.cooldown_until && !playlist.protection_reason && parseSqliteUtc(playlist.cooldown_until) > new Date()}
                        <button
                          type="button"
                          class="playlist-action primary"
                          disabled={playlistActionBusy === playlist.id}
                          onclick={() => void runPlaylistAction(playlist, 'skip-cooldown')}
                          title={t('downloads.skip_cooldown')}
                        >
                          <span class="material-symbols-outlined icon-18">skip_next</span>
                        </button>
                      {/if}
                      <button
                        type="button"
                        class="playlist-action"
                        disabled={playlistActionBusy === playlist.id}
                        onclick={() => void runPlaylistAction(playlist, 'pause')}
                        title={t('downloads.pause_playlist')}
                      >
                        <span class="material-symbols-outlined icon-18">pause</span>
                      </button>
                    {/if}
                    {#if playlist.status === 'cancelled' || playlist.status === 'completed_with_errors'}
                      <button
                        type="button"
                        class="playlist-action primary"
                        disabled={playlistActionBusy === playlist.id}
                        onclick={() => void runPlaylistAction(playlist, 'retry-failed')}
                        title={t('downloads.retry_incomplete_items', { count: playlist.failed_count + playlist.cancelled_count })}
                      >
                        <span class="material-symbols-outlined icon-18">refresh</span>
                      </button>
                    {/if}
                    {#if ['ready', 'running', 'paused', 'paused_rate_limit'].includes(playlist.status)}
                      <button
                        type="button"
                        class="playlist-action danger"
                        disabled={playlistActionBusy === playlist.id}
                        onclick={() => void runPlaylistAction(playlist, 'cancel')}
                        title={t('downloads.cancel_playlist_pending')}
                      >
                        <span class="material-symbols-outlined icon-18">stop_circle</span>
                      </button>
                    {/if}
                  </div>
                </article>
              {/each}
            </div>
          {/if}
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .downloads-popover-anchor {
    position: relative;
    display: flex;
    align-items: center;
  }

  .downloads-toggle-btn {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 38px;
    height: 38px;
    border-radius: var(--radius-full);
    background: transparent;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    transition: background var(--motion-fast) var(--ease-standard), color var(--motion-fast) var(--ease-standard);
  }

  .downloads-toggle-btn:hover,
  .downloads-toggle-btn.active {
    background: var(--overlay);
    color: var(--text-primary);
  }

  .downloads-toggle-btn.has-active {
    color: var(--accent);
  }

  .downloads-badge {
    position: absolute;
    top: 2px;
    right: 2px;
    background: var(--accent);
    color: var(--text-inverse);
    font-size: 10px;
    font-weight: 700;
    min-width: 16px;
    height: 16px;
    border-radius: 8px;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0 4px;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.5);
  }

  .popover-backdrop {
    position: fixed;
    inset: 0;
    background: transparent;
    border: none;
    z-index: 998;
    cursor: default;
  }

  .downloads-dropdown-card {
    position: absolute;
    top: 48px;
    right: 0;
    width: 760px;
    max-width: 95vw;
    max-height: 85vh;
    background: var(--surface-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-overlay);
    z-index: 999;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    animation: fadeInSlide var(--motion-fast) var(--ease-standard);
  }

  @keyframes fadeInSlide {
    from {
      opacity: 0;
      transform: translateY(-8px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .popover-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 16px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .manager-tabs {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 4px;
    padding: 8px 16px 0;
    background: var(--surface-2);
  }

  .manager-tabs button {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    min-height: 40px;
    border: 0;
    border-bottom: 2px solid transparent;
    background: transparent;
    color: var(--text-secondary);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }

  .manager-tabs button:hover {
    color: var(--text-primary);
    background: var(--overlay);
  }

  .manager-tabs button.active {
    color: var(--accent);
    border-bottom-color: var(--accent);
  }

  .manager-tabs button.attention:not(.active) {
    color: var(--warning, #fbbf24);
  }

  .tab-count {
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    border-radius: 9px;
    background: var(--overlay-strong, rgba(255, 255, 255, 0.13));
    color: currentColor;
    font-size: 10px;
    line-height: 18px;
    text-align: center;
  }

  .popover-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 15px;
    color: var(--text-primary);
  }

  .popover-icon {
    font-size: 20px;
    color: var(--accent);
  }

  .popover-close-btn {
    background: transparent;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    border-radius: var(--radius-full);
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .popover-close-btn:hover {
    color: var(--text-primary);
    background: var(--overlay);
  }

  .download-form-section {
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .create-playlist-option {
    display: flex;
    align-items: flex-start;
    gap: 9px;
    padding: 9px 11px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--surface-1);
    color: var(--text-primary);
    cursor: pointer;
  }

  .create-playlist-option input {
    margin-top: 2px;
    accent-color: var(--accent);
  }

  .create-playlist-option span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .create-playlist-option strong {
    font-size: 12px;
  }

  .create-playlist-option small {
    color: var(--text-muted);
    font-size: 11px;
  }

  .url-input-form {
    display: flex;
    gap: 8px;
  }

  .url-input-wrap {
    flex: 1;
    display: flex;
    align-items: center;
    background: var(--surface-1);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    padding: 0 var(--space-2);
    transition: border-color var(--motion-fast) var(--ease-standard);
  }

  .url-input-wrap:focus-within {
    border-color: var(--border-focus);
  }

  .link-icon {
    font-size: 18px;
    color: var(--text-muted);
    margin-right: 6px;
  }

  .url-input-wrap input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-size: 13px;
    padding: 8px 0;
  }

  .paste-btn {
    background: transparent;
    border: none;
    color: rgba(255, 255, 255, 0.5);
    cursor: pointer;
    padding: 4px;
    border-radius: 4px;
    display: flex;
    align-items: center;
  }

  .paste-btn:hover {
    color: #fff;
    background: rgba(255, 255, 255, 0.1);
  }

  .probe-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0 14px;
    background: rgba(255, 255, 255, 0.1);
    color: #fff;
    border: 1px solid rgba(255, 255, 255, 0.14);
    border-radius: 8px;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.15s;
  }

  .probe-btn:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.2);
  }

  .probe-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .existing-media-alert {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    background: rgba(245, 158, 11, 0.12);
    border: 1px solid rgba(245, 158, 11, 0.3);
    color: #fde68a;
    padding: 8px 10px;
    border-radius: 8px;
    font-size: 11.5px;
    line-height: 1.4;
  }
  .existing-media-alert strong {
    display: block;
    color: #fbbf24;
    font-weight: 600;
    margin-bottom: 2px;
  }

  .probe-preview-card {
    display: flex;
    flex-direction: column;
    gap: 12px;
    background: #202025;
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 12px;
    padding: 12px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
  }

  .probe-video-row {
    display: flex;
    gap: 12px;
    align-items: flex-start;
  }

  .probe-thumb-wrap {
    position: relative;
    width: 130px;
    height: 74px;
    border-radius: 8px;
    overflow: hidden;
    background: #000;
    flex-shrink: 0;
  }

  .probe-thumb {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .probe-duration-badge {
    position: absolute;
    bottom: 4px;
    right: 4px;
    background: rgba(0, 0, 0, 0.85);
    color: #fff;
    font-size: 10px;
    font-weight: 600;
    padding: 1px 5px;
    border-radius: 4px;
    letter-spacing: 0.3px;
  }

  .probe-details {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .probe-title {
    font-size: 13px;
    font-weight: 600;
    color: #fff;
    line-height: 1.35;
    display: -webkit-box;
    line-clamp: 2;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .probe-meta {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: rgba(255, 255, 255, 0.6);
  }

  .probe-channel {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .short-pill {
    background: rgba(239, 68, 68, 0.25);
    color: #f87171;
    padding: 1px 6px;
    border-radius: 4px;
    font-weight: 700;
    font-size: 10px;
  }

  .probe-controls-box {
    display: flex;
    gap: 12px;
    background: #17171b;
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 8px;
    padding: 10px 12px;
    align-items: center;
    justify-content: space-between;
  }

  .probe-field {
    display: flex;
    flex-direction: column;
    gap: 5px;
    flex: 1;
  }

  .probe-field.format-field {
    flex: 0 0 auto;
  }

  .probe-field label,
  .probe-field .field-label {
    font-size: 10px;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.5);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .options-select {
    background: #27272b;
    border: 1px solid rgba(255, 255, 255, 0.16);
    color: #fff;
    border-radius: 6px;
    font-size: 12px;
    padding: 6px 10px;
    outline: none;
    cursor: pointer;
    width: 100%;
    transition: border-color 0.15s;
  }

  .options-select:focus {
    border-color: var(--accent-color, #3ea6ff);
  }

  .container-toggle-row {
    display: flex;
    border-radius: 6px;
    overflow: hidden;
    border: 1px solid rgba(255, 255, 255, 0.16);
    background: #27272b;
  }

  .container-btn {
    padding: 6px 14px;
    background: transparent;
    border: none;
    color: rgba(255, 255, 255, 0.7);
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }

  .container-btn.active {
    background: var(--accent-color, #3ea6ff);
    color: #0f0f0f;
  }

  .start-download-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--accent-color, #3ea6ff);
    color: #082f49;
    border: none;
    border-radius: 8px;
    padding: 10px 16px;
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
    transition: filter 0.15s, transform 0.1s;
    width: 100%;
    box-shadow: 0 2px 8px rgba(62, 166, 255, 0.25);
  }

  .start-download-btn:hover:not(:disabled) {
    filter: brightness(1.1);
  }

  .start-download-btn:active:not(:disabled) {
    transform: scale(0.99);
  }

  .start-download-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
    box-shadow: none;
  }

  .popover-divider {
    height: 1px;
    background: rgba(255, 255, 255, 0.08);
  }

  .downloads-list-section {
    padding: 12px 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    overflow-y: auto;
    max-height: 430px;
  }

  .playlist-list-section {
    min-height: 220px;
  }

  .playlist-detail-header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .detail-back {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    border: 0;
    background: transparent;
    color: var(--text-secondary);
    font-size: 11px;
    cursor: pointer;
  }

  .detail-back:hover {
    color: var(--text-primary);
  }

  .playlist-detail-header > div {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .playlist-detail-header strong {
    overflow: hidden;
    color: var(--text-primary);
    font-size: 14px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .playlist-detail-header span,
  .playlist-detail-summary {
    color: var(--text-muted);
    font-size: 11px;
  }

  .playlist-detail-overview,
  .playlist-detail-summary,
  .playlist-selection-actions,
  .playlist-detail-controls {
    display: flex;
    align-items: center;
  }

  .playlist-detail-overview {
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 8px 16px;
    padding: 2px 0;
  }

  .playlist-detail-summary {
    flex-wrap: wrap;
    gap: 0;
  }

  .playlist-detail-summary > span {
    display: inline-flex;
    align-items: center;
  }

  .playlist-detail-summary > span + span::before {
    content: '·';
    margin: 0 7px;
    color: var(--text-muted);
  }

  .playlist-detail-summary .existing-summary {
    color: #86efac;
  }

  .playlist-detail-controls {
    gap: 6px;
    margin-left: auto;
  }

  .playlist-detail-controls button,
  .playlist-item-actions button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    border: 1px solid var(--border-default);
    border-radius: 6px;
    background: var(--surface-1);
    color: var(--text-secondary);
    padding: 5px 8px;
    font-size: 10px;
    cursor: pointer;
  }

  .playlist-detail-controls button.primary {
    border-color: color-mix(in srgb, var(--accent) 55%, transparent);
    background: color-mix(in srgb, var(--accent) 10%, var(--surface-1));
    color: var(--accent);
  }

  .playlist-detail-controls button:hover:not(:disabled) {
    border-color: var(--border-strong);
    background: var(--overlay);
    color: var(--text-primary);
  }

  .playlist-detail-controls button.primary:hover:not(:disabled) {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 16%, var(--surface-1));
    color: var(--accent);
  }

  .playlist-detail-controls button:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }

  .playlist-detail-controls button.danger,
  .playlist-item-actions button.danger {
    color: #fca5a5;
    border-color: rgba(248, 113, 113, 0.35);
  }

  .playlist-activity-banner {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 8px 10px;
    border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent);
    border-radius: 7px;
    background: color-mix(in srgb, var(--accent) 9%, transparent);
    color: var(--accent);
    font-size: 11px;
  }

  .playlist-activity-banner.cooldown {
    border-color: rgba(251, 191, 36, 0.3);
    background: rgba(251, 191, 36, 0.07);
    color: #fbbf24;
  }

  .playlist-selection-actions button,
  .load-more-playlist {
    border: 1px solid var(--border-default);
    border-radius: 6px;
    background: var(--surface-1);
    color: var(--text-secondary);
    padding: 5px 9px;
    font-size: 11px;
    cursor: pointer;
  }

  .playlist-selection-actions button:hover:not(:disabled),
  .load-more-playlist:hover:not(:disabled) {
    color: var(--text-primary);
    border-color: var(--border-strong);
  }

  .playlist-item-list {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .playlist-item-row {
    display: grid;
    grid-template-columns: 20px 30px 56px minmax(0, 1fr) auto 30px;
    align-items: center;
    gap: 8px;
    min-height: 48px;
    padding: 5px 7px;
    border-radius: 6px;
    background: var(--surface-1);
  }

  .playlist-item-row.unavailable {
    opacity: 0.58;
  }

  .playlist-item-row input {
    accent-color: var(--accent);
  }

  .playlist-item-position {
    color: var(--text-muted);
    font-size: 10px;
    text-align: right;
  }

  .playlist-item-thumb {
    display: flex;
    width: 56px;
    height: 34px;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    border-radius: 4px;
    background: #111;
    color: var(--text-muted);
  }

  .playlist-item-thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .playlist-item-copy {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 2px;
  }

  .playlist-item-copy strong {
    overflow: hidden;
    color: var(--text-primary);
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .playlist-item-copy span,
  .playlist-item-status {
    color: var(--text-muted);
    font-size: 9px;
  }

  .playlist-item-status.status-completed,
  .playlist-item-status.status-skipped_existing {
    color: #4ade80;
  }

  .playlist-item-status.status-downloading,
  .playlist-item-status.status-processing {
    color: var(--accent);
  }

  .playlist-item-status.status-failed {
    color: #f87171;
  }

  .playlist-item-error {
    color: #fca5a5 !important;
    line-height: 1.3;
    overflow-wrap: anywhere;
  }

  .playlist-item-progress {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-top: 2px;
  }

  .playlist-item-progress > span {
    color: var(--accent) !important;
  }

  .playlist-item-actions {
    display: flex;
    justify-content: flex-end;
  }

  .playlist-item-actions button {
    width: 28px;
    height: 28px;
    padding: 0;
  }

  .load-more-playlist {
    align-self: center;
    margin-top: 4px;
  }

  .conservative-badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: #86efac;
    font-size: 10px;
    text-transform: none;
  }

  .playlist-empty small {
    max-width: 420px;
    text-align: center;
    color: var(--text-muted);
  }

  .playlist-cards {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .playlist-download-card {
    display: grid;
    grid-template-columns: 112px minmax(0, 1fr) auto;
    align-items: stretch;
    gap: 12px;
    padding: 10px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--surface-1);
  }

  .playlist-download-card.needs-attention {
    border-color: rgba(245, 158, 11, 0.45);
  }

  .playlist-cover {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 70px;
    border-radius: 7px;
    overflow: hidden;
    background: #111;
    color: var(--text-muted);
  }

  .playlist-cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .playlist-card-content {
    display: flex;
    min-width: 0;
    flex-direction: column;
    justify-content: center;
    gap: 6px;
  }

  .playlist-card-heading,
  .playlist-card-meta,
  .playlist-progress-row,
  .playlist-card-footer {
    display: flex;
    align-items: center;
  }

  .playlist-card-heading {
    justify-content: space-between;
    gap: 12px;
  }

  .playlist-card-heading strong {
    overflow: hidden;
    color: var(--text-primary);
    font-size: 13px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .playlist-card-heading > span {
    flex-shrink: 0;
    color: var(--accent);
    font-size: 10px;
    font-weight: 600;
  }

  .playlist-card-heading > span.warning {
    color: #fbbf24;
  }

  .playlist-card-meta,
  .playlist-card-footer,
  .playlist-card-activity {
    gap: 5px;
    color: var(--text-muted);
    font-size: 10px;
  }

  .playlist-card-activity {
    color: var(--accent);
  }

  .playlist-card-activity .hidden {
    visibility: hidden;
  }

  .playlist-progress-row {
    gap: 9px;
    color: var(--text-secondary);
    font-size: 10px;
  }

  .playlist-progress-row .job-progress-bar-wrap {
    flex: 1;
  }

  .library-playlist-mark {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    margin-left: 4px;
    color: #86efac;
  }

  .playlist-card-actions {
    display: flex;
    align-self: center;
    gap: 4px;
  }

  .playlist-action {
    align-self: center;
    width: 30px;
    height: 32px;
    border: 0;
    border-radius: 6px;
    background: var(--overlay);
    color: var(--text-secondary);
    cursor: pointer;
  }

  .playlist-action:hover:not(:disabled) {
    background: var(--overlay-strong, rgba(255, 255, 255, 0.15));
    color: var(--text-primary);
  }

  .playlist-action.primary {
    color: var(--accent);
  }

  .playlist-action.danger {
    color: #f87171;
  }

  .playlist-action:disabled {
    cursor: wait;
    opacity: 0.5;
  }

  @media (max-width: 720px) {
    .downloads-dropdown-card {
      position: fixed;
      inset: 8px;
      width: auto;
      max-width: none;
      max-height: none;
    }

    .url-input-form {
      flex-direction: column;
    }

    .probe-btn {
      min-height: 38px;
    }

    .playlist-download-card {
      grid-template-columns: 80px minmax(0, 1fr);
    }

    .playlist-card-actions {
      grid-column: 1 / -1;
      justify-content: flex-end;
    }

    .playlist-item-row {
      grid-template-columns: 20px 24px minmax(0, 1fr) auto;
    }

    .playlist-item-thumb {
      display: none;
    }
  }

  .downloads-list-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 12px;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.6);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .active-pulse-badge {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--accent-color, #3ea6ff);
    font-size: 11px;
    text-transform: none;
  }

  .downloads-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 24px 0;
    color: rgba(255, 255, 255, 0.4);
    font-size: 12px;
    gap: 6px;
  }

  .jobs-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .job-card {
    display: flex;
    align-items: center;
    gap: 10px;
    background: #202024;
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 8px;
    padding: 8px 10px;
  }

  .job-thumb {
    width: 60px;
    height: 38px;
    border-radius: 4px;
    object-fit: cover;
    background: #000;
    flex-shrink: 0;
  }

  .job-thumb-placeholder {
    width: 60px;
    height: 38px;
    border-radius: 4px;
    background: #2a2a2e;
    display: flex;
    align-items: center;
    justify-content: center;
    color: rgba(255, 255, 255, 0.4);
    flex-shrink: 0;
  }

  .job-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .job-title {
    font-size: 12px;
    font-weight: 500;
    color: #fff;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .job-meta-row {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 10px;
    color: rgba(255, 255, 255, 0.5);
  }

  .format-badge {
    background: rgba(255, 255, 255, 0.1);
    color: #fff;
    font-weight: 600;
    padding: 0 4px;
    border-radius: 3px;
    font-size: 9px;
  }

  .job-status-row {
    margin-top: 2px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .job-progress-bar-wrap {
    width: 100%;
    height: 4px;
    background: rgba(255, 255, 255, 0.1);
    border-radius: 2px;
    overflow: hidden;
  }

  .job-progress-fill {
    height: 100%;
    background: var(--accent-color, #3ea6ff);
    transition: width 0.2s ease-out;
  }

  .job-progress-fill.active-progress {
    min-width: 2%;
    background: linear-gradient(90deg, var(--accent-color, #3ea6ff), #8fd3ff, var(--accent-color, #3ea6ff));
    background-size: 200% 100%;
    animation: downloadProgressPulse 1.4s linear infinite;
  }

  @keyframes downloadProgressPulse {
    from { background-position: 100% 0; }
    to { background-position: -100% 0; }
  }

  .job-progress-metrics {
    display: flex;
    gap: 4px;
    font-size: 10px;
    color: var(--accent-color, #3ea6ff);
    font-weight: 500;
  }

  .job-status-badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    font-weight: 500;
  }

  .job-status-badge.completed {
    color: #4ade80;
  }

  .job-status-badge.failed {
    color: #f87171;
  }

  .download-error-message {
    margin: 2px 0 0;
    color: #fca5a5;
    font-size: 10px;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }

  .job-status-badge.cancelled {
    color: rgba(255, 255, 255, 0.5);
  }

  .job-status-badge.organizing,
  .job-status-badge.queued {
    color: #fbbf24;
  }

  .job-actions {
    display: flex;
    align-items: center;
    flex-shrink: 0;
  }

  .job-action-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    font-weight: 600;
    border: none;
    border-radius: 6px;
    padding: 5px 8px;
    cursor: pointer;
    text-decoration: none;
  }

  .job-action-btn.play {
    background: #4ade80;
    color: #052e16;
  }

  .job-action-btn.download {
    background: var(--accent-color, #3ea6ff);
    color: #082f49;
  }

  .job-action-btn.cancel,
  .job-action-btn.remove {
    background: transparent;
    color: rgba(255, 255, 255, 0.5);
    padding: 6px;
  }

  .job-action-btn.cancel:hover,
  .job-action-btn.remove:hover {
    color: #fff;
    background: rgba(255, 255, 255, 0.1);
  }

  .spinner-small {
    width: 14px;
    height: 14px;
    border: 2px solid rgba(255, 255, 255, 0.3);
    border-top-color: #fff;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    display: inline-block;
  }

  .spinner-tiny {
    width: 10px;
    height: 10px;
    border: 1.5px solid rgba(255, 255, 255, 0.3);
    border-top-color: currentColor;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    display: inline-block;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>

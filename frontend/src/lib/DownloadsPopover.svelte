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

  let isOpen = $state(false);
  let triggerButton = $state<HTMLButtonElement | null>(null);
  let closeButton = $state<HTMLButtonElement | null>(null);
  let dialogElement = $state<HTMLDivElement | null>(null);
  let urlInput = $state('');
  let probing = $state(false);
  let probeError = $state<string | null>(null);
  let probedVideo = $state<VideoProbeResponse | null>(null);

  let selectedQuality = $state('best');
  let selectedContainer = $state<'mkv' | 'mp4'>('mkv');
  let startingDownload = $state(false);

  let jobs = $state<DownloadJob[]>([]);
  let pollInterval: number | null = null;

  let activeCount = $derived(
    jobs.filter((j) => ['downloading', 'queued', 'organizing', 'awaiting_index'].includes(j.status)).length
  );

  onMount(() => {
    void loadJobs();
    void loadSettings();
    startPolling();
    return () => stopPolling();
  });

  function startPolling() {
    stopPolling();
    pollInterval = window.setInterval(() => {
      // Poll every 1.2s if popover is open or if there are active downloads
      if (isOpen || activeCount > 0) {
        void loadJobs();
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

  async function togglePopover() {
    isOpen = !isOpen;
    if (isOpen) {
      void loadJobs();
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
    class:has-active={activeCount > 0}
    bind:this={triggerButton}
    onclick={togglePopover}
    title={activeCount > 0 ? t('downloads.active_count_tip', { count: activeCount }) : t('downloads.title')}
    aria-label={t('downloads.open_manager')}
    aria-expanded={isOpen}
    aria-haspopup="dialog"
    aria-controls="downloads-manager"
  >
    <span class="material-symbols-outlined icon-20">
      {activeCount > 0 ? 'downloading' : 'download'}
    </span>
    {#if activeCount > 0}
      <span class="downloads-badge">{activeCount}</span>
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

        {#if probeError}
          <Notice tone="error">{probeError}</Notice>
        {/if}

        <!-- Card de Prévia do Vídeo Consultado -->
        {#if probedVideo}
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
              <div class="job-card" class:status-completed={job.status === 'completed'} class:status-failed={job.status === 'failed'}>
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
                        <div class="job-progress-fill" style="width: {job.progress_percent}%"></div>
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
                    {:else if job.status === 'failed'}
                      <div class="job-status-badge failed" title={job.error_message ?? t('downloads.unknown_error')}>
                        <span class="material-symbols-outlined icon-14">cancel</span>
                        <span>{t('downloads.failed')}</span>
                      </div>
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
    width: 520px;
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
    max-height: 320px;
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

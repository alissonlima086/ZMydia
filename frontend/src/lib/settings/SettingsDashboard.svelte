<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { t } from '../i18n/i18n.svelte';
  import Button from '../ui/Button.svelte';
  import ConfirmDialog from '../ui/ConfirmDialog.svelte';
  import Notice from '../ui/Notice.svelte';
  import PageState from '../ui/PageState.svelte';
  import SettingGroup from '../ui/SettingGroup.svelte';
  import { type DashboardStats, type UpdateStatus, formatBytes, formatDuration, messageFrom, request } from './types';

  type ReorganizePreview = {
    moves: Array<{ media_id: number; current_path: string; target_path: string; size_bytes: number; collision: boolean }>;
    already_organized: number;
    collisions: number;
    required_bytes: number;
  };

  let stats = $state<DashboardStats | null>(null);
  let loading = $state(true);
  let errorMessage = $state<string | null>(null);
  let feedback = $state<string | null>(null);
  let preview = $state<ReorganizePreview | null>(null);
  let loadingPreview = $state(false);
  let organizingAll = $state(false);
  let showExecuteConfirm = $state(false);
  let updateStatus = $state<UpdateStatus | null>(null);
  let updateBusy = $state(false);
  let updateTimer: number | null = null;

  onMount(() => {
    void loadStats();
    void loadUpdateStatus();
  });
  onDestroy(() => {
    if (updateTimer !== null) window.clearTimeout(updateTimer);
  });

  function scheduleUpdatePoll() {
    if (updateTimer !== null) window.clearTimeout(updateTimer);
    updateTimer = window.setTimeout(() => void loadUpdateStatus(), 750);
  }

  async function loadUpdateStatus() {
    try {
      updateStatus = await request<UpdateStatus>('/api/updates/status');
      if (updateStatus.phase === 'downloading' || updateStatus.phase === 'checking' || updateStatus.phase === 'applying') scheduleUpdatePoll();
    } catch (error) {
      if (updateStatus?.phase === 'applying') {
        scheduleUpdatePoll();
      } else {
        errorMessage = messageFrom(error);
      }
    }
  }

  async function checkUpdates() {
    updateBusy = true;
    try {
      updateStatus = await request<UpdateStatus>('/api/updates/check', { method: 'POST' });
    } catch (error) {
      errorMessage = messageFrom(error);
      await loadUpdateStatus();
    } finally {
      updateBusy = false;
    }
  }

  async function downloadUpdate() {
    updateBusy = true;
    try {
      updateStatus = await request<UpdateStatus>('/api/updates/download', { method: 'POST' });
      scheduleUpdatePoll();
    } catch (error) {
      errorMessage = messageFrom(error);
    } finally {
      updateBusy = false;
    }
  }

  async function applyUpdate() {
    updateBusy = true;
    errorMessage = null;
    try {
      updateStatus = await request<UpdateStatus>('/api/updates/apply', { method: 'POST' });
      scheduleUpdatePoll();
    } catch (error) {
      errorMessage = messageFrom(error);
      await loadUpdateStatus();
    } finally {
      updateBusy = false;
    }
  }

  async function loadStats() {
    loading = true;
    errorMessage = null;
    try {
      stats = await request<DashboardStats>('/api/dashboard');
    } catch (error) {
      errorMessage = messageFrom(error);
    } finally {
      loading = false;
    }
  }

  async function loadReorganizePreview() {
    loadingPreview = true;
    feedback = null;
    try {
      const result = await request<ReorganizePreview>('/api/organizer/reorganize-all/preview');
      if (result.moves.length === 0) {
        preview = null;
        feedback = t('settings.dashboard.reorganize_nothing');
      } else {
        preview = result;
      }
    } catch (error) {
      errorMessage = messageFrom(error);
    } finally {
      loadingPreview = false;
    }
  }

  async function executeReorganization() {
    showExecuteConfirm = false;
    organizingAll = true;
    try {
      const summary = await request<{ moved: number; already_organized: number; errors: string[] }>(
        '/api/organizer/reorganize-all',
        { method: 'POST' }
      );
      feedback = `${t('settings.dashboard.reorganize_success', {
        moved: summary.moved,
        already_organized: summary.already_organized
      })}${summary.errors.length ? ` ${t('settings.dashboard.errors_count', { count: summary.errors.length })}` : ''}`;
      preview = null;
      await loadStats();
    } catch (error) {
      errorMessage = messageFrom(error);
    } finally {
      organizingAll = false;
    }
  }
</script>

<div class="settings-dashboard-view">
  {#if feedback}<Notice tone="success">{feedback}</Notice>{/if}
  {#if errorMessage}<Notice tone="error">{errorMessage}</Notice>{/if}

  {#if loading && !stats}
    <PageState kind="loading" title={t('settings.dashboard.refreshing')} />
  {:else if stats}
    <SettingGroup title={t('settings.dashboard.title')} description={t('settings.dashboard.subtitle')}>
      {#snippet actions()}
        <Button size="sm" variant="ghost" onclick={loadStats} disabled={loading}>
          <span class="material-symbols-outlined icon-18" aria-hidden="true">refresh</span>
          {t('catalog.refresh_btn')}
        </Button>
      {/snippet}

      <div class="dashboard-summary">
        <div class="dashboard-metric">
          <span>{t('settings.dashboard.total_videos')}</span>
          <strong>{stats.total_videos}</strong>
          <small>{formatDuration(stats.total_duration_ms)}</small>
        </div>
        <div class="dashboard-metric">
          <span>{t('settings.dashboard.card_storage')}</span>
          <strong>{formatBytes(stats.total_size_bytes)}</strong>
          <small>{stats.total_channels} {t('settings.dashboard.channels_unit')}</small>
        </div>
        <div class="dashboard-metric">
          <span>{t('settings.dashboard.videos_with_subtitles')}</span>
          <strong>{stats.videos_with_subtitles}</strong>
          <small>{t('settings.dashboard.indexed_files', { count: stats.total_subtitles })}</small>
        </div>
      </div>
    </SettingGroup>

    <SettingGroup title={t('settings.dashboard.health_title')} description={t('settings.dashboard.health_desc')}>
      <div class="health-list">
        <div class="health-row">
          <span class="status-dot" class:warning={stats.generated_thumbnails > 0}></span>
          <div><strong>{t('settings.dashboard.local_thumbnails')}</strong><small>{t('settings.dashboard.local_thumbnails_sub')}</small></div>
          <span>{stats.generated_thumbnails}</span>
        </div>
        <div class="health-row">
          <span class="status-dot" class:warning={stats.official_thumbnails < stats.with_youtube_id}></span>
          <div><strong>{t('settings.dashboard.official_processed')}</strong><small>{t('settings.dashboard.official_processed_sub')}</small></div>
          <span>{stats.official_thumbnails}/{stats.with_youtube_id}</span>
        </div>
      </div>
    </SettingGroup>

    {#if updateStatus}
      <SettingGroup title={t('settings.dashboard.update_title')} description={t('settings.dashboard.update_desc')}>
        <div class="update-panel">
          <div class="update-summary">
            <div>
              <strong>{t('settings.dashboard.current_version', { version: updateStatus.current_version })}</strong>
              <small>
                {#if updateStatus.phase === 'available'}
                  {t('settings.dashboard.update_available', { version: updateStatus.available_version ?? '' })}
                {:else if updateStatus.phase === 'downloading'}
                  {t('settings.dashboard.update_downloading', { percent: Math.round(updateStatus.progress_percent) })}
                {:else if updateStatus.phase === 'downloaded'}
                  {t('settings.dashboard.update_downloaded', { version: updateStatus.available_version ?? '' })}
                {:else if updateStatus.phase === 'applying'}
                  {t('settings.dashboard.update_applying')}
                {:else if updateStatus.phase === 'installed'}
                  {t('settings.dashboard.update_installed', { version: updateStatus.current_version })}
                {:else if updateStatus.phase === 'rolled_back'}
                  {t('settings.dashboard.update_rolled_back')}
                {:else if updateStatus.phase === 'up_to_date'}
                  {t('settings.dashboard.update_current')}
                {:else if updateStatus.phase === 'unsupported'}
                  {t('settings.dashboard.update_external')}
                {:else if updateStatus.phase === 'error'}
                  {updateStatus.error_message ?? t('common.operation_failed')}
                {:else}
                  {t('settings.dashboard.update_not_checked')}
                {/if}
              </small>
            </div>
            <div class="update-actions">
              {#if updateStatus.notes_url && updateStatus.phase !== 'idle'}
                <a class="update-notes" href={updateStatus.notes_url} target="_blank" rel="noopener noreferrer">{t('settings.dashboard.release_notes')}</a>
              {/if}
              {#if updateStatus.phase === 'available' && updateStatus.can_download}
                <Button size="sm" variant="primary" onclick={downloadUpdate} disabled={updateBusy}>{t('settings.dashboard.download_update')}</Button>
              {:else if updateStatus.phase === 'downloaded' && updateStatus.can_self_update}
                <Button size="sm" variant="primary" onclick={applyUpdate} disabled={updateBusy}>{t('settings.dashboard.install_update')}</Button>
              {:else if updateStatus.phase !== 'downloading' && updateStatus.phase !== 'applying' && updateStatus.phase !== 'unsupported'}
                <Button size="sm" variant="secondary" onclick={checkUpdates} disabled={updateBusy || updateStatus.phase === 'checking'}>{t('settings.dashboard.check_updates')}</Button>
              {/if}
            </div>
          </div>
          {#if updateStatus.phase === 'downloading'}
            <progress max="100" value={updateStatus.progress_percent} aria-label={t('settings.dashboard.update_progress')}></progress>
            <small>{formatBytes(updateStatus.downloaded_bytes)} / {formatBytes(updateStatus.total_bytes)}</small>
          {/if}
          {#if !updateStatus.can_self_update}
            <Notice tone="info">
              {#if updateStatus.installation_method === 'nix'}
                {t('settings.dashboard.update_instruction_nix')}
              {:else if updateStatus.installation_method === 'debian'}
                {t('settings.dashboard.update_instruction_debian', { version: updateStatus.available_version ?? '…' })}
              {:else if updateStatus.installation_method === 'portable'}
                {t('settings.dashboard.update_instruction_portable')}
              {:else if updateStatus.installation_method === 'development'}
                {t('settings.dashboard.update_instruction_development')}
              {/if}
            </Notice>
          {/if}
        </div>
      </SettingGroup>
    {/if}

    <SettingGroup
      title={t('settings.dashboard.organization_title')}
      description={t('settings.dashboard.organization_desc')}
    >
      <div class="organizer-panel">
        {#if preview}
          <Notice tone={preview.collisions > 0 ? 'warning' : 'info'} title={t('settings.dashboard.preview_ready')}>
            {t('settings.dashboard.preview_summary', { moves: preview.moves.length, collisions: preview.collisions, size: formatBytes(preview.required_bytes) })}
          </Notice>
          <div class="organizer-actions">
            <Button variant="ghost" onclick={() => (preview = null)}>{t('settings.dashboard.discard_preview')}</Button>
            <Button variant="primary" onclick={() => (showExecuteConfirm = true)}>{t('settings.dashboard.review_execute')}</Button>
          </div>
        {:else}
          <Button variant="secondary" onclick={loadReorganizePreview} disabled={loadingPreview}>
            <span class="material-symbols-outlined icon-18" aria-hidden="true">preview</span>
            {loadingPreview ? t('settings.dashboard.generating_preview') : t('settings.dashboard.generate_preview')}
          </Button>
        {/if}
      </div>
    </SettingGroup>
  {/if}
</div>

<ConfirmDialog
  open={showExecuteConfirm}
  title={t('settings.dashboard.execute_title')}
  description={preview ? t('settings.dashboard.execute_desc', { count: preview.moves.length }) : ''}
  confirmLabel={t('settings.dashboard.execute_confirm')}
  dangerous={true}
  busy={organizingAll}
  onConfirm={() => void executeReorganization()}
  onCancel={() => (showExecuteConfirm = false)}
/>

<style>
  .settings-dashboard-view { display: grid; gap: var(--space-6); }
  .dashboard-summary { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); }
  .dashboard-metric { min-width: 0; display: grid; gap: var(--space-1); padding: var(--space-4); }
  .dashboard-metric + .dashboard-metric { border-left: 1px solid var(--border-subtle); }
  .dashboard-metric span { color: var(--text-muted); font: var(--type-caption); }
  .dashboard-metric strong { overflow: hidden; color: var(--text-primary); font-size: 1.5rem; text-overflow: ellipsis; }
  .dashboard-metric small { color: var(--text-secondary); font: var(--type-caption); }
  .health-list { display: grid; }
  .health-row { min-height: 64px; display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: var(--space-3); padding: var(--space-3) var(--space-4); }
  .health-row + .health-row { border-top: 1px solid var(--border-subtle); }
  .health-row div { display: grid; gap: var(--space-1); }
  .health-row strong { font: var(--type-label); }
  .health-row small { color: var(--text-muted); font: var(--type-caption); }
  .status-dot { width: 9px; height: 9px; border-radius: var(--radius-full); background: var(--success); }
  .status-dot.warning { background: var(--warning); }
  .organizer-panel { display: grid; gap: var(--space-3); padding: var(--space-4); }
  .update-panel { display: grid; gap: var(--space-2); padding: var(--space-4); }
  .update-summary { display: flex; align-items: center; justify-content: space-between; gap: var(--space-4); }
  .update-summary > div:first-child { display: grid; gap: var(--space-1); }
  .update-summary small { color: var(--text-muted); }
  .update-actions { display: flex; align-items: center; gap: var(--space-3); }
  .update-notes { color: var(--accent); font: var(--type-label); }
  progress { width: 100%; accent-color: var(--accent); }
  .organizer-actions { display: flex; justify-content: flex-end; gap: var(--space-2); }
  @media (max-width: 680px) {
    .dashboard-summary { grid-template-columns: 1fr; }
    .dashboard-metric + .dashboard-metric { border-top: 1px solid var(--border-subtle); border-left: 0; }
    .organizer-actions { align-items: stretch; flex-direction: column; }
    .update-summary { align-items: stretch; flex-direction: column; }
    .update-actions { justify-content: space-between; }
  }
</style>

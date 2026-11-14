<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../i18n/i18n.svelte';
  import Button from '../ui/Button.svelte';
  import ConfirmDialog from '../ui/ConfirmDialog.svelte';
  import Notice from '../ui/Notice.svelte';
  import PageState from '../ui/PageState.svelte';
  import SettingGroup from '../ui/SettingGroup.svelte';
  import { type DashboardStats, formatBytes, formatDuration, messageFrom, request } from './types';

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

  onMount(() => void loadStats());

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
  .organizer-actions { display: flex; justify-content: flex-end; gap: var(--space-2); }
  @media (max-width: 680px) {
    .dashboard-summary { grid-template-columns: 1fr; }
    .dashboard-metric + .dashboard-metric { border-top: 1px solid var(--border-subtle); border-left: 0; }
    .organizer-actions { align-items: stretch; flex-direction: column; }
  }
</style>

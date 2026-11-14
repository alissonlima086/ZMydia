<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../i18n/i18n.svelte';
  import Button from '../ui/Button.svelte';
  import Notice from '../ui/Notice.svelte';
  import SettingGroup from '../ui/SettingGroup.svelte';
  import SettingRow from '../ui/SettingRow.svelte';
  import ToggleSwitch from '../ui/ToggleSwitch.svelte';
  import { type YoutubeSettings, type YoutubeStatus, messageFrom, request } from './types';

  let settings = $state<YoutubeSettings>({ use_browser_cookies: false, browser: 'firefox' });
  let saved = $state<YoutubeSettings>({ use_browser_cookies: false, browser: 'firefox' });
  let status = $state<YoutubeStatus | null>(null);
  let saving = $state(false);
  let syncing = $state(false);
  let feedback = $state<{ text: string; tone: 'success' | 'error' } | null>(null);
  const dirty = $derived(JSON.stringify(settings) !== JSON.stringify(saved));

  onMount(() => {
    void Promise.all([loadSettings(), loadStatus()]);
    const interval = window.setInterval(() => { if (status?.is_running) void loadStatus(); }, 1500);
    return () => window.clearInterval(interval);
  });
  async function loadSettings() { try { settings = await request('/api/settings/youtube'); saved = { ...settings }; } catch {} }
  async function loadStatus() { try { status = await request('/api/youtube/status'); } catch {} }
  function show(text: string, tone: 'success' | 'error' = 'success') { feedback = { text, tone }; }
  async function saveSettings() {
    saving = true;
    try { settings = await request('/api/settings/youtube', { method: 'PUT', body: JSON.stringify(settings) }); saved = { ...settings }; show(t('settings.youtube.saved_feedback')); }
    catch (error) { show(messageFrom(error), 'error'); } finally { saving = false; }
  }
  async function startSync(query = '') {
    syncing = true;
    try { await request(`/api/youtube/sync${query}`, { method: 'POST' }); show(t('settings.youtube.sync_started_feedback')); await loadStatus(); }
    catch (error) { show(messageFrom(error), 'error'); } finally { syncing = false; }
  }
</script>

<div class="integration-settings">
  {#if feedback}<Notice tone={feedback.tone}>{feedback.text}</Notice>{/if}
  <SettingGroup title={t('settings.youtube.heading_title')} description={t('settings.youtube.cookies_hint')}>
    <SettingRow label={t('settings.youtube.cookies_label')} description={t('settings.youtube.cookies_when_needed')}>
      {#snippet control()}<ToggleSwitch bind:checked={settings.use_browser_cookies} disabled={saving} label={t('settings.youtube.cookies_label')} />{/snippet}
    </SettingRow>
    {#if settings.use_browser_cookies}
      <SettingRow label={t('settings.youtube.browser_label')} description={t('settings.youtube.browser_required')}>
        {#snippet control()}<select bind:value={settings.browser} disabled={saving}><option value="firefox">Mozilla Firefox</option><option value="chrome">Google Chrome</option><option value="brave">Brave</option><option value="chromium">Chromium</option><option value="edge">Microsoft Edge</option><option value="opera">Opera</option><option value="vivaldi">Vivaldi</option></select>{/snippet}
      </SettingRow>
    {/if}
    <div class="save-row"><span class:dirty>{dirty ? t('common.unsaved_changes') : t('settings.youtube.configuration_saved')}</span><Button variant="primary" onclick={saveSettings} disabled={!dirty || saving}>{saving ? t('common.saving') : t('common.save')}</Button></div>
  </SettingGroup>

  <SettingGroup title={t('settings.youtube.worker_title')} description={t('settings.youtube.worker_desc')}>
    <SettingRow label={status?.is_running ? t('settings.youtube.sync_in_progress') : t('settings.youtube.worker_available')} description={status?.current_title || (status?.last_run_at ? t('settings.youtube.last_run', { date: new Date(status.last_run_at).toLocaleString() }) : t('settings.youtube.no_runs'))}>
      {#snippet control()}<span class:running={status?.is_running} class="worker-state">{status?.is_running ? `${status.processed}/${status.total_candidates}` : t('settings.youtube.completed_count', { count: status?.successful ?? 0 })}</span>{/snippet}
    </SettingRow>
    <SettingRow label={t('settings.youtube.pending_failures')} description={t('settings.youtube.pending_failures_desc')}>
      {#snippet control()}<strong class:error={(status?.failed ?? 0) > 0}>{status?.failed ?? 0}</strong>{/snippet}
    </SettingRow>
    <div class="actions">
      <Button onclick={() => startSync()} disabled={syncing || status?.is_running}>{t('settings.youtube.sync_new_btn')}</Button>
      {#if (status?.failed ?? 0) > 0}<Button onclick={() => startSync('?reset_failed=true')} disabled={syncing || status?.is_running}>{t('settings.youtube.retry_failed_btn')}</Button>{/if}
      <Button variant="ghost" onclick={() => startSync('?force_unofficial=true')} disabled={syncing || status?.is_running}>{t('settings.youtube.sync_remaining_btn')}</Button>
    </div>
  </SettingGroup>
</div>

<style>
  .integration-settings { display: grid; gap: var(--space-6); }
  select { min-height: var(--control-height); padding: 0 var(--space-3); border: 1px solid var(--border-default); border-radius: var(--radius-md); background: var(--surface-0); color: var(--text-primary); }
  .save-row, .actions { display: flex; align-items: center; justify-content: flex-end; gap: var(--space-2); padding: var(--space-3) var(--space-4); border-top: 1px solid var(--border-subtle); }
  .save-row span { margin-right: auto; color: var(--text-muted); font: var(--type-caption); }
  .save-row span.dirty, .error { color: var(--warning); }
  .worker-state { padding: 3px var(--space-2); border-radius: 999px; background: var(--surface-3); color: var(--text-secondary); font: var(--type-caption); }
  .worker-state.running { background: var(--accent-soft); color: var(--accent); }
  @media (max-width: 560px) { .actions { align-items: stretch; flex-direction: column; } }
</style>

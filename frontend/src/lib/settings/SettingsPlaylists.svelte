<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../i18n/i18n.svelte';
  import Button from '../ui/Button.svelte';
  import Notice from '../ui/Notice.svelte';
  import SettingGroup from '../ui/SettingGroup.svelte';
  import SettingRow from '../ui/SettingRow.svelte';
  import ToggleSwitch from '../ui/ToggleSwitch.svelte';
  import { messageFrom, request, type PlaylistDownloadSettings } from './types';

  const defaults: Record<'conservative' | 'balanced' | 'fast', PlaylistDownloadSettings> = {
    conservative: { active_profile: 'conservative', min_cooldown_seconds: 180, max_cooldown_seconds: 600, dynamic_duration_cooldown: true, daily_video_limit: 30, daily_gb_limit: 50, daily_limit_action: 'pause_and_notify' },
    balanced: { active_profile: 'balanced', min_cooldown_seconds: 45, max_cooldown_seconds: 120, dynamic_duration_cooldown: false, daily_video_limit: null, daily_gb_limit: null, daily_limit_action: 'ignore' },
    fast: { active_profile: 'fast', min_cooldown_seconds: 5, max_cooldown_seconds: 15, dynamic_duration_cooldown: false, daily_video_limit: null, daily_gb_limit: null, daily_limit_action: 'ignore' }
  };
  let settings = $state<PlaylistDownloadSettings>({ ...defaults.conservative });
  let saved = $state<PlaylistDownloadSettings>({ ...defaults.conservative });
  let videoLimitEnabled = $state(true);
  let gbLimitEnabled = $state(true);
  let saving = $state(false);
  let feedback = $state<{ text: string; tone: 'success' | 'error' } | null>(null);
  const dirty = $derived(JSON.stringify(settings) !== JSON.stringify(saved));

  onMount(async () => {
    try {
      settings = await request('/api/settings/playlist-downloads');
      saved = structuredClone(settings);
      videoLimitEnabled = settings.daily_video_limit !== null;
      gbLimitEnabled = settings.daily_gb_limit !== null;
    } catch (error) { feedback = { text: messageFrom(error), tone: 'error' }; }
  });

  function selectProfile(profile: PlaylistDownloadSettings['active_profile']) {
    settings = profile === 'custom'
      ? { ...settings, active_profile: 'custom' }
      : structuredClone(defaults[profile]);
    videoLimitEnabled = settings.daily_video_limit !== null;
    gbLimitEnabled = settings.daily_gb_limit !== null;
  }

  async function save() {
    saving = true;
    feedback = null;
    try {
      const payload = { ...settings };
      if (payload.active_profile === 'custom') {
        payload.daily_video_limit = videoLimitEnabled ? (payload.daily_video_limit ?? 30) : null;
        payload.daily_gb_limit = gbLimitEnabled ? (payload.daily_gb_limit ?? 50) : null;
        payload.daily_limit_action = (videoLimitEnabled || gbLimitEnabled) ? 'pause_and_notify' : 'ignore';
      }
      settings = await request('/api/settings/playlist-downloads', { method: 'PUT', body: JSON.stringify(payload) });
      saved = structuredClone(settings);
      feedback = { text: t('settings.playlists.saved'), tone: 'success' };
    } catch (error) { feedback = { text: messageFrom(error), tone: 'error' }; }
    finally { saving = false; }
  }

  function toggleVideoLimit() {
    settings.daily_video_limit = settings.daily_video_limit === null ? 30 : null;
  }

  function toggleGbLimit() {
    settings.daily_gb_limit = settings.daily_gb_limit === null ? 50 : null;
  }
</script>

<div class="playlist-settings">
  {#if feedback}<Notice tone={feedback.tone}>{feedback.text}</Notice>{/if}
  <SettingGroup title={t('settings.playlists.title')} description={t('settings.playlists.description')}>
    <div class="profiles">
      {#each ['conservative', 'balanced', 'fast', 'custom'] as profile}
        <button type="button" class:active={settings.active_profile === profile} onclick={() => selectProfile(profile as PlaylistDownloadSettings['active_profile'])}>
          <strong>{t(`settings.playlists.profile_${profile}`)}</strong>
          <span>{t(`settings.playlists.profile_${profile}_desc`)}</span>
        </button>
      {/each}
    </div>

    {#if settings.active_profile === 'custom'}
      <SettingRow label={t('settings.playlists.cooldown')} description={t('settings.playlists.cooldown_desc')}>
        {#snippet control()}
          <label>{t('settings.playlists.minimum')} <input type="number" min="0" max="86400" bind:value={settings.min_cooldown_seconds} /></label>
          <label>{t('settings.playlists.maximum')} <input type="number" min={settings.min_cooldown_seconds} max="86400" bind:value={settings.max_cooldown_seconds} /></label>
        {/snippet}
      </SettingRow>
      <SettingRow label={t('settings.playlists.dynamic')} description={t('settings.playlists.dynamic_desc')}>
        {#snippet control()}<ToggleSwitch bind:checked={settings.dynamic_duration_cooldown} label={t('settings.playlists.dynamic')} />{/snippet}
      </SettingRow>
      <SettingRow label={t('settings.playlists.video_limit')} description={t('settings.playlists.limit_desc')}>
        {#snippet control()}<ToggleSwitch bind:checked={videoLimitEnabled} onchange={toggleVideoLimit} label={t('settings.playlists.video_limit')} />{#if videoLimitEnabled}<input type="number" min="1" max="10000" bind:value={settings.daily_video_limit} />{/if}{/snippet}
      </SettingRow>
      <SettingRow label={t('settings.playlists.gb_limit')} description={t('settings.playlists.limit_desc')}>
        {#snippet control()}<ToggleSwitch bind:checked={gbLimitEnabled} onchange={toggleGbLimit} label={t('settings.playlists.gb_limit')} />{#if gbLimitEnabled}<input type="number" min="1" max="100000" bind:value={settings.daily_gb_limit} />{/if}{/snippet}
      </SettingRow>
    {/if}
    <div class="save-row"><span class:dirty>{dirty ? t('common.unsaved_changes') : t('common.no_pending_changes')}</span><Button variant="primary" onclick={save} disabled={!dirty || saving}>{saving ? t('common.saving') : t('common.save')}</Button></div>
  </SettingGroup>
</div>

<style>
  .playlist-settings { display: grid; gap: var(--space-6); }
  .profiles { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--space-3); padding: var(--space-4); }
  .profiles button { display: grid; gap: var(--space-1); padding: var(--space-4); border: 1px solid var(--border-subtle); border-radius: var(--radius-md); background: var(--surface-0); color: var(--text-primary); text-align: left; cursor: pointer; }
  .profiles button.active { border-color: var(--accent); background: var(--accent-soft); }
  .profiles span { color: var(--text-muted); font: var(--type-caption); }
  label { display: flex; align-items: center; gap: var(--space-2); color: var(--text-secondary); font: var(--type-caption); }
  input { width: 92px; min-height: var(--control-height); padding: 0 var(--space-2); border: 1px solid var(--border-default); border-radius: var(--radius-md); background: var(--surface-0); color: var(--text-primary); }
  .save-row { display: flex; align-items: center; justify-content: flex-end; gap: var(--space-3); padding: var(--space-3) var(--space-4); border-top: 1px solid var(--border-subtle); }
  .save-row span { margin-right: auto; color: var(--text-muted); font: var(--type-caption); }
  .save-row span.dirty { color: var(--warning); }
  @media (max-width: 620px) { .profiles { grid-template-columns: 1fr; } }
</style>

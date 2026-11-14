<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../i18n/i18n.svelte';
  import { musicStore } from '../music.svelte';
  import Button from '../ui/Button.svelte';
  import ConfirmDialog from '../ui/ConfirmDialog.svelte';
  import Notice from '../ui/Notice.svelte';
  import SettingGroup from '../ui/SettingGroup.svelte';
  import SettingRow from '../ui/SettingRow.svelte';
  import ToggleSwitch from '../ui/ToggleSwitch.svelte';
  import type { ListenBrainzSettings } from './types';

  let settings = $state<ListenBrainzSettings>({ enabled: false, user_token: '', username: null, auto_scrobble: true });
  let token = $state('');
  let showToken = $state(false);
  let busy = $state(false);
  let disconnectOpen = $state(false);
  let feedback = $state<{ text: string; tone: 'success' | 'error' } | null>(null);

  onMount(async () => { await musicStore.init(); settings = { ...musicStore.settings }; token = settings.user_token; });
  function show(text: string, tone: 'success' | 'error' = 'success') { feedback = { text, tone }; }
  async function save(patch: Partial<ListenBrainzSettings>, success: string) {
    const previous = { ...settings }; settings = { ...settings, ...patch }; busy = true;
    try { await musicStore.saveSettings(settings); show(success); }
    catch (error: any) { settings = previous; show(error.message || t('settings.listenbrainz.save_error'), 'error'); }
    finally { busy = false; }
  }
  async function connect() {
    const clean = token.trim(); if (!clean) return show(t('settings.listenbrainz.token_required'), 'error');
    busy = true;
    try {
      const result = await musicStore.validateToken(clean);
      if (!result.valid || !result.username) return show(result.message || t('settings.listenbrainz.invalid_token'), 'error');
      settings = { ...settings, user_token: clean, username: result.username };
      await musicStore.saveSettings(settings); show(`${t('settings.listenbrainz.connected_as')} @${result.username}`);
    } catch (error: any) { show(error.message || t('settings.listenbrainz.connect_error'), 'error'); }
    finally { busy = false; }
  }
  async function disconnect() {
    disconnectOpen = false; token = '';
    await save({ user_token: '', username: null }, t('settings.listenbrainz.disconnected_success'));
  }
</script>

<div class="integration-settings">
  {#if feedback}<Notice tone={feedback.tone}>{feedback.text}</Notice>{/if}
  <SettingGroup title={t('settings.listenbrainz.title')} description={t('settings.listenbrainz.subtitle')}>
    <SettingRow label={t('settings.listenbrainz.enable_module_title')} description={t('settings.listenbrainz.enable_module_desc')}>
      {#snippet control()}<ToggleSwitch checked={settings.enabled} disabled={busy} label={t('settings.listenbrainz.enable_module_title')} onchange={() => save({ enabled: !settings.enabled }, settings.enabled ? t('settings.listenbrainz.module_disabled') : t('settings.listenbrainz.module_enabled'))} />{/snippet}
    </SettingRow>
  </SettingGroup>

  {#if settings.enabled}
    <SettingGroup title={t('settings.listenbrainz.account_section_title')} description={t('settings.listenbrainz.account_section_desc')}>
      {#if settings.username}
        <SettingRow label={`@${settings.username}`} description={settings.token_hint ? t('settings.listenbrainz.token_stored', { hint: settings.token_hint }) : t('settings.listenbrainz.status_connected')}>
          {#snippet control()}<span class="connected"><span aria-hidden="true"></span>{t('settings.listenbrainz.status_connected')}</span>{/snippet}
        </SettingRow>
        <div class="actions"><a class="profile-link" href={`https://listenbrainz.org/user/${settings.username}/`} target="_blank" rel="noopener noreferrer">{t('settings.listenbrainz.view_profile')}</a><Button variant="danger" onclick={() => (disconnectOpen = true)} disabled={busy}>{t('settings.listenbrainz.disconnect')}</Button></div>
      {:else}
        <div class="token-form">
          <label for="listenbrainz-token">{t('settings.listenbrainz.user_token_label')}</label>
          <div class="token-row"><span class="token-input"><input id="listenbrainz-token" type={showToken ? 'text' : 'password'} bind:value={token} autocomplete="off" /><button type="button" onclick={() => (showToken = !showToken)} aria-label={showToken ? t('settings.listenbrainz.hide_token') : t('settings.listenbrainz.show_token')}><span class="material-symbols-outlined" aria-hidden="true">{showToken ? 'visibility_off' : 'visibility'}</span></button></span><Button variant="primary" onclick={connect} disabled={busy || !token.trim()}>{busy ? t('settings.listenbrainz.validating') : t('settings.listenbrainz.validate_and_connect')}</Button></div>
          <a href="https://listenbrainz.org/settings/" target="_blank" rel="noopener noreferrer">{t('settings.listenbrainz.get_token_link')}</a>
        </div>
      {/if}
    </SettingGroup>

    <SettingGroup title={t('settings.listenbrainz.scrobble_options_title')} description={t('settings.listenbrainz.scrobble_options_desc')}>
      <SettingRow label={t('settings.listenbrainz.auto_scrobble_label')} description={t('settings.listenbrainz.auto_scrobble_desc')}>
        {#snippet control()}<ToggleSwitch checked={settings.auto_scrobble} disabled={busy || !settings.username} label={t('settings.listenbrainz.auto_scrobble_label')} onchange={() => save({ auto_scrobble: !settings.auto_scrobble }, t('settings.listenbrainz.options_saved'))} />{/snippet}
      </SettingRow>
    </SettingGroup>
  {/if}
</div>

<ConfirmDialog open={disconnectOpen} title={t('settings.listenbrainz.disconnect_title')} description={t('settings.listenbrainz.disconnect_desc')} confirmLabel={t('settings.listenbrainz.disconnect')} dangerous onConfirm={disconnect} onCancel={() => (disconnectOpen = false)} busy={busy} />

<style>
  .integration-settings { display: grid; gap: var(--space-6); }
  .connected { display: inline-flex; align-items: center; gap: var(--space-2); color: var(--success); font: var(--type-caption); }
  .connected span { width: 8px; height: 8px; border-radius: 50%; background: var(--success); }
  .actions, .token-form { padding: var(--space-4); border-top: 1px solid var(--border-subtle); }
  .actions { display: flex; align-items: center; justify-content: flex-end; gap: var(--space-2); }
  .profile-link { margin-right: auto; color: var(--accent); font: var(--type-label); }
  .token-form { display: grid; gap: var(--space-3); }
  .token-form > label { color: var(--text-primary); font: var(--type-label); }
  .token-form > a { width: fit-content; color: var(--accent); font: var(--type-caption); }
  .token-row { display: flex; gap: var(--space-2); }
  .token-input { position: relative; flex: 1; display: flex; }
  .token-input input { width: 100%; min-width: 0; min-height: var(--control-height); padding: 0 44px 0 var(--space-3); border: 1px solid var(--border-default); border-radius: var(--radius-md); background: var(--surface-0); color: var(--text-primary); }
  .token-input button { position: absolute; inset: 0 0 0 auto; width: 42px; border: 0; background: transparent; color: var(--text-muted); cursor: pointer; }
  @media (max-width: 560px) { .token-row, .actions { align-items: stretch; flex-direction: column; } .profile-link { margin-right: 0; } }
</style>

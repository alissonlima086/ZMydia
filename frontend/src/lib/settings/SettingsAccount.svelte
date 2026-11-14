<script lang="ts">
  import { onMount } from 'svelte';
  import { apiErrorMessage, apiFetch } from '../api';
  import { authStore } from '../auth.svelte';
  import { t } from '../i18n/i18n.svelte';
  import Button from '../ui/Button.svelte';
  import Notice from '../ui/Notice.svelte';
  import PasswordField from '../ui/PasswordField.svelte';
  import SettingGroup from '../ui/SettingGroup.svelte';
  import SettingRow from '../ui/SettingRow.svelte';

  interface UserProfile { id: number; username: string; created_at: string; }

  let profile = $state<UserProfile | null>(null);
  let loadingProfile = $state(true);
  let newUsername = $state('');
  let savingProfile = $state(false);
  let currentPassword = $state('');
  let newPassword = $state('');
  let confirmPassword = $state('');
  let showCurrentPassword = $state(false);
  let showNewPassword = $state(false);
  let showConfirmPassword = $state(false);
  let savingPassword = $state(false);
  let revokeApiTokens = $state(false);
  let feedbackMessage = $state<string | null>(null);
  let feedbackType = $state<'success' | 'error'>('success');
  let feedbackTimeout: number | null = null;

  const profileDirty = $derived(Boolean(profile) && newUsername.trim() !== profile?.username);
  const passwordReady = $derived(currentPassword.length > 0 && newPassword.length >= 8 && newPassword === confirmPassword);

  function showFeedback(message: string, type: 'success' | 'error' = 'success') {
    feedbackMessage = message;
    feedbackType = type;
    if (feedbackTimeout) clearTimeout(feedbackTimeout);
    feedbackTimeout = window.setTimeout(() => (feedbackMessage = null), 5000);
  }

  async function loadProfile() {
    loadingProfile = true;
    try {
      const response = await apiFetch('/api/user/profile');
      if (!response.ok) throw new Error(await apiErrorMessage(response));
      profile = (await response.json()) as UserProfile;
      newUsername = profile.username;
    } catch (error: any) {
      showFeedback(error.message || t('settings.account.load_error'), 'error');
    } finally {
      loadingProfile = false;
    }
  }

  onMount(() => { void loadProfile(); });

  async function handleUpdateProfile(event: SubmitEvent) {
    event.preventDefault();
    const username = newUsername.trim();
    if (username.length < 3 || username.length > 50) {
      showFeedback(t('settings.account.username_len_error'), 'error');
      return;
    }
    savingProfile = true;
    try {
      const response = await apiFetch('/api/user/profile', {
        method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ username })
      });
      if (!response.ok) throw new Error(await apiErrorMessage(response));
      profile = (await response.json()) as UserProfile;
      newUsername = profile.username;
      authStore.username = profile.username;
      if (localStorage.getItem('mymedia_saved_username')) localStorage.setItem('mymedia_saved_username', profile.username);
      showFeedback(t('settings.account.username_updated'));
    } catch (error: any) {
      showFeedback(error.message || t('settings.account.load_error'), 'error');
    } finally {
      savingProfile = false;
    }
  }

  async function handleUpdatePassword(event: SubmitEvent) {
    event.preventDefault();
    if (!currentPassword) return showFeedback(t('settings.account.current_password_required'), 'error');
    if (newPassword.length < 8) return showFeedback(t('settings.account.new_password_min_error'), 'error');
    if (newPassword !== confirmPassword) return showFeedback(t('settings.account.passwords_mismatch'), 'error');
    savingPassword = true;
    try {
      const response = await apiFetch('/api/user/password', {
        method: 'PUT', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ current_password: currentPassword, new_password: newPassword, revoke_api_tokens: revokeApiTokens })
      });
      if (!response.ok) throw new Error(await apiErrorMessage(response));
      currentPassword = ''; newPassword = ''; confirmPassword = ''; revokeApiTokens = false;
      showFeedback(t('settings.account.password_updated'));
    } catch (error: any) {
      showFeedback(error.message || t('settings.account.load_error'), 'error');
    } finally {
      savingPassword = false;
    }
  }

  function formatDate(value: string) {
    return new Date(value).toLocaleString(undefined, { day: '2-digit', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit' });
  }
</script>

<div class="account-settings">
  {#if feedbackMessage}<Notice tone={feedbackType}>{feedbackMessage}</Notice>{/if}

  <SettingGroup title={t('settings.account.title')} description={t('settings.account.subtitle')}>
    <SettingRow label={`@${profile?.username || authStore.username || '…'}`} description={profile?.created_at ? `${t('settings.account.account_created')}: ${formatDate(profile.created_at)}` : undefined}>
      {#snippet control()}<span class="status-badge"><span aria-hidden="true"></span>{t('settings.account.session_tag')}</span>{/snippet}
    </SettingRow>
    <SettingRow label={t('settings.account.hash_tag')} description={t('settings.account.key_tag')}>
      {#snippet control()}<span class="material-symbols-outlined security-icon" aria-hidden="true">verified_user</span>{/snippet}
    </SettingRow>
  </SettingGroup>

  <SettingGroup title={t('settings.account.username_section_title')} description={t('settings.account.username_section_desc')}>
    <form class="form-row" onsubmit={handleUpdateProfile}>
      <label for="account-username">{t('settings.account.username_label')}</label>
      <div class="field-actions">
        <input id="account-username" type="text" bind:value={newUsername} autocomplete="username" minlength="3" maxlength="50" disabled={loadingProfile || savingProfile} />
        <Button type="submit" variant="primary" disabled={!profileDirty || savingProfile}>
          {savingProfile ? t('settings.account.changing_username') : t('settings.account.change_username_btn')}
        </Button>
      </div>
      <small class:dirty={profileDirty}>{profileDirty ? t('common.unsaved_changes') : t('common.no_pending_changes')}</small>
    </form>
  </SettingGroup>

  <SettingGroup title={t('settings.account.password_section_title')} description={t('settings.account.password_section_desc')}>
    <form class="password-form" onsubmit={handleUpdatePassword}>
      <div class="current-password"><PasswordField id="current-password" label={t('settings.account.current_password_label')} bind:value={currentPassword} bind:visible={showCurrentPassword} autocomplete="current-password" /></div>
      <PasswordField id="new-password" label={t('settings.account.new_password_label')} bind:value={newPassword} bind:visible={showNewPassword} autocomplete="new-password" minlength={8} />
      <PasswordField id="confirm-password" label={t('settings.account.confirm_password_label')} bind:value={confirmPassword} bind:visible={showConfirmPassword} autocomplete="new-password" minlength={8} />
      <label class="check-control"><input type="checkbox" bind:checked={revokeApiTokens} />{t('settings.account.revoke_tokens')}</label>
      <div class="form-footer">
        <span>{t('settings.account.other_sessions_warning')}</span>
        <Button type="submit" variant="primary" disabled={!passwordReady || savingPassword}>
          {savingPassword ? t('settings.account.changing_password') : t('settings.account.change_password_btn')}
        </Button>
      </div>
    </form>
  </SettingGroup>

  <SettingGroup title={t('settings.account.sessions_title')} description={t('settings.account.sessions_desc')}>
    <SettingRow label={t('settings.account.current_session')} description={t('settings.account.current_session_desc')}>
      {#snippet control()}<span class="status-badge"><span aria-hidden="true"></span>{t('common.active')}</span>{/snippet}
    </SettingRow>
    <SettingRow label={t('settings.account.other_devices')} description={t('settings.account.other_devices_desc')}>
      {#snippet control()}<span class="material-symbols-outlined security-icon" aria-hidden="true">devices</span>{/snippet}
    </SettingRow>
  </SettingGroup>
</div>

<style>
  .account-settings { display: grid; gap: var(--space-6); }
  .status-badge { display: inline-flex; align-items: center; gap: var(--space-2); color: var(--text-secondary); font: var(--type-caption); }
  .status-badge > span { width: 8px; height: 8px; border-radius: 50%; background: var(--success); }
  .security-icon { color: var(--success); }
  .form-row, .password-form { display: grid; gap: var(--space-3); padding: var(--space-4); }
  .form-row > label { color: var(--text-primary); font: var(--type-label); }
  .field-actions { display: flex; gap: var(--space-2); }
  input[type='text'] { min-width: 0; min-height: var(--control-height); padding: 0 var(--space-3); border: 1px solid var(--border-default); border-radius: var(--radius-md); background: var(--surface-0); color: var(--text-primary); font: var(--type-body); }
  .field-actions > input { flex: 1; }
  small { color: var(--text-muted); font: var(--type-caption); }
  small.dirty { color: var(--warning); }
  .password-form { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .current-password { grid-column: 1 / -1; }
  .check-control { grid-column: 1 / -1; display: flex; align-items: center; gap: var(--space-2); color: var(--text-secondary); font: var(--type-caption); }
  .check-control input { width: 18px; height: 18px; accent-color: var(--accent); }
  .form-footer { grid-column: 1 / -1; display: flex; align-items: center; justify-content: space-between; gap: var(--space-4); padding-top: var(--space-2); color: var(--text-muted); font: var(--type-caption); }
  @media (max-width: 620px) {
    .password-form { grid-template-columns: 1fr; }
    .current-password, .check-control, .form-footer { grid-column: auto; }
    .field-actions, .form-footer { align-items: stretch; flex-direction: column; }
  }
</style>

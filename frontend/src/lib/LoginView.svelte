<script lang="ts">
  import { authStore } from './auth.svelte';
  import Button from './ui/Button.svelte';
  import Notice from './ui/Notice.svelte';
  import PasswordField from './ui/PasswordField.svelte';
  import { t } from './i18n/i18n.svelte';

  const REMEMBER_KEY = 'mymedia_remember_me';
  const USER_KEY = 'mymedia_saved_username';
  const remembered = localStorage.getItem(REMEMBER_KEY) === 'true';
  let mode = $state<'login' | 'recover'>('login');
  let rememberMe = $state(remembered);
  let username = $state(remembered ? localStorage.getItem(USER_KEY) || '' : '');
  let password = $state('');
  let showPassword = $state(false);
  let recoveryCode = $state('');
  let newPassword = $state('');
  let confirmation = $state('');
  let showNewPassword = $state(false);
  let showConfirmation = $state(false);
  let replacementCode = $state<string | null>(null);
  let localError = $state<string | null>(null);

  async function login(event: SubmitEvent) {
    event.preventDefault(); localError = null;
    if (!(await authStore.login(username.trim(), password, rememberMe))) return;
    if (rememberMe) { localStorage.setItem(REMEMBER_KEY, 'true'); localStorage.setItem(USER_KEY, username.trim()); }
    else { localStorage.removeItem(REMEMBER_KEY); localStorage.removeItem(USER_KEY); }
  }
  async function recover(event: SubmitEvent) {
    event.preventDefault(); localError = null;
    if (newPassword.length < 8) return void (localError = t('auth.password_min'));
    if (newPassword !== confirmation) return void (localError = t('auth.password_mismatch'));
    replacementCode = await authStore.recover(username.trim(), recoveryCode.trim(), newPassword);
  }
  function returnToLogin() { mode = 'login'; replacementCode = null; recoveryCode = ''; newPassword = ''; confirmation = ''; authStore.error = null; }
</script>

<main class="auth-shell">
  <section class="auth-card">
    <div class="brand"><span class="material-symbols-outlined" aria-hidden="true">play_circle</span><span>ZMydia</span></div>
    {#if replacementCode}
      <p class="eyebrow">{t('auth.reset_done_eyebrow')}</p><h1>{t('auth.save_new_code')}</h1>
      <p class="description">{t('auth.sessions_revoked')}</p>
      <Notice tone="warning">{t('auth.old_code_invalid')}</Notice>
      <code>{replacementCode}</code>
      <Button variant="primary" onclick={returnToLogin}>{t('auth.back_to_login_saved')}</Button>
    {:else if mode === 'recover'}
      <p class="eyebrow">{t('auth.offline_recovery')}</p><h1>{t('auth.reset_password')}</h1>
      <p class="description">{t('auth.recovery_description')}</p>
      {#if localError || authStore.error}<Notice tone="error">{localError || authStore.error}</Notice>{/if}
      <form onsubmit={recover}>
        <label for="recovery-user">{t('auth.username')}</label><input id="recovery-user" bind:value={username} autocomplete="username" required />
        <label for="recovery-code">{t('auth.recovery_code')}</label><input id="recovery-code" bind:value={recoveryCode} autocomplete="off" required />
        <PasswordField id="recovery-password" label={t('auth.new_password')} bind:value={newPassword} bind:visible={showNewPassword} autocomplete="new-password" minlength={8} />
        <PasswordField id="recovery-confirmation" label={t('auth.confirm_new_password')} bind:value={confirmation} bind:visible={showConfirmation} autocomplete="new-password" minlength={8} />
        <Button type="submit" variant="primary" disabled={authStore.loading}>{authStore.loading ? t('auth.resetting') : t('auth.reset_and_revoke')}</Button>
        <Button type="button" variant="ghost" onclick={returnToLogin}>{t('auth.back_to_login')}</Button>
      </form>
    {:else}
      <p class="eyebrow">{t('auth.local_access')}</p><h1>{t('auth.sign_in_title')}</h1>
      <p class="description">{t('auth.sign_in_description')}</p>
      {#if localError || authStore.error}<Notice tone="error">{localError || authStore.error}</Notice>{/if}
      <form onsubmit={login}>
        <label for="username">{t('auth.username')}</label><input id="username" bind:value={username} autocomplete="username" required disabled={authStore.loading} />
        <PasswordField id="password" label={t('auth.password')} bind:value={password} bind:visible={showPassword} autocomplete="current-password" />
        <label class="remember"><input type="checkbox" bind:checked={rememberMe} disabled={authStore.loading} />{t('auth.remember_login')}</label>
        <Button type="submit" variant="primary" disabled={authStore.loading}>{authStore.loading ? t('auth.signing_in') : t('auth.sign_in_title')}</Button>
        <Button type="button" variant="ghost" onclick={() => { mode = 'recover'; authStore.error = null; }}>{t('auth.forgot_password')}</Button>
      </form>
    {/if}
  </section>
</main>

<style>
  .auth-shell { position: fixed; inset: 0; z-index: 9999; display: grid; place-items: center; padding: var(--space-4); background: radial-gradient(circle at 50% 45%, var(--accent-soft), transparent 38%), var(--surface-0); }
  .auth-card { width: min(420px, 100%); display: grid; gap: var(--space-4); padding: var(--space-8); border: 1px solid var(--border-default); border-radius: var(--radius-xl); background: var(--surface-1); box-shadow: var(--shadow-overlay); }
  .brand { display: flex; align-items: center; gap: var(--space-2); color: var(--accent); font: var(--type-label); } .eyebrow { margin: var(--space-2) 0 calc(-1 * var(--space-3)); color: var(--accent); font: var(--type-caption); letter-spacing: .12em; }
  h1 { margin: 0; color: var(--text-primary); font: var(--type-page-title); } .description { margin: 0; color: var(--text-secondary); font: var(--type-body); }
  form { display: grid; gap: var(--space-3); } form > label:not(.remember) { color: var(--text-primary); font: var(--type-label); } form > input { min-height: var(--control-height); padding: 0 var(--space-3); border: 1px solid var(--border-default); border-radius: var(--radius-md); background: var(--surface-0); color: var(--text-primary); }
  .remember { display: flex; align-items: center; gap: var(--space-2); color: var(--text-secondary); font: var(--type-caption); } .remember input { width: 18px; height: 18px; accent-color: var(--accent); }
  code { overflow-wrap: anywhere; padding: var(--space-4); border: 1px solid var(--border-default); border-radius: var(--radius-md); background: var(--surface-0); color: var(--text-primary); }
  @media (max-width: 480px) { .auth-card { padding: var(--space-5); } }
</style>

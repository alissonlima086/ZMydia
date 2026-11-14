<script lang="ts">
  import { authStore } from './auth.svelte';
  import Button from './ui/Button.svelte';
  import Notice from './ui/Notice.svelte';
  import PasswordField from './ui/PasswordField.svelte';
  import { t } from './i18n/i18n.svelte';

  let username = $state('');
  let password = $state('');
  let confirmation = $state('');
  let showPassword = $state(false);
  let showConfirmation = $state(false);
  let copied = $state(false);
  let error = $state<string | null>(null);

  async function submit(event: SubmitEvent) {
    event.preventDefault(); error = null;
    if (password !== confirmation) return void (error = t('auth.password_mismatch'));
    if (password.length < 8) return void (error = t('auth.password_min'));
    if (!(await authStore.setup(username.trim(), password))) error = authStore.error ?? t('auth.setup_failed');
  }
  async function copyCode() { if (!authStore.recoveryCode) return; await navigator.clipboard.writeText(authStore.recoveryCode); copied = true; }
</script>

<main class="auth-shell">
  <section class="auth-card">
    <div class="brand"><span class="material-symbols-outlined" aria-hidden="true">shield_lock</span><span>ZMydia</span></div>
    {#if authStore.recoveryCode}
      <p class="eyebrow">{t('auth.offline_recovery')}</p><h1>{t('auth.save_code')}</h1>
      <p class="description">{t('auth.save_code_description')}</p>
      <Notice tone="warning">{t('auth.save_code_warning')}</Notice>
      <code>{authStore.recoveryCode}</code>
      <Button onclick={copyCode}>{copied ? t('auth.code_copied') : t('auth.copy_code')}</Button>
      <Button variant="primary" onclick={() => authStore.finishSetup()}>{t('auth.enter_after_saving')}</Button>
    {:else}
      <p class="eyebrow">{t('auth.first_access')}</p><h1>{t('auth.create_admin')}</h1>
      <p class="description">{t('auth.create_admin_description')}</p>
      {#if error}<Notice tone="error">{error}</Notice>{/if}
      <form onsubmit={submit}>
        <label for="setup-user">{t('auth.username')}</label><input id="setup-user" bind:value={username} minlength="3" maxlength="50" autocomplete="username" required />
        <PasswordField id="setup-password" label={t('auth.password_or_passphrase')} bind:value={password} bind:visible={showPassword} autocomplete="new-password" minlength={8} />
        <PasswordField id="setup-confirm" label={t('auth.confirm_password')} bind:value={confirmation} bind:visible={showConfirmation} autocomplete="new-password" minlength={8} />
        <small>{t('auth.password_min')}</small>
        <Button type="submit" variant="primary" disabled={authStore.loading}>{authStore.loading ? t('auth.creating_account') : t('auth.create_admin')}</Button>
      </form>
    {/if}
  </section>
</main>

<style>
  .auth-shell { position: fixed; inset: 0; z-index: 9999; display: grid; place-items: center; padding: var(--space-4); background: var(--surface-0); }
  .auth-card { width: min(440px, 100%); display: grid; gap: var(--space-4); padding: var(--space-8); border: 1px solid var(--border-default); border-radius: var(--radius-xl); background: var(--surface-1); box-shadow: var(--shadow-overlay); }
  .brand { display: flex; align-items: center; gap: var(--space-2); color: var(--accent); font: var(--type-label); } .eyebrow { margin: var(--space-2) 0 calc(-1 * var(--space-3)); color: var(--accent); font: var(--type-caption); letter-spacing: .12em; }
  h1 { margin: 0; color: var(--text-primary); font: var(--type-page-title); } .description { margin: 0; color: var(--text-secondary); font: var(--type-body); }
  form { display: grid; gap: var(--space-3); } form > label { color: var(--text-primary); font: var(--type-label); } form > input { min-height: var(--control-height); padding: 0 var(--space-3); border: 1px solid var(--border-default); border-radius: var(--radius-md); background: var(--surface-0); color: var(--text-primary); } small { color: var(--text-muted); }
  code { overflow-wrap: anywhere; padding: var(--space-4); border: 1px solid var(--border-default); border-radius: var(--radius-md); background: var(--surface-0); color: var(--text-primary); }
  @media (max-width: 480px) { .auth-card { padding: var(--space-5); } }
</style>

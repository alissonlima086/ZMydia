<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { apiErrorMessage, apiFetch } from '../api';
  import { t } from '../i18n/i18n.svelte';
  import Button from '../ui/Button.svelte';
  import ConfirmDialog from '../ui/ConfirmDialog.svelte';
  import Notice from '../ui/Notice.svelte';
  import SettingGroup from '../ui/SettingGroup.svelte';
  import SettingRow from '../ui/SettingRow.svelte';
  import type { ApiTokenItem, CreateApiTokenResponse, ExtensionInfo } from './types';

  let info = $state<ExtensionInfo>({ has_signed: false, has_unsigned: false, active_type: null, download_url: null });
  let tokens = $state<ApiTokenItem[]>([]);
  let loading = $state(true);
  let pairing = $state(false);
  let challenge = $state<{ id: string; display_code: string } | null>(null);
  let fallbackToken = $state<string | null>(null);
  let copied = $state(false);
  let extensionOrigin = $state<string | null>(null);
  let revokeTarget = $state<ApiTokenItem | null>(null);
  let feedback = $state<{ text: string; tone: 'success' | 'error' | 'warning' } | null>(null);

  onMount(() => {
    void Promise.all([loadInfo(), loadTokens()]);
    window.addEventListener('message', handleMessage);
    if (new URLSearchParams(location.search).get('action') === 'pair') void startPairing();
  });
  onDestroy(() => window.removeEventListener('message', handleMessage));
  function handleMessage(event: MessageEvent) {
    if (event.source !== window || !event.data || typeof event.data !== 'object') return;
    if ((event.data.type === 'MYMEDIA_EXTENSION_LOADED' || event.data.type === 'MYMEDIA_PONG_EXTENSION') && typeof event.data.extensionOrigin === 'string' && event.data.extensionOrigin.startsWith('moz-extension://')) extensionOrigin = event.data.extensionOrigin;
    if (event.data.type === 'MYMEDIA_EXTENSION_PAIRED_SUCCESS') { feedback = { text: t('settings.extension.pair_success'), tone: 'success' }; fallbackToken = null; void loadTokens(); }
    if (event.data.type === 'MYMEDIA_EXTENSION_PAIRED_ERROR') feedback = { text: event.data.error || t('settings.extension.pair_error'), tone: 'error' };
  }
  async function loadInfo() { try { const response = await apiFetch('/api/extension/info'); if (response.ok) info = await response.json(); } catch {} }
  async function loadTokens() { loading = true; try { const response = await apiFetch('/api/extension/tokens'); if (response.ok) tokens = await response.json(); } finally { loading = false; } }
  async function startPairing() {
    pairing = true; challenge = null; fallbackToken = null; feedback = null;
    window.postMessage({ type: 'MYMEDIA_PING_EXTENSION' }, '*');
    await new Promise((resolve) => setTimeout(resolve, 150));
    try {
      const response = await apiFetch('/api/extension/pairings', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name: `Firefox (${new Date().toLocaleDateString()})`, extension_origin: extensionOrigin }) });
      if (!response.ok) throw new Error(await apiErrorMessage(response));
      challenge = await response.json();
    } catch (error: any) { feedback = { text: error.message || t('settings.extension.pair_start_error'), tone: 'error' }; }
    finally { pairing = false; }
  }
  async function confirmPairing() {
    if (!challenge) return; pairing = true;
    try {
      const response = await apiFetch(`/api/extension/pairings/${challenge.id}/confirm`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ code: challenge.display_code }) });
      if (!response.ok) throw new Error(await apiErrorMessage(response));
      const result = (await response.json()) as CreateApiTokenResponse;
      window.postMessage({ type: 'MYMEDIA_EXTENSION_PAIR_TOKEN', token: result.token, serverUrl: location.origin, tokenHint: result.token_hint }, '*');
      fallbackToken = result.token; challenge = null; feedback = { text: t('settings.extension.pair_success'), tone: 'success' }; await loadTokens();
    } catch (error: any) { feedback = { text: error.message || t('settings.extension.pair_confirm_error'), tone: 'error' }; }
    finally { pairing = false; }
  }
  async function copyToken() { if (!fallbackToken) return; await navigator.clipboard.writeText(fallbackToken); copied = true; setTimeout(() => (copied = false), 2500); }
  async function revoke() {
    if (!revokeTarget) return; const target = revokeTarget; pairing = true;
    try { const response = await apiFetch(`/api/extension/tokens/${target.id}`, { method: 'DELETE' }); if (!response.ok) throw new Error(await apiErrorMessage(response)); revokeTarget = null; await loadTokens(); feedback = { text: t('settings.extension.token_revoked'), tone: 'success' }; }
    catch (error: any) { feedback = { text: error.message || t('settings.extension.revoke_error'), tone: 'error' }; }
    finally { pairing = false; }
  }
  function date(value: string | null) { return value ? new Date(value).toLocaleString() : t('settings.extension.token_never_used'); }
</script>

<div class="integration-settings">
  {#if feedback}<Notice tone={feedback.tone}>{feedback.text}</Notice>{/if}
  <SettingGroup title={t('settings.extension.card_title')} description={t('settings.extension.card_desc')}>
    <SettingRow label={t('settings.extension.firefox_package')} description={info.has_signed ? t('settings.extension.signed_badge') : info.has_unsigned ? t('settings.extension.dev_badge') : t('settings.extension.pending_badge')}>
      {#snippet control()}{#if info.has_signed || info.has_unsigned}<a class="install-button" href="/extension/zmydia.xpi">{t('settings.extension.add_btn')}</a>{:else}<Button disabled>{t('settings.extension.build_first')}</Button>{/if}{/snippet}
    </SettingRow>
    <SettingRow label={t('settings.extension.connection_status')} description={t('settings.extension.connection_desc')}>
      {#snippet control()}<span class:connected={Boolean(extensionOrigin)} class="connection-state">{extensionOrigin ? t('settings.extension.detected') : t('settings.extension.not_detected')}</span>{/snippet}
    </SettingRow>
  </SettingGroup>

  <SettingGroup title={t('settings.extension.pairing_card_title')} description={t('settings.extension.pairing_flow')}>
    <div class="pairing-step"><span>1</span><div><strong>{t('settings.extension.install_title')}</strong><small>{t('settings.extension.install_desc')}</small></div></div>
    <div class="pairing-step"><span>2</span><div><strong>{t('settings.extension.pair_title')}</strong><small>{t('settings.extension.pair_desc')}</small></div><Button onclick={startPairing} disabled={pairing}>{pairing ? t('settings.extension.pairing_in_progress') : t('settings.extension.pair_browser_btn')}</Button></div>
    {#if challenge}<div class="challenge"><div><small>{t('settings.extension.confirm_code')}</small><strong>{challenge.display_code}</strong></div><Button variant="primary" onclick={confirmPairing} disabled={pairing}>{t('settings.extension.confirm_device')}</Button></div>{/if}
    {#if fallbackToken}<Notice tone="warning" title={t('settings.extension.pair_fallback_notice')}><div class="fallback"><code>{fallbackToken}</code><Button size="sm" onclick={copyToken}>{copied ? t('settings.extension.token_copied') : t('settings.extension.copy_token')}</Button></div></Notice>{/if}
  </SettingGroup>

  <SettingGroup title={t('settings.extension.tokens_list_title', { count: tokens.length })} description={t('settings.extension.tokens_list_desc')}>
    {#if loading}<div class="empty">{t('common.loading')}</div>{:else if tokens.length === 0}<div class="empty">{t('settings.extension.no_tokens')}</div>{:else}
      <div class="token-table"><div class="token-head"><span>{t('settings.extension.device')}</span><span>{t('settings.extension.token_scopes')}</span><span>{t('settings.extension.token_last_used')}</span><span></span></div>{#each tokens as token (token.id)}<div class="token-row"><div><strong>{token.name}</strong><small>{token.token_hint}</small></div><div class="scopes">{#each token.scopes.split(',') as scope}<span>{scope.trim()}</span>{/each}</div><span>{date(token.last_used_at)}</span><Button size="sm" variant="danger" onclick={() => (revokeTarget = token)}>{t('settings.extension.revoke_btn')}</Button></div>{/each}</div>
    {/if}
  </SettingGroup>
</div>

<ConfirmDialog open={Boolean(revokeTarget)} title={t('settings.extension.revoke_title')} description={revokeTarget ? t('settings.extension.revoke_desc', { name: revokeTarget.name }) : ''} confirmLabel={t('settings.extension.revoke_btn')} dangerous busy={pairing} onConfirm={revoke} onCancel={() => (revokeTarget = null)} />

<style>
  .integration-settings { display: grid; gap: var(--space-6); }
  .install-button { min-height: var(--control-height); display: inline-flex; align-items: center; padding: 0 var(--space-4); border-radius: var(--radius-md); background: var(--accent); color: white; font: var(--type-label); text-decoration: none; }
  .connection-state { color: var(--text-muted); font: var(--type-caption); } .connection-state.connected { color: var(--success); }
  .pairing-step { min-height: 72px; display: grid; grid-template-columns: auto 1fr auto; align-items: center; gap: var(--space-3); padding: var(--space-3) var(--space-4); }
  .pairing-step + .pairing-step { border-top: 1px solid var(--border-subtle); }
  .pairing-step > span { width: 28px; height: 28px; display: grid; place-items: center; border-radius: 50%; background: var(--surface-3); color: var(--text-secondary); font: var(--type-label); }
  .pairing-step div { display: grid; gap: var(--space-1); } .pairing-step strong { font: var(--type-label); } .pairing-step small { color: var(--text-muted); font: var(--type-caption); }
  .challenge { display: flex; align-items: center; justify-content: space-between; gap: var(--space-3); padding: var(--space-4); border-top: 1px solid var(--border-subtle); background: var(--accent-soft); }
  .challenge div { display: grid; gap: var(--space-1); } .challenge small { color: var(--text-muted); } .challenge strong { color: var(--accent); font: 700 1.4rem monospace; letter-spacing: .16em; }
  .fallback { display: flex; align-items: center; gap: var(--space-2); } .fallback code { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; }
  .empty { padding: var(--space-8); text-align: center; color: var(--text-muted); }
  .token-head, .token-row { display: grid; grid-template-columns: minmax(150px, 1fr) 1fr minmax(130px, .7fr) auto; align-items: center; gap: var(--space-3); padding: var(--space-3) var(--space-4); }
  .token-head { background: var(--surface-2); color: var(--text-muted); font: var(--type-caption); } .token-row + .token-row { border-top: 1px solid var(--border-subtle); }
  .token-row > div:first-child { min-width: 0; display: grid; gap: var(--space-1); } .token-row strong { overflow: hidden; text-overflow: ellipsis; font: var(--type-label); } .token-row small, .token-row > span { color: var(--text-muted); font: var(--type-caption); }
  .scopes { display: flex; flex-wrap: wrap; gap: var(--space-1); } .scopes span { padding: 2px var(--space-2); border-radius: 999px; background: var(--accent-soft); color: var(--accent); font: var(--type-caption); }
  @media (max-width: 680px) { .token-head { display: none; } .token-row { grid-template-columns: 1fr auto; } .token-row .scopes, .token-row > span { grid-column: 1; } }
  @media (max-width: 560px) { .pairing-step { grid-template-columns: auto 1fr; } .pairing-step :global(.ui-button) { grid-column: 1 / -1; } .challenge, .fallback { align-items: stretch; flex-direction: column; } }
</style>

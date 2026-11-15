<script lang="ts">
  import type { HTMLInputAttributes } from 'svelte/elements';
  import { t } from '../i18n/i18n.svelte';

  let {
    id,
    label,
    value = $bindable(''),
    visible = $bindable(false),
    autocomplete,
    minlength
  }: {
    id: string;
    label: string;
    value?: string;
    visible?: boolean;
    autocomplete: HTMLInputAttributes['autocomplete'];
    minlength?: number;
  } = $props();
</script>

<label class="password-field" for={id}>
  <span>{label}</span>
  <span class="password-input">
    <input {id} type={visible ? 'text' : 'password'} bind:value {autocomplete} {minlength} />
    <button type="button" onclick={() => (visible = !visible)} aria-label={visible ? t('common.hide_password') : t('common.show_password')}>
      <span class="material-symbols-outlined" aria-hidden="true">{visible ? 'visibility_off' : 'visibility'}</span>
    </button>
  </span>
</label>

<style>
  .password-field { display: grid; gap: var(--space-2); }
  .password-field > span:first-child { color: var(--text-primary); font: var(--type-label); }
  .password-input { display: flex; position: relative; }
  input { width: 100%; min-width: 0; min-height: var(--control-height); padding: 0 44px 0 var(--space-3); border: 1px solid var(--border-default); border-radius: var(--radius-md); background: var(--surface-0); color: var(--text-primary); font: var(--type-body); }
  button { position: absolute; inset: 0 0 0 auto; width: 42px; border: 0; background: transparent; color: var(--text-muted); cursor: pointer; }
  .material-symbols-outlined { font-size: 19px; }
</style>

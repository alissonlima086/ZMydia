<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    tone = 'info',
    title,
    children
  }: {
    tone?: 'info' | 'success' | 'warning' | 'error';
    title?: string;
    children: Snippet;
  } = $props();

  const icons = { info: 'info', success: 'check_circle', warning: 'warning', error: 'error' } as const;
</script>

<div class={`ui-notice ui-notice--${tone}`} role={tone === 'error' ? 'alert' : 'status'}>
  <span class="material-symbols-outlined" aria-hidden="true">{icons[tone]}</span>
  <div>
    {#if title}<strong>{title}</strong>{/if}
    <div class="ui-notice__body">{@render children()}</div>
  </div>
</div>

<style>
  .ui-notice { display: flex; align-items: flex-start; gap: var(--space-3); padding: var(--space-3) var(--space-4); border: 1px solid var(--border-default); border-radius: var(--radius-md); background: var(--surface-1); color: var(--text-secondary); font: var(--type-body); }
  .ui-notice > .material-symbols-outlined { margin-top: 1px; font-size: 19px; }
  strong { display: block; margin-bottom: var(--space-1); color: var(--text-primary); font: var(--type-label); }
  .ui-notice--info > .material-symbols-outlined { color: var(--accent); }
  .ui-notice--success { border-color: color-mix(in srgb, var(--success) 32%, var(--border-subtle)); }
  .ui-notice--success > .material-symbols-outlined { color: var(--success); }
  .ui-notice--warning { border-color: color-mix(in srgb, var(--warning) 38%, var(--border-subtle)); }
  .ui-notice--warning > .material-symbols-outlined { color: var(--warning); }
  .ui-notice--error { border-color: color-mix(in srgb, var(--danger) 38%, var(--border-subtle)); }
  .ui-notice--error > .material-symbols-outlined { color: var(--danger); }
</style>

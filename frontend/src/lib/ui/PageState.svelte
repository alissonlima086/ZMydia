<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    kind,
    title,
    description,
    icon,
    actions
  }: {
    kind: 'loading' | 'empty' | 'error';
    title: string;
    description?: string;
    icon?: string;
    actions?: Snippet;
  } = $props();
</script>

<section class={`ui-page-state ui-page-state--${kind}`} aria-live={kind === 'error' ? 'assertive' : 'polite'}>
  {#if kind === 'loading'}
    <span class="ui-page-state__spinner" aria-hidden="true"></span>
  {:else if icon}
    <span class="material-symbols-outlined ui-page-state__icon" aria-hidden="true">{icon}</span>
  {/if}
  <div class="ui-page-state__copy">
    <h2>{title}</h2>
    {#if description}<p>{description}</p>{/if}
  </div>
  {#if actions}<div class="ui-page-state__actions">{@render actions()}</div>{/if}
</section>

<style>
  .ui-page-state {
    min-height: 240px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-4);
    padding: var(--space-8) var(--space-6);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-lg);
    background: var(--surface-1);
    text-align: center;
  }
  .ui-page-state__copy { max-width: 520px; }
  h2 { margin: 0; color: var(--text-primary); font: var(--type-section-title); }
  p { margin: var(--space-2) 0 0; color: var(--text-secondary); font: var(--type-body); }
  .ui-page-state__icon { color: var(--text-muted); font-size: 32px; }
  .ui-page-state--error .ui-page-state__icon { color: var(--danger); }
  .ui-page-state__spinner {
    width: 24px;
    height: 24px;
    border: 2px solid var(--border-default);
    border-top-color: var(--accent);
    border-radius: var(--radius-full);
    animation: ui-state-spin 0.8s linear infinite;
  }
  .ui-page-state__actions { display: flex; justify-content: center; gap: var(--space-2); }
  @keyframes ui-state-spin { to { transform: rotate(360deg); } }
</style>

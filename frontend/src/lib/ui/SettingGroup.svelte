<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    title,
    description,
    children,
    actions
  }: {
    title: string;
    description?: string;
    children: Snippet;
    actions?: Snippet;
  } = $props();
</script>

<section class="setting-group">
  <header class="setting-group__header">
    <div>
      <h2>{title}</h2>
      {#if description}<p>{description}</p>{/if}
    </div>
    {#if actions}<div class="setting-group__actions">{@render actions()}</div>{/if}
  </header>
  <div class="setting-group__body">{@render children()}</div>
</section>

<style>
  .setting-group { display: grid; gap: var(--space-3); }
  .setting-group__header { display: flex; align-items: end; justify-content: space-between; gap: var(--space-4); }
  h2 { margin: 0; color: var(--text-primary); font: var(--type-section-title); }
  p { max-width: 680px; margin: var(--space-1) 0 0; color: var(--text-muted); font: var(--type-body); }
  .setting-group__actions { display: flex; gap: var(--space-2); }
  .setting-group__body {
    overflow: hidden;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-lg);
    background: var(--surface-1);
  }
  .setting-group__body :global(.setting-row + .setting-row) { border-top: 1px solid var(--border-subtle); }
  @media (max-width: 560px) {
    .setting-group__header { align-items: stretch; flex-direction: column; }
  }
</style>

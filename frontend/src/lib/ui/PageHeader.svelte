<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    title,
    description,
    eyebrow,
    meta,
    actions
  }: {
    title: string;
    description?: string;
    eyebrow?: string;
    meta?: Snippet;
    actions?: Snippet;
  } = $props();
</script>

<header class="ui-page-header">
  <div class="ui-page-header__copy">
    {#if eyebrow}<p class="ui-page-header__eyebrow">{eyebrow}</p>{/if}
    <div class="ui-page-header__title-row">
      <h1>{title}</h1>
      {#if meta}<div class="ui-page-header__meta">{@render meta()}</div>{/if}
    </div>
    {#if description}<p class="ui-page-header__description">{description}</p>{/if}
  </div>
  {#if actions}<div class="ui-page-header__actions">{@render actions()}</div>{/if}
</header>

<style>
  .ui-page-header {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--space-6);
    min-width: 0;
    padding-block: var(--space-2);
    border-bottom: 1px solid var(--border-subtle);
  }
  .ui-page-header__copy { min-width: 0; }
  .ui-page-header__eyebrow {
    margin: 0 0 var(--space-2);
    color: var(--text-muted);
    font: var(--type-caption);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .ui-page-header__title-row {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
    min-width: 0;
  }
  h1 {
    margin: 0;
    color: var(--text-primary);
    font: var(--type-page-title);
    letter-spacing: -0.025em;
  }
  .ui-page-header__description {
    max-width: 720px;
    margin: var(--space-2) 0 var(--space-3);
    color: var(--text-secondary);
    font: var(--type-body);
  }
  .ui-page-header__meta { color: var(--text-muted); font: var(--type-caption); }
  .ui-page-header__actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding-bottom: var(--space-3);
  }
  @media (max-width: 720px) {
    .ui-page-header { align-items: stretch; flex-direction: column; gap: var(--space-3); }
    .ui-page-header__actions { padding-bottom: var(--space-3); }
  }
</style>

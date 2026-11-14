<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    title,
    description,
    children,
    actions
  }: {
    title?: string;
    description?: string;
    children: Snippet;
    actions?: Snippet;
  } = $props();
</script>

<section class="ui-section">
  {#if title || description || actions}
    <header class="ui-section__header">
      <div>
        {#if title}<h2>{title}</h2>{/if}
        {#if description}<p>{description}</p>{/if}
      </div>
      {#if actions}<div class="ui-section__actions">{@render actions()}</div>{/if}
    </header>
  {/if}
  <div class="ui-section__content">{@render children()}</div>
</section>

<style>
  .ui-section { display: grid; gap: var(--space-4); min-width: 0; }
  .ui-section__header { display: flex; align-items: end; justify-content: space-between; gap: var(--space-4); }
  h2 { margin: 0; color: var(--text-primary); font: var(--type-section-title); }
  p { margin: var(--space-1) 0 0; color: var(--text-secondary); font: var(--type-body); }
  .ui-section__actions { display: flex; align-items: center; gap: var(--space-2); }
  .ui-section__content { min-width: 0; }
</style>

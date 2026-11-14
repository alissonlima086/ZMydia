<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    children,
    width = 'wide',
    className = ''
  }: {
    children: Snippet;
    width?: 'content' | 'wide' | 'full';
    className?: string;
  } = $props();
</script>

<div class={`ui-page-shell ui-page-shell--${width} ${className}`.trim()}>
  {@render children()}
</div>

<style>
  .ui-page-shell {
    display: grid;
    gap: var(--space-6);
    width: 100%;
    min-width: 0;
    margin-inline: auto;
  }

  .ui-page-shell--content { max-width: var(--content-width); }
  .ui-page-shell--wide { max-width: var(--content-width-wide); }
  .ui-page-shell--full { max-width: none; }

  @media (max-width: 720px) {
    .ui-page-shell { gap: var(--space-4); }
  }
</style>

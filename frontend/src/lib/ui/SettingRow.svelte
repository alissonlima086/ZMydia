<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    label,
    description,
    control,
    stacked = false
  }: {
    label: string;
    description?: string;
    control: Snippet;
    stacked?: boolean;
  } = $props();
</script>

<div class="setting-row" class:setting-row--stacked={stacked}>
  <div class="setting-row__copy">
    <div class="setting-row__label">{label}</div>
    {#if description}<div class="setting-row__description">{description}</div>{/if}
  </div>
  <div class="setting-row__control">{@render control()}</div>
</div>

<style>
  .setting-row { min-height: 68px; display: flex; align-items: center; justify-content: space-between; gap: var(--space-6); padding: var(--space-4); }
  .setting-row__copy { min-width: 0; }
  .setting-row__label { color: var(--text-primary); font: var(--type-label); }
  .setting-row__description { max-width: 620px; margin-top: var(--space-1); color: var(--text-muted); font: var(--type-caption); }
  .setting-row__control { flex: 0 0 auto; display: flex; align-items: center; gap: var(--space-2); }
  .setting-row--stacked { align-items: stretch; flex-direction: column; gap: var(--space-3); }
  .setting-row--stacked .setting-row__control { width: 100%; }
  @media (max-width: 560px) {
    .setting-row { align-items: stretch; flex-direction: column; gap: var(--space-3); }
    .setting-row__control { width: 100%; justify-content: flex-start; }
  }
</style>

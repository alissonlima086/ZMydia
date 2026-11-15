<script lang="ts">
  import { t } from '../i18n/i18n.svelte';

  let { count = 8, label = t('common.videos_loading') }: { count?: number; label?: string } = $props();
</script>

<section class="media-skeleton" aria-busy="true" aria-label={label}>
  <span class="sr-only">{label}</span>
  <div class="media-skeleton__grid" aria-hidden="true">
    {#each Array(count) as _}
      <div class="media-skeleton__card">
        <div class="media-skeleton__thumb"></div>
        <div class="media-skeleton__line media-skeleton__line--title"></div>
        <div class="media-skeleton__line media-skeleton__line--meta"></div>
      </div>
    {/each}
  </div>
</section>

<style>
  .media-skeleton__grid {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    column-gap: var(--space-4);
    row-gap: var(--space-8);
  }
  .media-skeleton__card { display: grid; gap: var(--space-2); }
  .media-skeleton__thumb, .media-skeleton__line {
    border-radius: var(--radius-md);
    background: linear-gradient(90deg, var(--surface-1) 20%, var(--surface-2) 50%, var(--surface-1) 80%);
    background-size: 220% 100%;
    animation: skeleton-shimmer 1.5s var(--ease-standard) infinite;
  }
  .media-skeleton__thumb { aspect-ratio: 16 / 9; }
  .media-skeleton__line { height: 12px; }
  .media-skeleton__line--title { width: 88%; }
  .media-skeleton__line--meta { width: 56%; }
  @keyframes skeleton-shimmer { to { background-position-x: -220%; } }
  @media (max-width: 1440px) { .media-skeleton__grid { grid-template-columns: repeat(3, minmax(0, 1fr)); } }
  @media (max-width: 1040px) { .media-skeleton__grid { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
  @media (max-width: 620px) { .media-skeleton__grid { grid-template-columns: 1fr; } }
</style>

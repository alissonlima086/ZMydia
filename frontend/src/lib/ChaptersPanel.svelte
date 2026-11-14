<script lang="ts">
  import type { Chapter } from './chapters';
  import type { MediaItem } from './player.svelte';
  import { t } from './i18n/i18n.svelte';

  let {
    chapters,
    activeChapter,
    media,
    source,
    onSeek,
    onClose
  }: {
    chapters: Chapter[];
    activeChapter: Chapter | null;
    media: MediaItem;
    source?: 'description' | 'notes';
    onSeek: (timeSec: number) => void;
    onClose: () => void;
  } = $props();

  let listEl = $state<HTMLElement | null>(null);

  // Mantém o capítulo ativo visível na lista conforme o tempo do vídeo avança
  $effect(() => {
    const currentId = activeChapter?.id;
    if (currentId && listEl) {
      const activeEl = listEl.querySelector(`#chapter-item-${currentId}`);
      if (activeEl) {
        activeEl.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
      }
    }
  });
</script>

<div class="chapters-panel">
  <!-- Cabeçalho Neste vídeo -->
  <div class="chapters-panel-header">
    <div class="chapters-title-wrap">
      <h3>{t('chapters.title')}</h3>
      {#if source === 'notes'}
        <span class="chapters-source-pill" title={t('notes.active_chapters_hint')}>
          <span class="material-symbols-outlined icon-14">edit_note</span>
          <span>{t('notes.tab_notes')}</span>
        </span>
      {/if}
    </div>
    <button
      type="button"
      class="chapters-close-btn"
      onclick={onClose}
      title={t('chapters.close')}
      aria-label={t('chapters.close')}
    >
      <span class="material-symbols-outlined">close</span>
    </button>
  </div>

  <!-- Conteúdo: Lista de Capítulos -->
  <div bind:this={listEl} class="chapters-list-container">
    {#each chapters as ch (ch.id)}
      {@const isActive = activeChapter?.id === ch.id}
      <div
        id="chapter-item-{ch.id}"
        class="chapter-card"
        class:active={isActive}
        onclick={() => onSeek(ch.startTime)}
        role="button"
        tabindex="0"
        onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && onSeek(ch.startTime)}
      >
        <!-- Miniatura do trecho -->
        <div class="chapter-thumb-wrap">
          {#if media.thumbnail_url}
            <img src={media.thumbnail_url} alt="" class="chapter-thumb-img" loading="eager" decoding="sync" />
          {:else}
            <div class="chapter-thumb-fallback">
              <span class="material-symbols-outlined">movie</span>
            </div>
          {/if}
          {#if isActive}
            <div class="chapter-playing-overlay">
              <span class="material-symbols-outlined">play_arrow</span>
            </div>
          {/if}
        </div>

        <!-- Metadados do capítulo -->
        <div class="chapter-meta">
          <h4 class="chapter-title" title={ch.title}>{ch.title}</h4>
          <span class="chapter-timestamp">{ch.timeFormatted}</span>
        </div>
      </div>
    {/each}
  </div>
</div>

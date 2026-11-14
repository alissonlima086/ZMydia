<script lang="ts">
  import type { MediaItem } from './player.svelte';
  import { updateMediaNotes } from './api';
  import { parseNoteLine } from './chapters';
  import { t } from './i18n/i18n.svelte';
  import Button from './ui/Button.svelte';
  import Notice from './ui/Notice.svelte';

  let {
    media,
    onSeek,
    isOriginalChaptersActive,
    isNotesChaptersActive,
    hasNotesChapters,
    onNotesSaved
  }: {
    media: MediaItem;
    onSeek: (timeSec: number) => void;
    isOriginalChaptersActive: boolean;
    isNotesChaptersActive: boolean;
    hasNotesChapters: boolean;
    onNotesSaved?: (notes: string | null) => void;
  } = $props();

  let isDescriptionExpanded = $state(false);
  let activeInfoTab = $state<'description' | 'notes'>('description');
  let isEditingNotes = $state(false);
  let notesDraft = $state('');
  let isSavingNotes = $state(false);
  let notesErrorMessage = $state<string | null>(null);

  // Sincroniza estado de anotações quando a mídia muda
  $effect(() => {
    isEditingNotes = false;
    notesDraft = media.user_notes ?? '';
    notesErrorMessage = null;
    if (!media.description && media.user_notes) {
      activeInfoTab = 'notes';
    } else {
      activeInfoTab = 'description';
    }
  });

  const parsedNoteLines = $derived.by(() => {
    const text = media.user_notes;
    if (!text) return [];
    return text.split('\n').map((line) => parseNoteLine(line));
  });

  function formatPublishDate(dateStr?: string | null): string | null {
    if (!dateStr) return null;
    try {
      const parts = dateStr.slice(0, 10).split('-');
      if (parts.length === 3) {
        const year = Number(parts[0]);
        const month = Number(parts[1]);
        const day = Number(parts[2]);
        if (!isNaN(year) && !isNaN(month) && !isNaN(day)) {
          const d = new Date(year, month - 1, day);
          return d.toLocaleDateString('pt-BR', {
            day: 'numeric',
            month: 'short',
            year: 'numeric'
          });
        }
      }
    } catch {}
    return null;
  }

  async function handleSaveNotes() {
    isSavingNotes = true;
    notesErrorMessage = null;
    try {
      const clean = notesDraft.trim();
      const res = await updateMediaNotes(media.id, clean ? clean : null);
      media.user_notes = res.user_notes;
      onNotesSaved?.(res.user_notes);
      isEditingNotes = false;
    } catch {
      notesErrorMessage = t('notes.save_error');
    } finally {
      isSavingNotes = false;
    }
  }

  function handleImportDescription() {
    if (media.description) {
      notesDraft = media.description;
    }
  }

  function handleCancelEditNotes() {
    notesDraft = media.user_notes ?? '';
    isEditingNotes = false;
    notesErrorMessage = null;
  }
</script>

<div class="watch-description-box" class:expanded={isDescriptionExpanded}>
  <!-- Abas: Descrição vs Anotações -->
  <div class="info-tabs-bar">
    {#if media.description || media.published_at || media.youtube_id}
      <button
        type="button"
        class="info-tab-pill"
        class:active={activeInfoTab === 'description'}
        onclick={() => (activeInfoTab = 'description')}
      >
        <span class="material-symbols-outlined tab-icon">description</span>
        <span>{t('notes.tab_description')}</span>
      </button>
    {/if}

    <button
      type="button"
      class="info-tab-pill"
      class:active={activeInfoTab === 'notes'}
      onclick={() => (activeInfoTab = 'notes')}
    >
      <span class="material-symbols-outlined tab-icon">edit_note</span>
      <span>{t('notes.tab_notes')}</span>
      {#if media.user_notes}
        <span class="info-tab-dot" title={t('notes.tab_notes')}></span>
      {/if}
    </button>
  </div>

  <!-- Conteúdo da Aba Descrição -->
  {#if activeInfoTab === 'description'}
    <div class="description-header">
      {#if media.published_at}
        <div class="description-date-badge" title={t('player.published_at_tip')}>
          <span class="material-symbols-outlined desc-date-icon">calendar_today</span>
          <span>{t('player.published_on', { date: formatPublishDate(media.published_at) ?? '' })}</span>
        </div>
      {:else}
        <div></div>
      {/if}

      {#if media.youtube_id}
        <a
          href="https://www.youtube.com/watch?v={media.youtube_id}"
          target="_blank"
          rel="noopener noreferrer"
          class="yt-external-link"
          title={t('player.open_youtube_tip')}
        >
          <span class="material-symbols-outlined icon-14">open_in_new</span>
          <span>{t('player.watch_on_youtube')}</span>
        </a>
      {/if}
    </div>

    {#if media.description}
      <div class="description-content">
        {media.description}
      </div>
      <button
        type="button"
        class="desc-toggle-btn"
        onclick={() => (isDescriptionExpanded = !isDescriptionExpanded)}
      >
        {isDescriptionExpanded ? t('common.show_less') : t('common.show_more')}
      </button>
    {/if}

  <!-- Conteúdo da Aba Anotações -->
  {:else if activeInfoTab === 'notes'}
    <div class="notes-tab-container">
      <div class="notes-header-row">
        <div class="notes-header-left">
          {#if !isEditingNotes}
            {#if isOriginalChaptersActive && hasNotesChapters}
              <div class="notes-status-badge warning" title={t('notes.priority_hint')}>
                <span class="material-symbols-outlined">info</span>
                <span>{t('notes.priority_hint')}</span>
              </div>
            {:else if isNotesChaptersActive}
              <div class="notes-status-badge active" title={t('notes.active_chapters_hint')}>
                <span class="material-symbols-outlined">auto_awesome</span>
                <span>{t('notes.active_chapters_hint')}</span>
              </div>
            {/if}
          {:else if media.description}
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onclick={handleImportDescription}
              title={t('notes.import_description_tip')}
            >
              <span class="material-symbols-outlined">content_copy</span>
              <span>{t('notes.import_description')}</span>
            </Button>
          {/if}
        </div>

        <div class="notes-header-actions">
          {#if !isEditingNotes}
            <Button
              type="button"
              size="sm"
              onclick={() => {
                notesDraft = media.user_notes ?? '';
                isEditingNotes = true;
              }}
            >
              <span class="material-symbols-outlined">edit</span>
              <span>{media.user_notes ? t('notes.edit_notes_btn') : t('notes.add_notes_btn')}</span>
            </Button>
          {:else}
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onclick={handleCancelEditNotes}
              disabled={isSavingNotes}
            >
              {t('notes.cancel_btn')}
            </Button>
            <Button
              type="button"
              variant="primary"
              size="sm"
              onclick={handleSaveNotes}
              disabled={isSavingNotes}
            >
              <span class="material-symbols-outlined">{isSavingNotes ? 'sync' : 'check'}</span>
              <span>{isSavingNotes ? t('notes.saving') : t('notes.save_btn')}</span>
            </Button>
          {/if}
        </div>
      </div>

      {#if notesErrorMessage}
        <Notice tone="error">{notesErrorMessage}</Notice>
      {/if}

      {#if isEditingNotes}
        <div class="notes-editor-box">
          <textarea
            class="notes-textarea"
            bind:value={notesDraft}
            placeholder={t('notes.textarea_placeholder')}
            rows="8"
            disabled={isSavingNotes}
          ></textarea>
        </div>
      {:else if media.user_notes}
        <div class="notes-content-viewer">
          {#each parsedNoteLines as parts}
            <div class="note-line">
              {#if parts.length === 0}
                <br />
              {:else}
                {#each parts as part}
                  {#if part.type === 'timestamp' && part.timeSec !== undefined}
                    <button
                      type="button"
                      class="note-timestamp-btn"
                      title={t('notes.seek_to', { time: part.content })}
                      onclick={() => onSeek(part.timeSec!)}
                    >{part.content}</button>
                  {:else}
                    <span>{part.content}</span>
                  {/if}
                {/each}
              {/if}
            </div>
          {/each}
        </div>
      {:else}
        <div class="notes-empty-card">
          <span class="material-symbols-outlined empty-icon">edit_note</span>
          <p>{t('notes.empty_placeholder')}</p>
          <button
            type="button"
            class="notes-empty-add-btn"
            onclick={() => {
              notesDraft = '';
              isEditingNotes = true;
            }}
          >
            <span class="material-symbols-outlined">add</span>
            <span>{t('notes.add_notes_btn')}</span>
          </button>
        </div>
      {/if}
    </div>
  {/if}
</div>

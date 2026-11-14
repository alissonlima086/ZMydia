<script lang="ts">
  import { onMount } from 'svelte';
  import { apiFetch } from './api';
  import { t } from './i18n/i18n.svelte';
  import type { MediaItem } from './player.svelte';
  import Button from './ui/Button.svelte';
  import Notice from './ui/Notice.svelte';

  type PlaylistSummary = {
    id: string;
    title: string;
    item_count: number;
  };

  let {
    media,
    onClose
  }: {
    media: MediaItem;
    onClose: () => void;
  } = $props();

  let playlists = $state<PlaylistSummary[]>([]);
  let stagedPlaylists = $state<Record<string, boolean>>({});
  let initialPlaylists = $state<Record<string, boolean>>({});

  let loading = $state(true);
  let isSaving = $state(false);
  let newPlaylistName = $state('');
  let isCreatingPlaylist = $state(false);
  let errorMessage = $state<string | null>(null);

  onMount(() => {
    void loadMemberships();
  });

  async function loadMemberships() {
    loading = true;
    try {
      const pRes = await apiFetch('/api/playlists')
        .then((r) => (r.ok ? r.json() : []))
        .catch(() => []);

      playlists = pRes as PlaylistSummary[];

      const pInit: Record<string, boolean> = {};
      await Promise.all(
        playlists.map(async (pl) => {
          const detail = await apiFetch(`/api/playlists/${pl.id}`)
            .then((r) => (r.ok ? r.json() : null))
            .catch(() => null);
          if (detail && detail.items) {
            pInit[pl.id] = (detail.items as MediaItem[]).some((it) => it.id === media.id);
          }
        })
      );
      initialPlaylists = { ...pInit };
      stagedPlaylists = { ...pInit };

    } catch {
      // Ignora falhas pontuais de carregamento
    } finally {
      loading = false;
    }
  }

  function togglePlaylist(id: string) {
    stagedPlaylists[id] = !stagedPlaylists[id];
  }

  async function createPlaylistAndAdd() {
    const title = newPlaylistName.trim();
    if (!title || isCreatingPlaylist) return;
    isCreatingPlaylist = true;
    errorMessage = null;
    try {
      const res = await apiFetch('/api/playlists', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ title })
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const created = (await res.json()) as PlaylistSummary;
      playlists = [...playlists, created];
      stagedPlaylists[created.id] = true;
      initialPlaylists[created.id] = false;
      newPlaylistName = '';
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : t('common.error');
    } finally {
      isCreatingPlaylist = false;
    }
  }

  async function handleConfirm() {
    isSaving = true;
    errorMessage = null;
    try {
      // Processa alterações de playlists
      const playlistPromises: Promise<Response>[] = [];
      for (const pl of playlists) {
        const staged = !!stagedPlaylists[pl.id];
        const initial = !!initialPlaylists[pl.id];
        if (staged !== initial) {
          if (staged) {
            playlistPromises.push(
              apiFetch(`/api/playlists/${pl.id}/items`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ media_id: media.id })
              })
            );
          } else {
            playlistPromises.push(
              apiFetch(`/api/playlists/${pl.id}/items/${media.id}`, {
                method: 'DELETE'
              })
            );
          }
        }
      }

      const responses = await Promise.all(playlistPromises);
      const failed = responses.find((response) => !response.ok);
      if (failed) throw new Error(`HTTP ${failed.status}`);
      onClose();
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : t('common.error');
    } finally {
      isSaving = false;
    }
  }

  function handleWindowKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape' && !isSaving) onClose();
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="save-modal-backdrop"
  onclick={onClose}
  role="presentation"
  tabindex="-1"
>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="save-modal-card" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-labelledby="save-modal-title" tabindex="-1">
    <!-- Cabeçalho com miniatura do vídeo e título -->
    <div class="save-modal-header">
      <div class="save-modal-video-preview">
        {#if media.thumbnail_url}
          <img src={media.thumbnail_url} alt="" class="save-modal-thumb" />
        {:else}
          <div class="save-modal-thumb-fallback">
            <span class="material-symbols-outlined">movie</span>
          </div>
        {/if}
        <div class="save-modal-title-stack">
          <h3 id="save-modal-title">{t('save_modal.title')}</h3>
          <span class="save-modal-subtitle" title={media.title}>{media.title}</span>
        </div>
      </div>
      <button type="button" class="close-icon-btn" onclick={onClose} aria-label={t('save_modal.close')}>
        <span class="material-symbols-outlined">close</span>
      </button>
    </div>

    <div class="save-modal-content">
      {#if loading}
        <div class="save-loading">
          <div class="spinner-small"></div>
          <span>{t('common.loading')}</span>
        </div>
      {:else}
        {#if errorMessage}<Notice tone="error">{errorMessage}</Notice>{/if}
        <!-- Seção: Playlists -->
        <div class="save-section">
          <div class="save-section-header">
            <span class="material-symbols-outlined section-icon">queue_music</span>
            <h4>{t('nav.playlists')}</h4>
            <span class="section-count-badge">{playlists.length}</span>
          </div>

          {#if playlists.length === 0}
            <div class="save-empty-hint-box">
              <span class="material-symbols-outlined icon-20">playlist_add</span>
              <span>{t('playlists_view.empty_title')}</span>
            </div>
          {:else}
            <div class="save-list">
              {#each playlists as pl (pl.id)}
                {@const isChecked = !!stagedPlaylists[pl.id]}
                <div
                  class="save-item-row"
                  class:checked={isChecked}
                  onclick={() => togglePlaylist(pl.id)}
                  role="checkbox"
                  aria-checked={isChecked}
                  tabindex="0"
                  onkeydown={(e) => e.key === ' ' || e.key === 'Enter' ? togglePlaylist(pl.id) : null}
                >
                  <div class="custom-checkbox" class:checked={isChecked}>
                    {#if isChecked}
                      <span class="material-symbols-outlined check-icon">check</span>
                    {/if}
                  </div>
                  <span class="material-symbols-outlined item-type-icon">queue_music</span>
                  <span class="save-item-title">{pl.title}</span>
                  {#if pl.item_count !== undefined}
                    <span class="item-count-chip">{pl.item_count}</span>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}

          <!-- Criar nova playlist -->
          <form
            class="quick-create-row"
            onsubmit={(e) => {
              e.preventDefault();
              void createPlaylistAndAdd();
            }}
          >
            <div class="quick-input-wrap">
              <span class="material-symbols-outlined quick-input-icon">add</span>
              <input
                type="text"
                class="quick-input"
                placeholder={t('save_modal.new_playlist_placeholder')}
                bind:value={newPlaylistName}
              />
            </div>
            <Button
              type="submit"
              size="sm"
              disabled={!newPlaylistName.trim() || isCreatingPlaylist}
            >
              {t('common.create')}
            </Button>
          </form>
        </div>

      {/if}
    </div>

    <!-- Rodapé de Ações com Cancelar e Confirmar -->
    <div class="save-modal-footer">
      <Button type="button" variant="ghost" onclick={onClose} disabled={isSaving}>
        {t('common.cancel')}
      </Button>
      <Button
        type="button"
        variant="primary"
        onclick={() => void handleConfirm()}
        disabled={isSaving}
      >
        {#if isSaving}
          <div class="spinner-small"></div>
          <span>{t('common.loading')}</span>
        {:else}
          <span class="material-symbols-outlined icon-18">check</span>
          <span>{t('save_modal.save_btn')}</span>
        {/if}
      </Button>
    </div>
  </div>
</div>

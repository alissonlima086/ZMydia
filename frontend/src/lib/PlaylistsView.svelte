<script lang="ts">
  import { onMount } from 'svelte';
  import { apiFetch } from './api';
  import { player, type MediaItem } from './player.svelte';
  import { t } from './i18n/i18n.svelte';
  import Button from './ui/Button.svelte';
  import ConfirmDialog from './ui/ConfirmDialog.svelte';
  import PageHeader from './ui/PageHeader.svelte';
  import PageState from './ui/PageState.svelte';

  type PlaylistSummary = {
    id: string;
    title: string;
    description: string | null;
    media_count: number;
    created_at: string;
    updated_at: string;
    thumbnail_media_id: number | null;
  };

  type PlaylistDetail = {
    id: string;
    title: string;
    description: string | null;
    created_at: string;
    updated_at: string;
    items: MediaItem[];
  };

  let playlists = $state<PlaylistSummary[]>([]);
  let selectedPlaylist = $state<PlaylistDetail | null>(null);
  let loading = $state(true);
  let errorMessage = $state<string | null>(null);
  let isCreating = $state(false);
  let newTitle = $state('');
  let newDescription = $state('');
  let confirmDeleteOpen = $state(false);
  let deleting = $state(false);

  function extractPlaylistIdFromUrl(): string | null {
    if (typeof window === 'undefined') return null;
    const match = window.location.pathname.match(/^\/(?:playlists|playlist)\/([a-zA-Z0-9_-]+)/);
    if (match) return match[1];
    const q = new URLSearchParams(window.location.search).get('list') || new URLSearchParams(window.location.search).get('id');
    return q || null;
  }

  onMount(() => {
    void syncFromUrl();
  });

  async function syncFromUrl() {
    const routeId = extractPlaylistIdFromUrl();
    if (routeId) {
      await openPlaylist(routeId, false);
    } else {
      selectedPlaylist = null;
      await loadPlaylists();
    }
  }

  async function loadPlaylists() {
    loading = true;
    errorMessage = null;
    try {
      const res = await apiFetch('/api/playlists');
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      playlists = (await res.json()) as PlaylistSummary[];
    } catch (err) {
      errorMessage = err instanceof Error ? err.message : String(err);
    } finally {
      loading = false;
    }
  }

  async function openPlaylist(id: string, updateUrl = true) {
    loading = true;
    errorMessage = null;
    try {
      const res = await apiFetch(`/api/playlists/${id}`);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      selectedPlaylist = (await res.json()) as PlaylistDetail;
      if (updateUrl) {
        const targetUrl = `/playlists/${id}`;
        if (window.location.pathname !== targetUrl) {
          history.pushState({ playlistId: id }, '', targetUrl);
          window.dispatchEvent(new Event('routechange'));
        }
      }
    } catch (err) {
      errorMessage = err instanceof Error ? err.message : String(err);
    } finally {
      loading = false;
    }
  }

  function backToList() {
    selectedPlaylist = null;
    if (window.location.pathname !== '/playlists') {
      history.pushState({}, '', '/playlists');
      window.dispatchEvent(new Event('routechange'));
    }
    void loadPlaylists();
  }

  async function createPlaylistSubmit(event: Event) {
    event.preventDefault();
    if (!newTitle.trim()) return;

    try {
      const res = await apiFetch('/api/playlists', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          title: newTitle.trim(),
          description: newDescription.trim() || null
        })
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const created = (await res.json()) as PlaylistSummary;
      newTitle = '';
      newDescription = '';
      isCreating = false;
      await openPlaylist(created.id);
    } catch (err) {
      errorMessage = err instanceof Error ? err.message : String(err);
    }
  }

  async function deleteCurrentPlaylist() {
    if (!selectedPlaylist) return;
    confirmDeleteOpen = true;
  }

  async function confirmDeletePlaylist() {
    if (!selectedPlaylist || deleting) return;
    deleting = true;
    try {
      const res = await apiFetch(`/api/playlists/${selectedPlaylist.id}`, { method: 'DELETE' });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      confirmDeleteOpen = false;
      backToList();
    } catch (err) {
      errorMessage = err instanceof Error ? err.message : String(err);
    } finally {
      deleting = false;
    }
  }

  async function removeItem(mediaId: number) {
    if (!selectedPlaylist) return;
    try {
      const res = await apiFetch(`/api/playlists/${selectedPlaylist.id}/items/${mediaId}`, {
        method: 'DELETE'
      });
      if (res.ok) {
        selectedPlaylist.items = selectedPlaylist.items.filter((item) => item.id !== mediaId);
      }
    } catch (err) {
      errorMessage = err instanceof Error ? err.message : String(err);
    }
  }

  function playAll() {
    if (!selectedPlaylist || !selectedPlaylist.items.length) return;
    void player.playPlaylist({
      id: selectedPlaylist.id,
      title: selectedPlaylist.title,
      author: 'Playlist',
      items: selectedPlaylist.items
    }, 0);
  }

  function playItem(index: number) {
    if (!selectedPlaylist || !selectedPlaylist.items.length) return;
    void player.playPlaylist({
      id: selectedPlaylist.id,
      title: selectedPlaylist.title,
      author: 'Playlist',
      items: selectedPlaylist.items
    }, index);
  }

  function formatDuration(ms: number | null): string {
    if (!ms || ms <= 0) return '';
    const totalSecs = Math.floor(ms / 1000);
    const hours = Math.floor(totalSecs / 3600);
    const minutes = Math.floor((totalSecs % 3600) / 60);
    const seconds = totalSecs % 60;
    const pad = (n: number) => n.toString().padStart(2, '0');
    if (hours > 0) return `${hours}:${pad(minutes)}:${pad(seconds)}`;
    return `${minutes}:${pad(seconds)}`;
  }
</script>

<svelte:window onpopstate={syncFromUrl} />

<div class="playlists-container">
  {#if selectedPlaylist}
    {@const playlist = selectedPlaylist}
    <Button variant="ghost" size="sm" onclick={backToList}><span class="material-symbols-outlined icon-18" aria-hidden="true">arrow_back</span>{t('common.back')}</Button>
    <PageHeader title={playlist.title} description={playlist.description ?? undefined} eyebrow={t('playlists_view.title')}>
      {#snippet meta()}<span>{t('common.video_count', { count: playlist.items.length })}</span>{/snippet}
      {#snippet actions()}<Button variant="primary" disabled={playlist.items.length === 0} onclick={playAll}><span class="material-symbols-outlined icon-18" aria-hidden="true">play_arrow</span>{t('playlists_view.play_all')}</Button><Button variant="danger" onclick={() => void deleteCurrentPlaylist()}>{t('playlists_view.delete_playlist')}</Button>{/snippet}
    </PageHeader>

    {#if errorMessage}
      <PageState kind="error" icon="error" title={t('playlists_view.title')} description={errorMessage} />
    {:else if selectedPlaylist.items.length === 0}
      <PageState kind="empty" icon="playlist_add" title={t('playlists_view.empty_title')} description={t('playlists_view.empty_desc')} />
    {:else}
      <div class="playlist-items-list">
        {#each selectedPlaylist.items as item, index (item.id)}
          <div class="playlist-row-item">
            <span class="row-index">{index + 1}</span>
            <button
              type="button"
              class="row-thumb-btn"
              onclick={() => playItem(index)}
            >
              <img src={item.thumbnail_url} alt={item.title} class="row-thumb-img" loading={index < 8 ? 'eager' : 'lazy'} decoding={index < 6 ? 'sync' : 'async'} fetchpriority={index < 3 ? 'high' : 'auto'} />
              {#if item.duration_ms}
                <span class="row-duration">{formatDuration(item.duration_ms)}</span>
              {/if}
            </button>
            <div
              class="row-info"
              role="button"
              tabindex="0"
              onclick={() => playItem(index)}
              onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && playItem(index)}
            >
              <h4 class="row-title" title={item.title}>{item.title}</h4>
              {#if item.channel}
                <span class="row-channel">
                  <span class="material-symbols-outlined icon-13 icon-inline icon-gap-after">subscriptions</span>
                  {item.channel.name}
                </span>
              {/if}
            </div>
            <button
              type="button"
              class="remove-item-btn"
              onclick={() => void removeItem(item.id)}
              title={t('common.remove')}
            >
              <span class="material-symbols-outlined icon-16">close</span>
            </button>
          </div>
        {/each}
      </div>
    {/if}
  {:else}
    <PageHeader title={t('playlists_view.title')} description={t('playlists_view.empty_desc')}>
      {#snippet meta()}<span>{t('playlists_view.count', { count: playlists.length })}</span>{/snippet}
      {#snippet actions()}<Button variant={isCreating ? 'ghost' : 'primary'} onclick={() => (isCreating = !isCreating)}>{isCreating ? t('common.cancel') : t('playlists_view.create_btn')}</Button>{/snippet}
    </PageHeader>

    {#if isCreating}
      <form class="create-playlist-form" onsubmit={createPlaylistSubmit}>
        <h3>{t('playlists_view.modal_title')}</h3>
        <input
          type="text"
          class="text-input"
          placeholder={t('playlists_view.name_placeholder')}
          bind:value={newTitle}
          required
        />
        <textarea
          class="text-input textarea"
          placeholder={t('common.optional_desc')}
          bind:value={newDescription}
          rows="2"
        ></textarea>
        <div class="form-actions">
          <Button type="submit" variant="primary">{t('common.create')}</Button>
          <Button type="button" variant="ghost" onclick={() => (isCreating = false)}>{t('common.cancel')}</Button>
        </div>
      </form>
    {/if}

    {#if loading}
      <PageState kind="loading" title={t('common.loading')} />
    {:else if errorMessage}
      {#snippet retry()}<Button onclick={() => void loadPlaylists()}>{t('common.retry')}</Button>{/snippet}
      <PageState kind="error" icon="error" title={t('playlists_view.title')} description={errorMessage} actions={retry} />
    {:else if playlists.length === 0}
      <PageState kind="empty" icon="playlist_add" title={t('playlists_view.empty_title')} description={t('playlists_view.empty_desc')} />
    {:else}
      <div class="playlists-grid">
        {#each playlists as pl, index (pl.id)}
          <div
            class="playlist-card"
            role="button"
            tabindex="0"
            onclick={() => void openPlaylist(pl.id)}
            onkeydown={(e) => e.key === 'Enter' && void openPlaylist(pl.id)}
          >
            <div class="playlist-card-cover">
              {#if pl.thumbnail_media_id}
                <img
                  src={`/api/media/${pl.thumbnail_media_id}/thumbnail`}
                  alt={pl.title}
                  class="playlist-cover-img"
                  loading={index < 8 ? 'eager' : 'lazy'}
                  decoding={index < 6 ? 'sync' : 'async'}
                  fetchpriority={index < 3 ? 'high' : 'auto'}
                />
              {:else}
                <div class="playlist-empty-cover">
                  <span class="material-symbols-outlined icon-32">playlist_play</span>
                </div>
              {/if}
              <span class="playlist-badge">{t('common.video_count', { count: pl.media_count })}</span>
            </div>
            <div class="playlist-card-body">
              <h3 class="playlist-card-title">{pl.title}</h3>
              {#if pl.description}
                <p class="playlist-card-desc">{pl.description}</p>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<ConfirmDialog
  open={confirmDeleteOpen}
  title={t('playlists_view.delete_playlist')}
  description={selectedPlaylist ? t('playlists_view.delete_confirm', { name: selectedPlaylist.title }) : ''}
  confirmLabel={t('common.remove')}
  cancelLabel={t('common.cancel')}
  dangerous
  busy={deleting}
  onConfirm={() => void confirmDeletePlaylist()}
  onCancel={() => (confirmDeleteOpen = false)}
/>

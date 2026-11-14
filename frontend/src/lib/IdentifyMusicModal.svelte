<script lang="ts">
  import { t } from './i18n/i18n.svelte';
  import { musicStore, type MediaMusicTrack, type MusicCandidate } from './music.svelte';
  import type { MediaItem } from './player.svelte';
  import ConfirmDialog from './ui/ConfirmDialog.svelte';
  import Notice from './ui/Notice.svelte';
  import MusicTrackStatus from './music/MusicTrackStatus.svelte';

  interface Props {
    media: MediaItem;
    isOpen: boolean;
    onClose: () => void;
    onTrackUpdated?: (track: MediaMusicTrack | null) => void;
  }

  let { media, isOpen, onClose, onTrackUpdated }: Props = $props();

  // Active view tab inside modal: 'search' | 'id'
  let activeTab = $state<'search' | 'id'>('search');

  // Text search state
  let searchQuery = $state('');
  let isSearchingText = $state(false);

  // Search candidates state
  let candidates = $state<MusicCandidate[]>([]);
  let candidateError = $state<string | null>(null);
  let hasSearched = $state(false);

  // MBID / URL lookup state
  let manualIdInput = $state('');
  let lookingUpId = $state(false);
  let manualCandidate = $state<MusicCandidate | null>(null);
  let idLookupError = $state<string | null>(null);

  // Current track status
  let currentTrack = $state<MediaMusicTrack | null>(null);

  // Action states
  let actionLoading = $state(false);
  let feedback = $state<{ type: 'success' | 'error'; text: string } | null>(null);
  let confirmUnlinkOpen = $state(false);

  function showMessage(type: 'success' | 'error', text: string) {
    feedback = { type, text };
  }

  $effect(() => {
    if (isOpen && media) {
      void initModal();
    }
  });

  async function initModal() {
    feedback = null;
    candidateError = null;
    idLookupError = null;
    manualCandidate = null;
    candidates = [];
    hasSearched = false;
    isSearchingText = false;
    activeTab = 'search';

    // Pre-fill query with cleaned video title (without channel name or noise)
    searchQuery = cleanInitialTitle(media.title, media.channel?.name);

    try {
      currentTrack = await musicStore.loadTrackForMedia(media.id);
    } catch {
      currentTrack = null;
    }
  }

  function cleanInitialTitle(raw: string, channelName?: string): string {
    let clean = raw
      .replace(/\.(mp4|mkv|webm|avi|mov)$/i, '')
      .replace(/\[[a-zA-Z0-9_-]{11}\]/g, '')
      .replace(/[([][a-zA-Z0-9\s]*official[a-zA-Z0-9\s]*[)\]]/gi, '')
      .replace(/[([][a-zA-Z0-9\s]*music\s*video[a-zA-Z0-9\s]*[)\]]/gi, '')
      .replace(/[([][a-zA-Z0-9\s]*clipe[a-zA-Z0-9\s]*[)\]]/gi, '')
      .replace(/[([][a-zA-Z0-9\s]*video[a-zA-Z0-9\s]*[)\]]/gi, '')
      .replace(/[([][a-zA-Z0-9\s]*audio[a-zA-Z0-9\s]*[)\]]/gi, '')
      .replace(/[([][a-zA-Z0-9\s]*lyrics[a-zA-Z0-9\s]*[)\]]/gi, '')
      .replace(/[([][a-zA-Z0-9\s]*letra[a-zA-Z0-9\s]*[)\]]/gi, '')
      .replace(/[([][a-zA-Z0-9\s]*hd[)\]]/gi, '')
      .replace(/[([][a-zA-Z0-9\s]*4k[)\]]/gi, '')
      .replace(/[([][a-zA-Z0-9\s]*mv[)\]]/gi, '');

    // Remove channel name if present (sem considerar nome do canal, só o nome do vídeo)
    if (channelName && channelName.trim()) {
      const ch = channelName.trim();
      const escapedCh = ch.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
      const chRegexEnd = new RegExp(`[\\s|｜\\-–—:]+${escapedCh}\\s*$`, 'i');
      clean = clean.replace(chRegexEnd, '');
      const chRegexStart = new RegExp(`^\\s*${escapedCh}[\\s|｜\\-–—:]+`, 'i');
      clean = clean.replace(chRegexStart, '');
      const chStandaloneEnd = new RegExp(`\\b${escapedCh}\\s*$`, 'i');
      clean = clean.replace(chStandaloneEnd, '');
    }

    clean = clean.replace(/[\s|｜\-–—:]+$/, '').replace(/^[\s|｜\-–—:]+/, '');
    return clean.replace(/\s+/g, ' ').trim();
  }

  async function handleTextSearch(event?: Event) {
    if (event) event.preventDefault();
    const clean = searchQuery.trim();
    if (!clean) return;

    isSearchingText = true;
    candidateError = null;
    hasSearched = true;
    try {
      const results = await musicStore.searchMusic(clean);
      candidates = results;
      if (results.length === 0) {
        candidateError = `Nenhum resultado encontrado no MusicBrainz para "${clean}".`;
      }
    } catch (e: any) {
      candidateError = e.message || 'Erro ao realizar pesquisa';
    } finally {
      isSearchingText = false;
    }
  }

  async function handleSelectCandidate(cand: MusicCandidate) {
    actionLoading = true;
    try {
      const saved = await musicStore.associateMusic(media.id, {
        recording_mbid: cand.recording_mbid,
        artist_name: cand.artist_name,
        track_name: cand.track_name,
        release_name: cand.release_name,
        source: cand.source
      });
      currentTrack = saved;
      onTrackUpdated?.(saved);
      showMessage('success', `Faixa "${saved.track_name}" vinculada com sucesso!`);
    } catch (e: any) {
      showMessage('error', e.message || 'Erro ao vincular música');
    } finally {
      actionLoading = false;
    }
  }

  async function handleLookupId() {
    const clean = manualIdInput.trim();
    if (!clean) return;

    lookingUpId = true;
    idLookupError = null;
    manualCandidate = null;

    try {
      const found = await musicStore.lookupMbid(clean);
      if (found) {
        manualCandidate = found;
      } else {
        idLookupError = 'Nenhuma gravação encontrada com este ID ou link.';
      }
    } catch (e: any) {
      idLookupError = e.message || 'Erro ao consultar ID';
    } finally {
      lookingUpId = false;
    }
  }

  async function handleConfirmIdMatch() {
    if (!manualCandidate) return;
    await handleSelectCandidate(manualCandidate);
    manualCandidate = null;
    manualIdInput = '';
  }

  async function handleRemoveTrack() {
    confirmUnlinkOpen = true;
  }

  async function confirmRemoveTrack() {
    if (actionLoading) return;
    confirmUnlinkOpen = false;
    actionLoading = true;
    try {
      await musicStore.deleteMusic(media.id);
      currentTrack = null;
      onTrackUpdated?.(null);
      showMessage('success', 'Música desvinculada com sucesso.');
    } catch (e: any) {
      showMessage('error', e.message || 'Erro ao desvincular música');
    } finally {
      actionLoading = false;
    }
  }

  function handleWindowKeydown(event: KeyboardEvent) {
    if (isOpen && event.key === 'Escape' && !actionLoading && !confirmUnlinkOpen) onClose();
  }

  async function handleManualScrobble() {
    actionLoading = true;
    try {
      const res = await musicStore.scrobble(media.id);
      if (currentTrack) {
        currentTrack.last_scrobbled_at = res.scrobbled_at;
      }
      showMessage('success', 'Registro (scrobble) enviado para o ListenBrainz com sucesso!');
    } catch (e: any) {
      showMessage('error', e.message || 'Falha ao enviar scrobble');
    } finally {
      actionLoading = false;
    }
  }

  function formatDuration(secs?: number | null) {
    if (!secs) return '';
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return `${m}:${s < 10 ? '0' : ''}${s}`;
  }

  function formatReleaseYear(dateStr?: string | null): string {
    if (!dateStr) return '';
    const trimmed = dateStr.trim();
    if (trimmed.length >= 4) {
      return trimmed.slice(0, 4);
    }
    return trimmed;
  }

  function isCloseDuration(candSecs?: number | null, videoMs?: number | null): boolean {
    if (!candSecs || !videoMs) return false;
    const videoSecs = Math.round(videoMs / 1000);
    return Math.abs(candSecs - videoSecs) <= 8;
  }

</script>

<svelte:window onkeydown={handleWindowKeydown} />

{#if isOpen}
  <div class="modal-backdrop" onclick={onClose} role="presentation">
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      class="modal-dialog"
      onclick={(e) => e.stopPropagation()}
      role="dialog"
      aria-modal="true"
      tabindex="-1"
    >
      <div class="modal-header">
        <div class="header-titles">
          <div class="header-icon-title">
            <span class="material-symbols-outlined header-music-icon">music_note</span>
            <h2 class="modal-title">{t('music.modal_title')}</h2>
          </div>
          <p class="modal-subtitle" title={media.title}>{media.title}</p>
        </div>
        <button type="button" class="close-btn" onclick={onClose} aria-label={t('common.close')}>
          <span class="material-symbols-outlined">close</span>
        </button>
      </div>

      <div class="modal-body">
        {#if feedback}
          <Notice tone={feedback.type}>{feedback.text}</Notice>
        {/if}

        <!-- Seção: Música já vinculada atualmente -->
        {#if currentTrack}
          <MusicTrackStatus track={currentTrack} busy={actionLoading} onScrobble={handleManualScrobble} onUnlink={handleRemoveTrack} />
        {:else}
          <!-- Abas de navegação: Busca no MusicBrainz e Por ID / Link -->
          <div class="modal-tabs">
          <button
            type="button"
            class="tab-btn"
            class:active={activeTab === 'search'}
            onclick={() => (activeTab = 'search')}
          >
            <span class="material-symbols-outlined">search</span>
            <span>{t('music.tab_search')}</span>
          </button>
          <button
            type="button"
            class="tab-btn"
            class:active={activeTab === 'id'}
            onclick={() => (activeTab = 'id')}
          >
            <span class="material-symbols-outlined">fingerprint</span>
            <span>{t('music.tab_id')}</span>
          </button>
        </div>

        <!-- ABA 1: BUSCA POR TÍTULO NO MUSICBRAINZ -->
        {#if activeTab === 'search'}
          <div class="tab-panel">
            <!-- Barra de Pesquisa por Título -->
            <form class="music-search-form" onsubmit={handleTextSearch}>
              <div class="music-search-input-wrap">
                <span class="material-symbols-outlined search-leading-icon">search</span>
                <input
                  type="text"
                  bind:value={searchQuery}
                  placeholder={t('music.search_placeholder')}
                  class="music-search-input"
                  disabled={isSearchingText}
                />
                {#if searchQuery}
                  <button
                    type="button"
                    class="music-search-clear-btn"
                    onclick={() => { searchQuery = ''; }}
                    title={t('common.clear')}
                    aria-label={t('common.clear')}
                  >
                    <span class="material-symbols-outlined">close</span>
                  </button>
                {/if}
              </div>

              <button
                type="submit"
                class="music-search-submit-btn"
                disabled={isSearchingText || !searchQuery.trim()}
                title={t('music.search_button')}
              >
                {#if isSearchingText}
                  <span class="material-symbols-outlined spin">progress_activity</span>
                  <span>{t('music.searching_musicbrainz')}</span>
                {:else}
                  <span class="material-symbols-outlined">search</span>
                  <span>{t('music.search_button')}</span>
                {/if}
              </button>
            </form>

            <!-- Área de Resultados / Estados -->
            <div class="section-container">
              {#if isSearchingText}
                <div class="loading-state">
                  <span class="material-symbols-outlined spin big-spinner">progress_activity</span>
                  <p>{t('music.searching_musicbrainz')}</p>
                </div>
              {:else if candidateError}
                <div class="candidate-error-box">
                  <span class="material-symbols-outlined">info</span>
                  <span>{candidateError}</span>
                </div>
              {:else if candidates.length > 0}
                <div class="candidates-list-header">
                  <span class="candidates-heading">{t('music.candidates_title')}</span>
                  <span class="results-badge">
                    {candidates.length} {candidates.length === 1 ? 'faixa encontrada' : 'faixas encontradas'}
                  </span>
                </div>
                <div class="candidates-list">
                  {#each candidates as cand (cand.recording_mbid)}
                    <div class="candidate-row">
                      <div class="candidate-details">
                        <div class="candidate-title-line">
                          <span class="cand-name">{cand.track_name}</span>
                          {#if cand.disambiguation}
                            <span class="cand-disambiguation-badge" title="Versão ou nota da gravação no MusicBrainz">
                              {cand.disambiguation}
                            </span>
                          {/if}
                          {#if cand.recording_mbid}
                            <a
                              href="https://listenbrainz.org/track/{cand.recording_mbid}"
                              target="_blank"
                              rel="noopener noreferrer"
                              class="candidate-lb-link"
                              title={t('music.view_on_listenbrainz')}
                              aria-label={t('music.view_on_listenbrainz')}
                              onclick={(e) => e.stopPropagation()}
                            >
                              <span>ListenBrainz</span>
                              <span class="material-symbols-outlined external-link-icon">open_in_new</span>
                            </a>
                          {/if}
                        </div>
                        <span class="cand-artist">{cand.artist_name}</span>
                        <div class="cand-meta">
                          {#if cand.release_name}
                            <span class="cand-release" title="{cand.release_type ? `${cand.release_type}: ` : ''}{cand.release_name}">
                              {#if cand.release_type}
                                <span class="cand-release-type">{cand.release_type}:</span>
                              {/if}
                              {cand.release_name}
                            </span>
                          {/if}
                          {#if cand.release_date}
                            <span class="cand-release-date">• {formatReleaseYear(cand.release_date)}</span>
                          {/if}
                          {#if cand.duration_secs}
                            <span class="cand-dur">• {formatDuration(cand.duration_secs)}</span>
                          {/if}
                          {#if isCloseDuration(cand.duration_secs, media.duration_ms)}
                            <span
                              class="cand-match-badge"
                              title="Duração aproxima-se da duração deste clipe ({formatDuration(Math.round((media.duration_ms || 0) / 1000))})"
                            >
                              <span class="material-symbols-outlined match-icon">check</span>
                              compatível com vídeo
                            </span>
                          {/if}
                          {#if cand.recording_mbid}
                            <span class="cand-id-tag" title="MusicBrainz Recording MBID: {cand.recording_mbid}">
                              MBID: {cand.recording_mbid.slice(0, 8)}…
                            </span>
                          {/if}
                        </div>
                      </div>
                      <button
                        type="button"
                        class="select-candidate-btn"
                        disabled={actionLoading}
                        onclick={() => handleSelectCandidate(cand)}
                      >
                        <span class="material-symbols-outlined">done</span>
                        <span>{t('music.select')}</span>
                      </button>
                    </div>
                  {/each}
                </div>
              {:else if !hasSearched}
                <!-- Estado Inicial: Pré-visualização da busca pronta -->
                <div class="initial-prompt-box">
                  <span class="material-symbols-outlined initial-prompt-icon">music_note</span>
                  <h4 class="initial-prompt-title">{t('music.prompt_title')}</h4>
                  <p class="initial-prompt-desc">{t('music.prompt_desc')}</p>
                  <div class="initial-prompt-actions">
                    <button
                      type="button"
                      class="prompt-action-btn primary"
                      disabled={!searchQuery.trim()}
                      onclick={() => handleTextSearch()}
                    >
                      <span class="material-symbols-outlined">search</span>
                      <span>{t('music.search_with_title', { query: searchQuery || '...' })}</span>
                    </button>
                  </div>
                </div>
              {:else}
                <!-- Sem resultados após busca -->
                <div class="empty-candidates">
                  <span class="material-symbols-outlined">search_off</span>
                  <p>{t('music.no_candidates_found')}</p>
                  <p class="empty-hint">{t('music.no_candidates_hint')}</p>
                </div>
              {/if}
            </div>
          </div>

        <!-- ABA 2: POR ID / LINK -->
        {:else if activeTab === 'id'}
          <div class="tab-panel">
            <div class="section-container">
              <h3 class="section-heading">
                <span class="material-symbols-outlined">fingerprint</span>
                <span>{t('music.manual_id_title')}</span>
              </h3>
              <p class="section-desc">{t('music.manual_id_desc')}</p>

              <div class="manual-input-row">
                <input
                  type="text"
                  bind:value={manualIdInput}
                  placeholder="Ex: e8a93e3d-0453-4b68-8094-0f1118fbca79 ou link do ListenBrainz"
                  class="manual-input"
                  onkeydown={(e) => e.key === 'Enter' && handleLookupId()}
                />
                <button
                  type="button"
                  class="manual-search-btn"
                  disabled={lookingUpId || !manualIdInput.trim()}
                  onclick={handleLookupId}
                >
                  {#if lookingUpId}
                    <span class="material-symbols-outlined spin">progress_activity</span>
                    <span>{t('common.loading')}</span>
                  {:else}
                    <span class="material-symbols-outlined">search</span>
                    <span>{t('common.search')}</span>
                  {/if}
                </button>
              </div>

              {#if idLookupError}
                <div class="manual-error-box">
                  <span class="material-symbols-outlined">error</span>
                  <span>{idLookupError}</span>
                </div>
              {/if}

              {#if manualCandidate}
                <div class="manual-preview-card">
                  <div class="manual-preview-details">
                    <div class="candidate-title-line">
                      <span class="preview-title">{manualCandidate.track_name}</span>
                      {#if manualCandidate.disambiguation}
                        <span class="cand-disambiguation-badge" title="Versão ou nota da gravação no MusicBrainz">
                          {manualCandidate.disambiguation}
                        </span>
                      {/if}
                      {#if manualCandidate.recording_mbid}
                        <a
                          href="https://listenbrainz.org/track/{manualCandidate.recording_mbid}"
                          target="_blank"
                          rel="noopener noreferrer"
                          class="candidate-lb-link"
                          title={t('music.view_on_listenbrainz')}
                          aria-label={t('music.view_on_listenbrainz')}
                        >
                          <span>ListenBrainz</span>
                          <span class="material-symbols-outlined external-link-icon">open_in_new</span>
                        </a>
                      {/if}
                    </div>
                    <span class="preview-artist">{manualCandidate.artist_name}</span>
                    <div class="cand-meta">
                      {#if manualCandidate.release_name}
                        <span class="preview-release" title="{manualCandidate.release_type ? `${manualCandidate.release_type}: ` : ''}{manualCandidate.release_name}">
                          {#if manualCandidate.release_type}
                            <span class="cand-release-type">{manualCandidate.release_type}:</span>
                          {/if}
                          {manualCandidate.release_name}
                        </span>
                      {/if}
                      {#if manualCandidate.release_date}
                        <span class="cand-release-date">• {formatReleaseYear(manualCandidate.release_date)}</span>
                      {/if}
                      {#if manualCandidate.duration_secs}
                        <span class="cand-dur">• {formatDuration(manualCandidate.duration_secs)}</span>
                      {/if}
                      {#if isCloseDuration(manualCandidate.duration_secs, media.duration_ms)}
                        <span
                          class="cand-match-badge"
                          title="Duração aproxima-se da duração deste clipe ({formatDuration(Math.round((media.duration_ms || 0) / 1000))})"
                        >
                          <span class="material-symbols-outlined match-icon">check</span>
                          compatível com vídeo
                        </span>
                      {/if}
                    </div>
                    <span class="preview-mbid">MBID: {manualCandidate.recording_mbid}</span>
                  </div>
                  <button
                    type="button"
                    class="confirm-manual-btn"
                    disabled={actionLoading}
                    onclick={handleConfirmIdMatch}
                  >
                    <span class="material-symbols-outlined">check</span>
                    <span>{t('music.confirm_and_link')}</span>
                  </button>
                </div>
              {/if}
            </div>
          </div>
        {/if}
      {/if}
      </div>
    </div>
  </div>
{/if}

<ConfirmDialog
  open={confirmUnlinkOpen}
  title={t('music.unlink')}
  description={t('music.unlink_confirm')}
  confirmLabel={t('music.unlink')}
  cancelLabel={t('common.cancel')}
  dangerous
  busy={actionLoading}
  onConfirm={() => void confirmRemoveTrack()}
  onCancel={() => (confirmUnlinkOpen = false)}
/>

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 9999;
    padding: 16px;
    box-sizing: border-box;
  }

  .modal-dialog {
    background: var(--bg-surface, #1e1e24);
    border: 1px solid var(--border-color, rgba(255, 255, 255, 0.1));
    border-radius: 16px;
    width: 100%;
    max-width: 620px;
    max-height: 90vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 20px 40px rgba(0, 0, 0, 0.5);
    overflow: hidden;
    box-sizing: border-box;
  }

  .modal-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    padding: 18px 22px;
    border-bottom: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
    flex-shrink: 0;
  }

  .header-titles {
    display: flex;
    flex-direction: column;
    gap: 4px;
    overflow: hidden;
  }

  .header-icon-title {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .header-music-icon {
    color: var(--accent);
    font-size: 22px;
  }

  .modal-title {
    font-size: 18px;
    font-weight: 600;
    margin: 0;
    color: var(--text-color, #fff);
  }

  .modal-subtitle {
    font-size: 13px;
    color: var(--text-muted, #999);
    margin: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 500px;
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--text-muted, #888);
    cursor: pointer;
    padding: 4px;
    border-radius: 6px;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: background 0.15s ease, color 0.15s ease;
  }

  .close-btn:hover {
    color: #fff;
    background: rgba(255, 255, 255, 0.1);
  }

  .modal-body {
    padding: 18px 22px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 16px;
    box-sizing: border-box;
  }

  /* Modal Tabs */
  .modal-tabs {
    display: flex;
    gap: 8px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.08);
    padding-bottom: 10px;
  }

  .tab-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: none;
    border: 1px solid transparent;
    padding: 7px 14px;
    border-radius: 8px;
    font-size: 13px;
    font-weight: 500;
    color: var(--text-muted, #888);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .tab-btn .material-symbols-outlined {
    font-size: 17px;
  }

  .tab-btn:hover {
    color: #fff;
    background: rgba(255, 255, 255, 0.05);
  }

  .tab-btn.active {
    color: var(--accent);
    background: rgba(var(--accent-rgb), 0.12);
    border-color: rgba(var(--accent-rgb), 0.25);
    font-weight: 600;
  }

  .tab-panel {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  /* Search Form */
  .music-search-form {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    box-sizing: border-box;
  }

  .music-search-input-wrap {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    background: rgba(0, 0, 0, 0.25);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 8px;
    height: 40px;
    padding: 0 10px;
    gap: 8px;
    transition: border-color 0.2s ease, box-shadow 0.2s ease;
    box-sizing: border-box;
  }

  .music-search-input-wrap:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px rgba(var(--accent-rgb), 0.2);
    background: rgba(0, 0, 0, 0.35);
  }

  .search-leading-icon {
    color: #888;
    font-size: 18px;
    flex-shrink: 0;
  }

  .music-search-input {
    flex: 1;
    min-width: 0;
    background: transparent;
    border: none;
    outline: none;
    color: #fff;
    font-size: 13.5px;
    font-family: inherit;
  }

  .music-search-input::placeholder {
    color: #777;
  }

  .music-search-clear-btn {
    background: transparent;
    border: none;
    color: #888;
    cursor: pointer;
    padding: 2px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .music-search-clear-btn:hover {
    color: #fff;
    background: rgba(255, 255, 255, 0.1);
  }

  .music-search-clear-btn .material-symbols-outlined {
    font-size: 16px;
  }

  .music-search-submit-btn {
    height: 40px;
    padding: 0 16px;
    background: var(--accent);
    color: #fff;
    border: none;
    border-radius: 8px;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    white-space: nowrap;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    flex-shrink: 0;
    transition: background 0.15s ease, opacity 0.15s ease;
  }

  .music-search-submit-btn:hover:not(:disabled) {
    background: #f08337;
  }

  .music-search-submit-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .music-search-submit-btn .material-symbols-outlined {
    font-size: 18px;
  }

  .results-badge {
    font-size: 11.5px;
    color: var(--text-muted, #888);
    background: rgba(255, 255, 255, 0.05);
    padding: 3px 8px;
    border-radius: 6px;
  }

  /* Section Styles */
  .section-container {
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid var(--border-color, rgba(255, 255, 255, 0.06));
    border-radius: 12px;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    box-sizing: border-box;
  }

  .section-heading {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    font-weight: 600;
    margin: 0;
    color: var(--text-color, #eee);
  }

  .section-heading .material-symbols-outlined {
    font-size: 18px;
    color: var(--accent);
  }

  .candidates-list-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-bottom: 4px;
  }

  .candidates-heading {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-muted, #aaa);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  /* Initial Prompt State */
  .initial-prompt-box {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    padding: 24px 12px;
    gap: 8px;
  }

  .initial-prompt-icon {
    font-size: 36px;
    color: var(--accent);
    opacity: 0.8;
  }

  .initial-prompt-title {
    font-size: 15px;
    font-weight: 600;
    color: var(--text-color, #fff);
    margin: 0;
  }

  .initial-prompt-desc {
    font-size: 12.5px;
    color: var(--text-muted, #888);
    margin: 0;
    max-width: 440px;
    line-height: 1.4;
  }

  .initial-prompt-actions {
    display: flex;
    gap: 10px;
    margin-top: 8px;
    justify-content: center;
  }

  .prompt-action-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 8px 14px;
    border-radius: 8px;
    font-size: 12.5px;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .prompt-action-btn .material-symbols-outlined {
    font-size: 16px;
  }

  .prompt-action-btn.primary {
    background: var(--accent);
    color: #fff;
    border: none;
  }

  .prompt-action-btn.primary:hover:not(:disabled) {
    background: #f08337;
  }

  .prompt-action-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  /* Loading & Empty States */
  .loading-state,
  .empty-candidates {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 24px 0;
    gap: 10px;
    color: var(--text-muted, #888);
    font-size: 13px;
    text-align: center;
  }

  .empty-hint {
    font-size: 12px;
    color: #666;
    margin: 0;
  }

  .big-spinner {
    font-size: 32px;
    color: var(--accent);
  }

  .candidate-error-box,
  .manual-error-box {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
    background: rgba(239, 68, 68, 0.12);
    border: 1px solid rgba(239, 68, 68, 0.25);
    border-radius: 8px;
    color: #f87171;
    font-size: 12px;
  }

  /* Candidates List */
  .candidates-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: 280px;
    overflow-y: auto;
  }

  .candidate-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    background: rgba(0, 0, 0, 0.2);
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 8px;
    padding: 10px 14px;
    gap: 12px;
    transition: border-color 0.15s ease, background 0.15s ease;
  }

  .candidate-row:hover {
    border-color: rgba(255, 255, 255, 0.12);
    background: rgba(0, 0, 0, 0.28);
  }

  .candidate-details {
    display: flex;
    flex-direction: column;
    gap: 3px;
    overflow: hidden;
    flex: 1;
  }

  .candidate-title-line {
    display: flex;
    align-items: center;
    gap: 8px;
    overflow: hidden;
  }

  .cand-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-color, #fff);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .cand-disambiguation-badge {
    display: inline-flex;
    align-items: center;
    padding: 1px 7px;
    border-radius: 4px;
    font-size: 11px;
    font-weight: 600;
    line-height: 1.3;
    background: rgba(var(--accent-rgb), 0.16);
    color: #fb923c;
    border: 1px solid rgba(251, 146, 60, 0.35);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .candidate-lb-link {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 11px;
    font-weight: 500;
    color: var(--accent);
    background: rgba(var(--accent-rgb), 0.1);
    border: 1px solid rgba(var(--accent-rgb), 0.25);
    border-radius: 4px;
    padding: 1px 6px;
    text-decoration: none;
    flex-shrink: 0;
    transition: all 0.15s ease;
  }

  .candidate-lb-link:hover {
    background: rgba(var(--accent-rgb), 0.22);
    color: #fff;
    border-color: var(--accent);
  }

  .candidate-lb-link .external-link-icon {
    font-size: 12px;
  }

  .cand-artist {
    font-size: 13px;
    color: var(--accent);
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .cand-meta {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: var(--text-muted, #888);
    flex-wrap: wrap;
  }

  .cand-release {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 240px;
  }

  .cand-release-type {
    color: #e2e8f0;
    font-weight: 500;
  }

  .cand-release-date {
    color: var(--text-muted, #94a3b8);
  }

  .cand-match-badge {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 1px 6px;
    border-radius: 4px;
    font-size: 10px;
    font-weight: 600;
    background: rgba(34, 197, 94, 0.15);
    color: #4ade80;
    border: 1px solid rgba(34, 197, 94, 0.3);
    white-space: nowrap;
  }

  .cand-match-badge .match-icon {
    font-size: 12px;
  }

  .cand-id-tag {
    font-family: monospace;
    font-size: 10px;
    color: #64748b;
    padding: 0 4px;
    background: rgba(255, 255, 255, 0.04);
    border-radius: 3px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    white-space: nowrap;
  }

  .select-candidate-btn {
    background: rgba(255, 255, 255, 0.08);
    color: var(--text-color, #fff);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 6px;
    padding: 6px 12px;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 4px;
    white-space: nowrap;
    transition: all 0.15s;
    flex-shrink: 0;
  }

  .select-candidate-btn:hover:not(:disabled) {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }

  .select-candidate-btn:disabled {
    cursor: default;
  }

  /* Manual ID Section */
  .section-desc {
    font-size: 12px;
    color: var(--text-muted, #888);
    margin: 0;
    line-height: 1.4;
  }

  .manual-input-row {
    display: flex;
    gap: 10px;
    width: 100%;
    box-sizing: border-box;
  }

  .manual-input {
    flex: 1;
    min-width: 0;
    background: rgba(0, 0, 0, 0.25);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 8px;
    padding: 8px 12px;
    color: var(--text-color, #fff);
    font-size: 13px;
    outline: none;
    height: 38px;
    box-sizing: border-box;
  }

  .manual-input:focus {
    border-color: var(--accent);
  }

  .manual-search-btn {
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.12);
    color: var(--text-color, #fff);
    border-radius: 8px;
    padding: 0 14px;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
    height: 38px;
    transition: background 0.15s ease;
  }

  .manual-search-btn:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.15);
  }

  .manual-search-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .manual-preview-card {
    background: rgba(0, 0, 0, 0.3);
    border: 1px solid rgba(var(--accent-rgb), 0.25);
    border-radius: 10px;
    padding: 12px 14px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    box-sizing: border-box;
  }

  .manual-preview-details {
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow: hidden;
  }

  .preview-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-color, #fff);
  }

  .preview-artist {
    font-size: 13px;
    color: var(--accent);
  }

  .preview-release,
  .preview-mbid {
    font-size: 11px;
    color: var(--text-muted, #888);
  }

  .confirm-manual-btn {
    background: var(--accent);
    color: #fff;
    border: none;
    border-radius: 6px;
    padding: 8px 14px;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
    transition: background 0.15s ease;
  }

  .confirm-manual-btn:hover:not(:disabled) {
    background: #f08337;
  }

  .spin {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from {
      transform: rotate(0deg);
    }
    to {
      transform: rotate(360deg);
    }
  }
</style>

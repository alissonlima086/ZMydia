<script lang="ts">
  import { onMount } from 'svelte';
  import { apiFetch } from '../api';
  import { t } from '../i18n/i18n.svelte';
  import Button from '../ui/Button.svelte';
  import ConfirmDialog from '../ui/ConfirmDialog.svelte';
  import Notice from '../ui/Notice.svelte';
  import SettingGroup from '../ui/SettingGroup.svelte';
  import SettingRow from '../ui/SettingRow.svelte';
  import FilesystemCenter from './FilesystemCenter.svelte';
  import { type DownloadSettings, type MediaSource, formatBytes, formatDate, messageFrom, request } from './types';

  let sources = $state<MediaSource[]>([]);
  let path = $state('');
  let recursive = $state(true);
  let mode = $state<'read_only' | 'managed'>('read_only');
  let loading = $state(true);
  let submitting = $state(false);
  let actionId = $state<number | null>(null);
  let scanningAll = $state(false);
  let feedback = $state<{ text: string; tone: 'success' | 'error' | 'warning' } | null>(null);
  let feedbackTimeout: number | null = null;
  let confirmAction = $state<{ kind: 'remove' | 'managed'; source: MediaSource } | null>(null);
  let downloadSettings = $state<DownloadSettings>({ target_source_id: null, preferred_container: 'mkv' });
  let savingDownloadSettings = $state(false);

  function showFeedback(text: string, tone: 'success' | 'error' | 'warning' = 'success') {
    feedback = { text, tone };
    if (feedbackTimeout) clearTimeout(feedbackTimeout);
    feedbackTimeout = window.setTimeout(() => (feedback = null), 4500);
  }

  onMount(() => {
    void loadAll();
    const interval = window.setInterval(() => {
      if (sources.some((source) => source.status === 'pending' || source.status === 'scanning')) void refreshSources(false);
    }, 2000);
    return () => window.clearInterval(interval);
  });

  function savedOrder() { try { return JSON.parse(localStorage.getItem('mymedia_sources_order') || '[]') as number[]; } catch { return []; } }
  function persistOrder() { localStorage.setItem('mymedia_sources_order', JSON.stringify(sources.map((source) => source.id))); }

  async function loadAll() {
    loading = true;
    await Promise.all([refreshSources(false), loadDownloadSettings()]);
    loading = false;
  }

  async function loadDownloadSettings() {
    try { const response = await apiFetch('/api/settings/downloads'); if (response.ok) downloadSettings = await response.json(); } catch {}
  }

  async function refreshSources(showLoading = true) {
    if (showLoading) loading = true;
    try {
      const list = await request<MediaSource[]>('/api/sources');
      const order = savedOrder();
      list.sort((a, b) => {
        const ai = order.indexOf(a.id); const bi = order.indexOf(b.id);
        if (ai < 0 && bi < 0) return a.id - b.id;
        if (ai < 0) return 1; if (bi < 0) return -1; return ai - bi;
      });
      sources = list;
    } catch (error) { showFeedback(messageFrom(error), 'error'); }
    finally { if (showLoading) loading = false; }
  }

  async function addSource(event: SubmitEvent) {
    event.preventDefault(); if (!path.trim()) return;
    submitting = true;
    try {
      const result = await request<{ source: MediaSource }>('/api/sources', { method: 'POST', body: JSON.stringify({ path: path.trim(), recursive, mode }) });
      sources = [...sources, result.source]; persistOrder(); path = '';
      showFeedback('Pasta adicionada. A varredura inicial foi agendada.');
      await refreshSources(false);
    } catch (error) { showFeedback(messageFrom(error), 'error'); }
    finally { submitting = false; }
  }

  async function updateDownloadSettings(target_source_id: number | null, preferred_container = downloadSettings.preferred_container) {
    savingDownloadSettings = true;
    try {
      const response = await apiFetch('/api/settings/downloads', { method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ target_source_id, preferred_container }) });
      if (!response.ok) throw new Error('Não foi possível salvar o destino de downloads.');
      downloadSettings = await response.json(); showFeedback('Preferências de download atualizadas.');
    } catch (error) { showFeedback(messageFrom(error), 'error'); }
    finally { savingDownloadSettings = false; }
  }

  async function scanSource(source: MediaSource) {
    actionId = source.id;
    try { await request(`/api/sources/${source.id}/scan`, { method: 'POST' }); showFeedback(`Varredura iniciada em “${source.path}”.`); await refreshSources(false); }
    catch (error) { showFeedback(messageFrom(error), 'error'); } finally { actionId = null; }
  }
  async function scanAllSources() {
    scanningAll = true;
    try { await request('/api/scans', { method: 'POST' }); showFeedback('Varredura iniciada nas pastas ativas.'); await refreshSources(false); }
    catch (error) { showFeedback(messageFrom(error), 'error'); } finally { scanningAll = false; }
  }
  async function toggleSource(source: MediaSource) {
    actionId = source.id;
    try {
      await request(`/api/sources/${source.id}`, { method: 'PATCH', body: JSON.stringify({ enabled: !source.enabled }) });
      if (source.enabled && downloadSettings.target_source_id === source.id) await updateDownloadSettings(null);
      await refreshSources(false); showFeedback(`Pasta ${source.enabled ? 'desativada' : 'ativada'}.`);
    } catch (error) { showFeedback(messageFrom(error), 'error'); } finally { actionId = null; }
  }
  async function setMode(source: MediaSource, nextMode: 'read_only' | 'managed') {
    actionId = source.id;
    try {
      await request(`/api/sources/${source.id}`, { method: 'PATCH', body: JSON.stringify({ mode: nextMode }) });
      if (nextMode === 'read_only' && downloadSettings.target_source_id === source.id) await updateDownloadSettings(null);
      await refreshSources(false); showFeedback(t(`settings.folders.mode_${nextMode}_saved`));
    } catch (error) { showFeedback(messageFrom(error), 'error'); } finally { actionId = null; confirmAction = null; }
  }
  async function removeSource(source: MediaSource) {
    actionId = source.id;
    try {
      await request(`/api/sources/${source.id}`, { method: 'DELETE' });
      sources = sources.filter((item) => item.id !== source.id); persistOrder();
      if (downloadSettings.target_source_id === source.id) await updateDownloadSettings(null);
      showFeedback('Pasta removida do catálogo. Nenhum arquivo foi apagado.');
    } catch (error) { showFeedback(messageFrom(error), 'error'); } finally { actionId = null; confirmAction = null; }
  }
  function move(sourceIndex: number, direction: -1 | 1) {
    const target = sourceIndex + direction; if (target < 0 || target >= sources.length) return;
    const reordered = [...sources]; [reordered[sourceIndex], reordered[target]] = [reordered[target], reordered[sourceIndex]];
    sources = reordered; persistOrder(); showFeedback('Prioridade do catálogo atualizada.');
  }
</script>

<div class="folder-settings">
  {#if feedback}<Notice tone={feedback.tone}>{feedback.text}</Notice>{/if}

  <SettingGroup title="Adicionar pasta" description={t('settings.folders.description')}>
    <form class="source-form" onsubmit={addSource}>
      <label for="source-path">{t('settings.folders.path_label')}</label>
      <div class="path-row"><input id="source-path" bind:value={path} placeholder={t('settings.folders.path_placeholder')} autocomplete="off" spellcheck="false" required /><Button type="submit" variant="primary" disabled={submitting || !path.trim()}>{submitting ? t('settings.folders.validating') : t('settings.folders.add_folder')}</Button></div>
      <div class="source-options">
        <label><input type="checkbox" bind:checked={recursive} />{t('settings.folders.include_subfolders')}</label>
        <label>{t('settings.folders.mode_label')}<select bind:value={mode}><option value="read_only">{t('settings.folders.mode_read_only')}</option><option value="managed">{t('settings.folders.mode_managed')}</option></select></label>
      </div>
      <p class="mode-help">{mode === 'managed' ? t('settings.folders.mode_managed_help') : t('settings.folders.mode_read_only_help')}</p>
    </form>
  </SettingGroup>

  <SettingGroup title="Downloads" description="O destino de downloads é independente da prioridade usada para navegar no catálogo.">
    <SettingRow label="Pasta de destino" description="Somente pastas gerenciadas, ativas e disponíveis podem receber downloads.">
      {#snippet control()}
        <select class="destination" value={downloadSettings.target_source_id ?? ''} onchange={(event) => updateDownloadSettings(event.currentTarget.value ? Number(event.currentTarget.value) : null)} disabled={savingDownloadSettings}>
          <option value="">Nenhuma</option>
          {#each sources.filter((source) => source.mode === 'managed' && source.enabled && (source.available ?? true) && (source.writable ?? true)) as source}<option value={source.id}>{source.path}</option>{/each}
        </select>
      {/snippet}
    </SettingRow>
    <SettingRow label={t('settings.folders.format_title')} description={t('settings.folders.format_desc')}>
      {#snippet control()}
        <select value={downloadSettings.preferred_container} onchange={(event) => updateDownloadSettings(downloadSettings.target_source_id, event.currentTarget.value)} disabled={savingDownloadSettings}><option value="mkv">MKV</option><option value="mp4">MP4</option></select>
      {/snippet}
    </SettingRow>
  </SettingGroup>

  <SettingGroup title={t('settings.folders.title')} description="A ordem abaixo altera apenas a prioridade de exibição do catálogo.">
    {#snippet actions()}<Button size="sm" onclick={scanAllSources} disabled={scanningAll || !sources.some((source) => source.enabled)}>{scanningAll ? t('settings.folders.scanning') : t('settings.folders.scan_all_btn')}</Button>{/snippet}
    {#if loading && sources.length === 0}<div class="empty">{t('common.loading')}</div>
    {:else if sources.length === 0}<div class="empty">{t('settings.folders.no_folders')}</div>
    {:else}
      <div class="source-table" role="table" aria-label={t('settings.folders.title')}>
        <div class="source-head" role="row"><span>Pasta</span><span>Modo e acesso</span><span>Última varredura</span><span>Ações</span></div>
        {#each sources as source, index (source.id)}
          {@const sourceAvailable = source.available ?? source.status !== 'error'}
          {@const sourceReadable = source.readable ?? sourceAvailable}
          {@const sourceWritable = source.writable ?? source.mode === 'managed'}
          <div class="source-row" role="row">
            <div class="source-path"><strong title={source.path}>{source.path}</strong><small>{source.file_count} vídeo(s) · prioridade {index + 1}</small>{#if source.error_message}<small class="error">{source.error_message}</small>{/if}</div>
            <div class="source-state">
              <span class:managed={source.mode === 'managed'}>{source.mode === 'managed' ? 'Gerenciada' : 'Somente leitura'}</span>
              <small class:error={!sourceAvailable || !sourceReadable}>
                {#if !sourceAvailable}Indisponível
                {:else if !sourceReadable}Sem permissão de leitura
                {:else if source.mode === 'managed' && !sourceWritable}Sem permissão de escrita
                {:else}{sourceWritable ? 'Leitura e escrita' : 'Somente leitura no disco'}{/if}
              </small>
              <small>{source.free_space_bytes == null ? 'Espaço desconhecido' : `${formatBytes(source.free_space_bytes)} livres`}</small>
              <small class:error={source.status === 'error'}>{source.enabled ? t(`settings.folders.status_${source.status}`) : t('settings.folders.status_disabled')}</small>
            </div>
            <span class="last-scan">{formatDate(source.last_scanned_at)}</span>
            <div class="source-actions">
              <Button size="sm" variant="ghost" onclick={() => move(index, -1)} disabled={index === 0} aria-label={t('settings.folders.move_up_tip')}>↑</Button>
              <Button size="sm" variant="ghost" onclick={() => move(index, 1)} disabled={index === sources.length - 1} aria-label={t('settings.folders.move_down_tip')}>↓</Button>
              <Button size="sm" onclick={() => scanSource(source)} disabled={!source.enabled || actionId === source.id || source.status === 'scanning'}>{t('settings.folders.scan_single_btn')}</Button>
              <details><summary aria-label="Mais ações">•••</summary><div class="action-menu">
                <button type="button" onclick={() => toggleSource(source)}>{source.enabled ? t('settings.folders.disable') : t('settings.folders.enable')}</button>
                <button type="button" onclick={() => source.mode === 'managed' ? setMode(source, 'read_only') : (confirmAction = { kind: 'managed', source })}>{source.mode === 'managed' ? t('settings.folders.set_read_only') : t('settings.folders.set_managed')}</button>
                <button class="danger" type="button" onclick={() => (confirmAction = { kind: 'remove', source })}>{t('settings.folders.remove')}</button>
              </div></details>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </SettingGroup>

  <FilesystemCenter />
</div>

<ConfirmDialog open={confirmAction?.kind === 'managed'} title="Permitir alterações nesta pasta?" description={confirmAction ? `${t('settings.folders.managed_confirm')}\n${confirmAction.source.path}` : ''} confirmLabel="Tornar gerenciada" onConfirm={() => confirmAction && setMode(confirmAction.source, 'managed')} onCancel={() => (confirmAction = null)} busy={actionId !== null} />
<ConfirmDialog open={confirmAction?.kind === 'remove'} title="Remover pasta do catálogo?" description={confirmAction ? `Os arquivos em “${confirmAction.source.path}” permanecerão no disco. Apenas o índice será removido.` : ''} confirmLabel="Remover pasta" dangerous onConfirm={() => confirmAction && removeSource(confirmAction.source)} onCancel={() => (confirmAction = null)} busy={actionId !== null} />

<style>
  .folder-settings { display: grid; gap: var(--space-6); }
  .source-form { display: grid; gap: var(--space-3); padding: var(--space-4); }
  .source-form > label { color: var(--text-primary); font: var(--type-label); }
  .path-row { display: flex; gap: var(--space-2); }
  .path-row input { flex: 1; min-width: 0; }
  input, select { min-height: var(--control-height); padding: 0 var(--space-3); border: 1px solid var(--border-default); border-radius: var(--radius-md); background: var(--surface-0); color: var(--text-primary); }
  .source-options { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-5); }
  .source-options label { display: flex; align-items: center; gap: var(--space-2); color: var(--text-secondary); font: var(--type-caption); }
  .source-options input { width: 18px; min-height: 18px; accent-color: var(--accent); }
  .mode-help { margin: 0; color: var(--text-muted); font: var(--type-caption); }
  .destination { max-width: min(380px, 48vw); }
  .empty { padding: var(--space-8); text-align: center; color: var(--text-muted); }
  .source-table { min-width: 0; }
  .source-head, .source-row { display: grid; grid-template-columns: minmax(220px, 1.5fr) minmax(130px, .7fr) minmax(120px, .7fr) auto; gap: var(--space-4); align-items: center; padding: var(--space-3) var(--space-4); }
  .source-head { color: var(--text-muted); font: var(--type-caption); background: var(--surface-2); }
  .source-row + .source-row { border-top: 1px solid var(--border-subtle); }
  .source-path, .source-state { min-width: 0; display: grid; gap: var(--space-1); }
  .source-path strong { overflow: hidden; text-overflow: ellipsis; color: var(--text-primary); font: var(--type-label); }
  small, .last-scan { color: var(--text-muted); font: var(--type-caption); }
  .source-state > span { width: fit-content; padding: 2px var(--space-2); border-radius: 999px; background: var(--surface-3); color: var(--text-secondary); font: var(--type-caption); }
  .source-state > span.managed { background: var(--accent-soft); color: var(--accent); }
  .error { color: var(--danger); }
  .source-actions { position: relative; display: flex; align-items: center; justify-content: flex-end; gap: var(--space-1); }
  details { position: relative; }
  summary { width: 36px; height: 32px; display: grid; place-items: center; border-radius: var(--radius-md); color: var(--text-secondary); cursor: pointer; list-style: none; }
  summary:hover { background: var(--overlay); }
  .action-menu { position: absolute; z-index: 10; top: calc(100% + 4px); right: 0; min-width: 190px; display: grid; padding: var(--space-1); border: 1px solid var(--border-default); border-radius: var(--radius-md); background: var(--surface-3); box-shadow: var(--shadow-overlay); }
  .action-menu button { padding: var(--space-2) var(--space-3); border: 0; border-radius: var(--radius-sm); background: transparent; color: var(--text-secondary); text-align: left; cursor: pointer; }
  .action-menu button:hover { background: var(--overlay); color: var(--text-primary); }
  .action-menu button.danger { color: var(--danger); }
  @media (max-width: 780px) { .source-head { display: none; } .source-row { grid-template-columns: 1fr auto; } .source-state, .last-scan { grid-column: 1; } .source-actions { grid-column: 2; grid-row: 1 / span 3; } }
  @media (max-width: 560px) { .path-row { flex-direction: column; } .source-row { grid-template-columns: 1fr; } .source-actions { grid-column: 1; grid-row: auto; justify-content: flex-start; } .destination { max-width: 100%; width: 100%; } }
</style>

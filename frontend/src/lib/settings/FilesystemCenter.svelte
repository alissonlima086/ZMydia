<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../i18n/i18n.svelte';
  import { formatBytes, formatDate, messageFrom, request } from './types';

  type DuplicateCandidate = {
    id: number;
    status: string;
    score: number;
    reason: Record<string, unknown> | null;
    winner_path: string;
    winner_size_bytes: number;
    duplicate_path: string;
    duplicate_size_bytes: number;
    created_at: string;
    decided_at: string | null;
  };

  type FilesystemOperation = {
    id: number;
    operation_type: string;
    state: string;
    from_path: string;
    to_path: string;
    quarantine_path: string | null;
    size_bytes: number | null;
    checksum_sha256: string | null;
    error_message: string | null;
    retention_until: string | null;
    created_at: string;
    completed_at: string | null;
    can_undo: boolean;
    can_discard: boolean;
  };

  let duplicates = $state<DuplicateCandidate[]>([]);
  let operations = $state<FilesystemOperation[]>([]);
  let loading = $state(true);
  let action = $state<string | null>(null);
  let feedback = $state<string | null>(null);

  onMount(() => {
    void load();
  });

  async function load() {
    loading = true;
    try {
      [duplicates, operations] = await Promise.all([
        request<DuplicateCandidate[]>('/api/duplicates'),
        request<FilesystemOperation[]>('/api/filesystem/operations')
      ]);
    } catch (error) {
      feedback = messageFrom(error);
    } finally {
      loading = false;
    }
  }

  async function candidateAction(candidate: DuplicateCandidate, decision: 'approve' | 'reject') {
    if (decision === 'approve' && !confirm(t('settings.filesystem.approve_confirm'))) return;
    action = `candidate-${candidate.id}`;
    try {
      await request(`/api/duplicates/${candidate.id}/${decision}`, { method: 'POST' });
      feedback = decision === 'approve'
        ? t('settings.filesystem.quarantined_feedback')
        : t('settings.filesystem.rejected_feedback');
      await load();
    } catch (error) {
      feedback = messageFrom(error);
    } finally {
      action = null;
    }
  }

  async function undo(operation: FilesystemOperation) {
    if (!confirm(t('settings.filesystem.undo_confirm'))) return;
    action = `operation-${operation.id}`;
    try {
      await request(`/api/filesystem/operations/${operation.id}/undo`, { method: 'POST' });
      feedback = t('settings.filesystem.undo_feedback');
      await load();
    } catch (error) {
      feedback = messageFrom(error);
    } finally {
      action = null;
    }
  }

  async function discard(operation: FilesystemOperation) {
    if (!confirm(t('settings.filesystem.discard_confirm'))) return;
    action = `operation-${operation.id}`;
    try {
      await request(`/api/filesystem/operations/${operation.id}/discard`, { method: 'DELETE' });
      feedback = t('settings.filesystem.discard_feedback');
      await load();
    } catch (error) {
      feedback = messageFrom(error);
    } finally {
      action = null;
    }
  }

  function filename(path: string): string {
    const parts = path.split(/[\\/]/).filter(Boolean);
    return parts[parts.length - 1] ?? path;
  }
</script>

<section class="filesystem-center" aria-labelledby="filesystem-center-title">
  <div class="section-heading">
    <div>
      <p>{t('settings.filesystem.eyebrow')}</p>
      <h2 id="filesystem-center-title">{t('settings.filesystem.title')}</h2>
    </div>
    <button type="button" class="btn-scan-all" onclick={load} disabled={loading}>
      {loading ? t('common.loading') : t('settings.filesystem.refresh')}
    </button>
  </div>
  <p class="section-desc-hint">{t('settings.filesystem.description')}</p>

  {#if feedback}
    <div class="dashboard-feedback" role="status">{feedback}</div>
  {/if}

  <div class="filesystem-group">
    <h3>{t('settings.filesystem.duplicates_title')}</h3>
    {#if loading && duplicates.length === 0}
      <p class="source-placeholder">{t('common.loading')}</p>
    {:else if duplicates.filter((item) => item.status === 'pending').length === 0}
      <p class="source-placeholder">{t('settings.filesystem.no_duplicates')}</p>
    {:else}
      <div class="filesystem-table-wrap">
        <table class="filesystem-table">
          <thead>
            <tr>
              <th>{t('settings.filesystem.keep')}</th>
              <th>{t('settings.filesystem.candidate')}</th>
              <th>{t('settings.filesystem.comparison')}</th>
              <th>{t('settings.filesystem.actions')}</th>
            </tr>
          </thead>
          <tbody>
            {#each duplicates.filter((item) => item.status === 'pending') as candidate (candidate.id)}
              <tr>
                <td title={candidate.winner_path}>
                  <strong>{filename(candidate.winner_path)}</strong>
                  <small>{formatBytes(candidate.winner_size_bytes)}</small>
                </td>
                <td title={candidate.duplicate_path}>
                  <strong>{filename(candidate.duplicate_path)}</strong>
                  <small>{formatBytes(candidate.duplicate_size_bytes)}</small>
                </td>
                <td>
                  <span>{t('settings.filesystem.score', { score: candidate.score })}</span>
                  <small>{t('settings.filesystem.hash_on_approve')}</small>
                </td>
                <td class="filesystem-actions">
                  <button
                    type="button"
                    onclick={() => candidateAction(candidate, 'approve')}
                    disabled={action === `candidate-${candidate.id}`}
                  >{t('settings.filesystem.quarantine')}</button>
                  <button
                    type="button"
                    onclick={() => candidateAction(candidate, 'reject')}
                    disabled={action === `candidate-${candidate.id}`}
                  >{t('settings.filesystem.keep_both')}</button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>

  <div class="filesystem-group">
    <h3>{t('settings.filesystem.journal_title')}</h3>
    {#if operations.length === 0}
      <p class="source-placeholder">{t('settings.filesystem.no_operations')}</p>
    {:else}
      <div class="filesystem-table-wrap">
        <table class="filesystem-table">
          <thead>
            <tr>
              <th>{t('settings.filesystem.operation')}</th>
              <th>{t('settings.filesystem.state')}</th>
              <th>{t('settings.filesystem.retention')}</th>
              <th>{t('settings.filesystem.actions')}</th>
            </tr>
          </thead>
          <tbody>
            {#each operations as operation (operation.id)}
              <tr>
                <td title={`${operation.from_path} → ${operation.to_path}`}>
                  <strong>{t(`settings.filesystem.type_${operation.operation_type}`)}</strong>
                  <small>{filename(operation.from_path)}</small>
                </td>
                <td>
                  <span class:error-state={operation.state === 'failed'}>{t(`settings.filesystem.state_${operation.state}`)}</span>
                  {#if operation.error_message}<small>{operation.error_message}</small>{/if}
                </td>
                <td>
                  {operation.retention_until ? formatDate(operation.retention_until) : '—'}
                </td>
                <td class="filesystem-actions">
                  {#if operation.can_undo}
                    <button type="button" onclick={() => undo(operation)} disabled={action === `operation-${operation.id}`}>
                      {t('settings.filesystem.undo')}
                    </button>
                  {/if}
                  {#if operation.can_discard}
                    <button class="danger" type="button" onclick={() => discard(operation)} disabled={action === `operation-${operation.id}`}>
                      {t('settings.filesystem.discard')}
                    </button>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>
</section>

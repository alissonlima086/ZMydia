<script lang="ts">
  import Button from './Button.svelte';

  let {
    open,
    title,
    description,
    confirmLabel = 'Confirmar',
    cancelLabel = 'Cancelar',
    dangerous = false,
    busy = false,
    onConfirm,
    onCancel
  }: {
    open: boolean;
    title: string;
    description: string;
    confirmLabel?: string;
    cancelLabel?: string;
    dangerous?: boolean;
    busy?: boolean;
    onConfirm: () => void;
    onCancel: () => void;
  } = $props();

  function handleKeydown(event: KeyboardEvent) {
    if (open && event.key === 'Escape' && !busy) {
      event.preventDefault();
      onCancel();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if open}
  <div class="confirm-dialog__backdrop">
    <button class="confirm-dialog__dismiss" type="button" aria-label={cancelLabel} onclick={() => !busy && onCancel()}></button>
    <div
      class="confirm-dialog"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="confirm-dialog-title"
      aria-describedby="confirm-dialog-description"
    >
      <div class="confirm-dialog__icon" class:dangerous aria-hidden="true">
        <span class="material-symbols-outlined">{dangerous ? 'warning' : 'help'}</span>
      </div>
      <div>
        <h2 id="confirm-dialog-title">{title}</h2>
        <p id="confirm-dialog-description">{description}</p>
      </div>
      <div class="confirm-dialog__actions">
        <Button variant="ghost" disabled={busy} onclick={onCancel}>{cancelLabel}</Button>
        <Button variant={dangerous ? 'danger' : 'primary'} disabled={busy} onclick={onConfirm}>
          {busy ? 'Processando…' : confirmLabel}
        </Button>
      </div>
    </div>
  </div>
{/if}

<style>
  .confirm-dialog__backdrop { position: fixed; inset: 0; z-index: 1200; display: grid; place-items: center; padding: var(--space-4); background: rgb(0 0 0 / 72%); }
  .confirm-dialog__dismiss { position: absolute; inset: 0; width: 100%; height: 100%; border: 0; background: transparent; cursor: default; }
  .confirm-dialog { position: relative; width: min(440px, 100%); display: grid; grid-template-columns: auto 1fr; gap: var(--space-4); padding: var(--space-6); border: 1px solid var(--border-strong); border-radius: var(--radius-lg); background: var(--surface-2); box-shadow: var(--shadow-overlay); }
  .confirm-dialog__icon { width: 38px; height: 38px; display: grid; place-items: center; border-radius: var(--radius-full); background: var(--accent-soft); color: var(--accent); }
  .confirm-dialog__icon.dangerous { background: color-mix(in srgb, var(--danger) 14%, transparent); color: var(--danger); }
  h2 { margin: 0; color: var(--text-primary); font: var(--type-section-title); }
  p { margin: var(--space-2) 0 0; color: var(--text-secondary); font: var(--type-body); }
  .confirm-dialog__actions { grid-column: 1 / -1; display: flex; justify-content: flex-end; gap: var(--space-2); margin-top: var(--space-2); }
</style>

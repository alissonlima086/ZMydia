<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';

  type Props = HTMLButtonAttributes & {
    children: Snippet;
    variant?: 'primary' | 'secondary' | 'ghost' | 'danger';
    size?: 'sm' | 'md';
  };

  let {
    children,
    variant = 'secondary',
    size = 'md',
    class: className = '',
    ...rest
  }: Props = $props();
</script>

<button {...rest} class={`ui-button ui-button--${variant} ui-button--${size} ${className}`.trim()}>
  {@render children()}
</button>

<style>
  .ui-button {
    min-height: var(--control-height);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    border: 1px solid transparent;
    border-radius: var(--radius-md);
    font: var(--type-label);
    cursor: pointer;
    transition:
      color var(--motion-fast) var(--ease-standard),
      background var(--motion-fast) var(--ease-standard),
      border-color var(--motion-fast) var(--ease-standard),
      opacity var(--motion-fast) var(--ease-standard);
  }
  .ui-button--md { padding: 0 var(--space-4); }
  .ui-button--sm { min-height: 32px; padding: 0 var(--space-3); }
  .ui-button--primary { background: var(--accent); color: white; }
  .ui-button--primary:hover:not(:disabled) { background: var(--accent-hover); }
  .ui-button--secondary {
    border-color: var(--border-default);
    background: var(--surface-2);
    color: var(--text-primary);
  }
  .ui-button--secondary:hover:not(:disabled) { border-color: var(--border-strong); background: var(--overlay); }
  .ui-button--ghost { background: transparent; color: var(--text-secondary); }
  .ui-button--ghost:hover:not(:disabled) { background: var(--surface-2); color: var(--text-primary); }
  .ui-button--danger { border-color: color-mix(in srgb, var(--danger) 42%, transparent); background: color-mix(in srgb, var(--danger) 12%, transparent); color: #fca5a5; }
  .ui-button--danger:hover:not(:disabled) { background: color-mix(in srgb, var(--danger) 20%, transparent); }
  .ui-button:disabled { cursor: not-allowed; opacity: 0.5; }
</style>

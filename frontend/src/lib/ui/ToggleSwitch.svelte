<script lang="ts">
  let {
    checked = $bindable(false),
    disabled = false,
    label,
    onchange
  }: { checked?: boolean; disabled?: boolean; label: string; onchange?: () => void } = $props();
</script>

<label class="toggle-switch">
  <input type="checkbox" bind:checked {disabled} aria-label={label} {onchange} />
  <span aria-hidden="true"></span>
</label>

<style>
  .toggle-switch { display: inline-flex; cursor: pointer; }
  input { position: absolute; opacity: 0; pointer-events: none; }
  span { width: 42px; height: 24px; padding: 3px; border-radius: 999px; background: var(--surface-3); transition: background var(--motion-fast); }
  span::after { content: ''; display: block; width: 18px; height: 18px; border-radius: 50%; background: var(--text-muted); transition: transform var(--motion-fast), background var(--motion-fast); }
  input:checked + span { background: var(--accent); }
  input:checked + span::after { transform: translateX(18px); background: white; }
  input:focus-visible + span { outline: 2px solid var(--accent); outline-offset: 2px; }
  input:disabled + span { opacity: .5; cursor: not-allowed; }
</style>

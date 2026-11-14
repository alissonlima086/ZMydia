<script lang="ts">
  import SettingsView from './SettingsView.svelte';

  interface Props {
    activeTab?: string;
    onTabChange?: (tab: string) => void;
  }

  let { activeTab = 'dashboard', onTabChange }: Props = $props();
</script>

<SettingsView {activeTab} {onTabChange} />

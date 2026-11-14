<script lang="ts">
  import SettingsDashboard from './settings/SettingsDashboard.svelte';
  import SettingsAccount from './settings/SettingsAccount.svelte';
  import SettingsLanguage from './settings/SettingsLanguage.svelte';
  import SettingsFolders from './settings/SettingsFolders.svelte';
  import SettingsYoutube from './settings/SettingsYoutube.svelte';
  import SettingsSubtitles from './settings/SettingsSubtitles.svelte';
  import SettingsFeed from './settings/SettingsFeed.svelte';
  import SettingsExtension from './settings/SettingsExtension.svelte';
  import SettingsListenBrainz from './settings/SettingsListenBrainz.svelte';
  import PageHeader from './ui/PageHeader.svelte';
  import PageShell from './ui/PageShell.svelte';
  import { t } from './i18n/i18n.svelte';

  interface Props {
    activeTab?: string;
    onTabChange?: (tab: string) => void;
  }

  let { activeTab = 'dashboard', onTabChange }: Props = $props();

  const tabs = $derived([
    { id: 'dashboard', label: t('settings.tabs.dashboard'), icon: 'dashboard' },
    { id: 'conta', label: t('settings.tabs.account'), icon: 'manage_accounts' },
    { id: 'idioma', label: t('settings.tabs.language'), icon: 'language' },
    { id: 'pastas', label: t('settings.tabs.folders'), icon: 'folder' },
    { id: 'feed', label: t('settings.tabs.feed'), icon: 'filter_alt' },
    { id: 'youtube', label: t('settings.tabs.youtube'), icon: 'smart_display' },
    { id: 'legendas', label: t('settings.tabs.subtitles'), icon: 'subtitles' },
    { id: 'extensao', label: t('settings.tabs.extension'), icon: 'extension' },
    { id: 'listenbrainz', label: t('settings.tabs.listenbrainz'), icon: 'album' }
  ]);
</script>

<PageShell width="content" className="settings-page-shell">
  <PageHeader title={t('nav.settings_title')} description={t('nav.settings_desc')} />

  <div class="settings-shell">
    <nav class="settings-shell__nav" aria-label={t('settings.mobile_sections')}>
      {#each tabs as tab}
        <button
          type="button"
          class="settings-shell__nav-item"
          class:active={activeTab === tab.id}
          aria-current={activeTab === tab.id ? 'page' : undefined}
          onclick={() => onTabChange?.(tab.id)}
        >
          <span class="material-symbols-outlined" aria-hidden="true">{tab.icon}</span>
          <span>{tab.label}</span>
        </button>
      {/each}
    </nav>

    <section class="settings-shell__content" aria-live="polite">
      {#if activeTab === 'dashboard'}
        <SettingsDashboard />
      {:else if activeTab === 'conta'}
        <SettingsAccount />
      {:else if activeTab === 'idioma'}
        <SettingsLanguage />
      {:else if activeTab === 'pastas'}
        <SettingsFolders />
      {:else if activeTab === 'youtube'}
        <SettingsYoutube />
      {:else if activeTab === 'legendas'}
        <SettingsSubtitles />
      {:else if activeTab === 'feed'}
        <SettingsFeed />
      {:else if activeTab === 'extensao'}
        <SettingsExtension />
      {:else if activeTab === 'listenbrainz'}
        <SettingsListenBrainz />
      {/if}
    </section>
  </div>
</PageShell>

<style>
  .settings-shell {
    display: grid;
    grid-template-columns: 190px minmax(0, 1fr);
    align-items: start;
    gap: var(--space-8);
  }
  .settings-shell__nav {
    position: sticky;
    top: var(--space-4);
    display: grid;
    gap: var(--space-1);
  }
  .settings-shell__nav-item {
    min-height: 40px;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 0 var(--space-3);
    border: 0;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--text-secondary);
    font: var(--type-label);
    text-align: left;
    cursor: pointer;
  }
  .settings-shell__nav-item:hover { background: var(--surface-1); color: var(--text-primary); }
  .settings-shell__nav-item.active { background: var(--accent-soft); color: var(--text-primary); }
  .settings-shell__nav-item.active .material-symbols-outlined { color: var(--accent); }
  .settings-shell__nav-item .material-symbols-outlined { width: 20px; color: var(--text-muted); font-size: 19px; }
  .settings-shell__content { min-width: 0; }

  @media (max-width: 860px) {
    .settings-shell { grid-template-columns: 1fr; gap: var(--space-4); }
    .settings-shell__nav {
      position: static;
      display: flex;
      gap: var(--space-2);
      padding-bottom: var(--space-2);
      overflow-x: auto;
      scrollbar-width: thin;
    }
    .settings-shell__nav-item { flex: 0 0 auto; }
  }
  @media (max-width: 480px) {
    .settings-shell__nav-item { min-height: 36px; padding: 0 var(--space-2); }
    .settings-shell__nav-item .material-symbols-outlined { display: none; }
  }
</style>

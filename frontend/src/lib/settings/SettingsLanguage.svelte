<script lang="ts">
  import { i18n, t } from '../i18n/i18n.svelte';
  import { SUPPORTED_LOCALES, type SupportedLocale } from '../i18n/types';
  import SettingGroup from '../ui/SettingGroup.svelte';
  import SettingRow from '../ui/SettingRow.svelte';

  const localeNameKeys: Record<SupportedLocale, string> = {
    'pt-BR': 'settings.language.pt_br',
    'en-US': 'settings.language.en_us',
    'es-ES': 'settings.language.es_es'
  };
</script>

<SettingGroup title={t('settings.language.title')} description={t('settings.language.description')}>
  {#each SUPPORTED_LOCALES as locale}
    <SettingRow label={`${locale.flag} ${locale.nativeName}`} description={t(localeNameKeys[locale.code])}>
      {#snippet control()}
        <label class="language-choice">
          <input
            type="radio"
            name="interface-language"
            value={locale.code}
            checked={i18n.currentLocale === locale.code}
            onchange={() => i18n.setLocale(locale.code)}
          />
          <span>{i18n.currentLocale === locale.code ? t('common.active') : t('settings.language.select_label')}</span>
        </label>
      {/snippet}
    </SettingRow>
  {/each}
</SettingGroup>

<style>
  .language-choice { display: inline-flex; align-items: center; gap: var(--space-2); color: var(--text-secondary); font: var(--type-caption); cursor: pointer; }
  .language-choice input { width: 18px; height: 18px; accent-color: var(--accent); }
</style>

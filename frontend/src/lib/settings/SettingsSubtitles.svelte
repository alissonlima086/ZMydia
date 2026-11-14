<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../i18n/i18n.svelte';
  import { hexToRgba, POPULAR_GOOGLE_FONTS, subtitleStyleStore } from '../subtitleStyle.svelte';
  import Button from '../ui/Button.svelte';
  import Notice from '../ui/Notice.svelte';
  import SettingGroup from '../ui/SettingGroup.svelte';
  import SettingRow from '../ui/SettingRow.svelte';
  import ToggleSwitch from '../ui/ToggleSwitch.svelte';
  import { COMMON_SUB_LANGS, type SubtitleSettings, messageFrom, request } from './types';

  const defaults: SubtitleSettings = { auto_download: true, languages: ['pt', 'en'], include_auto_generated: false };
  let settings = $state<SubtitleSettings>({ ...defaults, languages: [...defaults.languages] });
  let saved = $state<SubtitleSettings>({ ...defaults, languages: [...defaults.languages] });
  let customLanguage = $state('');
  let customFont = $state('');
  let saving = $state(false);
  let syncing = $state(false);
  let controlsVisible = $state(false);
  let feedback = $state<{ text: string; tone: 'success' | 'error' } | null>(null);
  const dirty = $derived(JSON.stringify(settings) !== JSON.stringify(saved));

  onMount(async () => { try { settings = await request('/api/settings/subtitles'); saved = structuredClone(settings); } catch {} });
  function toggleLanguage(code: string) { const normalized = code.trim().toLowerCase(); settings.languages = settings.languages.includes(normalized) ? settings.languages.filter((item) => item !== normalized) : [...settings.languages, normalized]; }
  function addLanguage(event: SubmitEvent) { event.preventDefault(); if (customLanguage.trim()) toggleLanguage(customLanguage); customLanguage = ''; }
  async function save() { saving = true; try { settings = await request('/api/settings/subtitles', { method: 'PUT', body: JSON.stringify(settings) }); saved = structuredClone(settings); feedback = { text: t('settings.subtitles.saved_feedback'), tone: 'success' }; } catch (error) { feedback = { text: messageFrom(error), tone: 'error' }; } finally { saving = false; } }
  async function sync() { syncing = true; try { const result = await request<{ downloaded: number }>('/api/subtitles/sync', { method: 'POST' }); feedback = { text: result.downloaded ? t('settings.subtitles.downloaded_feedback', { count: result.downloaded }) : t('settings.subtitles.no_pending'), tone: 'success' }; } catch (error) { feedback = { text: messageFrom(error), tone: 'error' }; } finally { syncing = false; } }
</script>

<div class="subtitle-settings">
  {#if feedback}<Notice tone={feedback.tone}>{feedback.text}</Notice>{/if}
  <SettingGroup title={t('settings.subtitles.section_title')} description={t('settings.subtitles.section_desc')}>
    <SettingRow label={t('settings.subtitles.auto_download_label')} description={t('settings.subtitles.auto_download_desc')}>
      {#snippet control()}<ToggleSwitch bind:checked={settings.auto_download} disabled={saving} label={t('settings.subtitles.auto_download_label')} />{/snippet}
    </SettingRow>
    <SettingRow label={t('settings.subtitles.include_auto_label')} description={t('settings.subtitles.include_auto_desc')}>
      {#snippet control()}<ToggleSwitch bind:checked={settings.include_auto_generated} disabled={saving} label={t('settings.subtitles.include_auto_label')} />{/snippet}
    </SettingRow>
    <SettingRow label={t('settings.subtitles.preferred_langs_title')} description={t('settings.subtitles.preferred_langs_desc')} stacked>
      {#snippet control()}
        <div class="language-editor"><div class="language-list">{#each COMMON_SUB_LANGS as language}<button type="button" class:selected={settings.languages.includes(language.code)} onclick={() => toggleLanguage(language.code)}>{settings.languages.includes(language.code) ? '✓ ' : ''}{t(`subtitles.lang_${language.code}`)} ({language.code})</button>{/each}</div><form onsubmit={addLanguage}><input bind:value={customLanguage} placeholder={t('subtitles.custom_code_placeholder')} /><Button type="submit" size="sm" disabled={!customLanguage.trim()}>{t('subtitles.add_custom_btn')}</Button></form></div>
      {/snippet}
    </SettingRow>
    <div class="save-row"><span class:dirty>{dirty ? t('common.unsaved_changes') : t('settings.subtitles.languages_configured', { count: settings.languages.length })}</span><Button onclick={sync} disabled={syncing || settings.languages.length === 0}>{syncing ? t('subtitles.syncing_subs') : t('subtitles.sync_pending_btn')}</Button><Button variant="primary" onclick={save} disabled={!dirty || saving}>{saving ? t('common.saving') : t('common.save_preferences')}</Button></div>
  </SettingGroup>

  <SettingGroup title={t('settings.subtitles.appearance_title')} description={t('settings.subtitles.appearance_desc')}>
    {#snippet actions()}<Button size="sm" variant="ghost" onclick={() => subtitleStyleStore.reset()}>{t('settings.subtitles.reset_defaults')}</Button>{/snippet}
    <div class="appearance-grid">
      <div class="appearance-controls">
        <label>{t('settings.subtitles.font_size_label')} <output>{subtitleStyleStore.current.fontSizePercent}%</output><input type="range" min="75" max="200" step="5" value={subtitleStyleStore.current.fontSizePercent} oninput={(event) => subtitleStyleStore.update({ fontSizePercent: Number(event.currentTarget.value) })} /></label>
        <label>{t('settings.subtitles.bottom_distance_label')} <output>{subtitleStyleStore.current.bottomOffset}px</output><input type="range" min="20" max="120" step="2" value={subtitleStyleStore.current.bottomOffset} oninput={(event) => subtitleStyleStore.update({ bottomOffset: Number(event.currentTarget.value) })} /></label>
        <label>{t('settings.subtitles.bg_opacity_label')} <output>{subtitleStyleStore.current.bgOpacity}%</output><input type="range" min="0" max="100" step="5" value={subtitleStyleStore.current.bgOpacity} oninput={(event) => subtitleStyleStore.update({ bgOpacity: Number(event.currentTarget.value) })} /></label>
        <div class="two-columns"><label>{t('settings.subtitles.text_color_label')}<input type="color" value={subtitleStyleStore.current.textColor} oninput={(event) => subtitleStyleStore.update({ textColor: event.currentTarget.value })} /></label><label>{t('settings.subtitles.background_color')}<input type="color" value={subtitleStyleStore.current.bgColor} oninput={(event) => subtitleStyleStore.update({ bgColor: event.currentTarget.value })} /></label></div>
        <label>{t('settings.subtitles.font_label')}<select value={subtitleStyleStore.current.fontFamily} onchange={(event) => subtitleStyleStore.update({ fontFamily: event.currentTarget.value })}>{#each POPULAR_GOOGLE_FONTS as font}<option value={font.name}>{font.name} — {font.category}</option>{/each}</select></label>
        <form class="custom-font" onsubmit={(event) => { event.preventDefault(); if (customFont.trim()) subtitleStyleStore.update({ fontFamily: customFont.trim() }); }}><input bind:value={customFont} placeholder={t('subtitles.font_input_placeholder')} /><Button size="sm" type="submit" disabled={!customFont.trim()}>{t('subtitles.apply_font')}</Button></form>
        <fieldset><legend>{t('settings.subtitles.outline_label')}</legend>{#each ['outline', 'shadow', 'none'] as shadow}<label><input type="radio" name="subtitle-shadow" value={shadow} checked={subtitleStyleStore.current.textShadow === shadow} onchange={() => subtitleStyleStore.update({ textShadow: shadow as 'outline' | 'shadow' | 'none' })} />{shadow === 'outline' ? t('subtitles.outline_standard') : shadow === 'shadow' ? t('subtitles.shadow_soft_label') : t('subtitles.shadow_none_label')}</label>{/each}</fieldset>
      </div>
      <div class="preview-column">
        <div class="preview-header"><strong>{t('settings.subtitles.preview_title')}</strong><label><input type="checkbox" bind:checked={controlsVisible} />{t('subtitles.simulate_controls')}</label></div>
        <div class="preview-screen">
          <div class="preview-subtitle" style:bottom={`${subtitleStyleStore.current.bottomOffset + (controlsVisible ? 50 : 0)}px`} style:font-size={`${subtitleStyleStore.current.fontSizePercent / 100 * 20}px`} style:font-family={subtitleStyleStore.current.fontFamily} style:color={subtitleStyleStore.current.textColor} style:background={hexToRgba(subtitleStyleStore.current.bgColor, subtitleStyleStore.current.bgOpacity)}>{t('settings.subtitles.preview_text')}<br />{t('settings.subtitles.preview_subtext')}</div>
          {#if controlsVisible}<div class="mock-controls"><span></span><div>▶　04:15 / 12:40 <b>CC　⛶</b></div></div>{/if}
        </div>
      </div>
    </div>
  </SettingGroup>
</div>

<style>
  .subtitle-settings { display: grid; gap: var(--space-6); }
  .language-editor { width: 100%; display: grid; gap: var(--space-3); }
  .language-list { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
  .language-list button { flex: 0 0 auto; width: auto; min-height: 28px; padding: 0 10px; border: 1px solid var(--border-default); border-radius: 999px; background: var(--surface-2); color: var(--text-secondary); font: var(--type-caption); line-height: 1; white-space: nowrap; cursor: pointer; }
  .language-list button.selected { border-color: var(--accent); background: var(--accent-soft); color: var(--accent); }
  .language-editor form, .custom-font { display: flex; gap: var(--space-2); }
  input:not([type='checkbox']):not([type='radio']):not([type='range']):not([type='color']), select { min-height: var(--control-height); padding: 0 var(--space-3); border: 1px solid var(--border-default); border-radius: var(--radius-md); background: var(--surface-0); color: var(--text-primary); }
  .save-row { display: flex; align-items: center; justify-content: flex-end; gap: var(--space-2); padding: var(--space-3) var(--space-4); border-top: 1px solid var(--border-subtle); }
  .save-row > span { margin-right: auto; color: var(--text-muted); font: var(--type-caption); } .save-row > span.dirty { color: var(--warning); }
  .appearance-grid { display: grid; grid-template-columns: minmax(280px, .9fr) minmax(340px, 1.1fr); gap: clamp(36px, 4vw, 64px); padding: var(--space-5); }
  .appearance-controls { display: grid; gap: var(--space-4); }
  .appearance-controls > label { display: grid; grid-template-columns: 1fr auto; gap: var(--space-2); color: var(--text-primary); font: var(--type-label); }
  .appearance-controls output { color: var(--text-muted); font: var(--type-caption); } .appearance-controls input[type='range'], .appearance-controls select { grid-column: 1 / -1; width: 100%; }
  .two-columns { display: grid; grid-template-columns: 1fr 1fr; gap: var(--space-3); } .two-columns label { display: flex; align-items: center; justify-content: space-between; color: var(--text-secondary); font: var(--type-caption); }
  fieldset { display: flex; flex-wrap: wrap; gap: var(--space-3); margin: 0; padding: var(--space-3); border: 1px solid var(--border-default); border-radius: var(--radius-md); } fieldset legend { color: var(--text-primary); font: var(--type-label); } fieldset label { color: var(--text-secondary); font: var(--type-caption); }
  .preview-column { min-width: 0; display: grid; align-content: start; gap: var(--space-3); padding-left: var(--space-5); border-left: 1px solid var(--border-subtle); } .preview-header { display: flex; justify-content: space-between; gap: var(--space-3); color: var(--text-primary); font: var(--type-label); } .preview-header label { color: var(--text-muted); font: var(--type-caption); }
  .preview-screen { position: relative; min-height: 320px; overflow: hidden; border-radius: var(--radius-lg); background: radial-gradient(circle at 65% 25%, #42506a, #151922 55%, #080a0e); }
  .preview-subtitle { position: absolute; left: 50%; width: max-content; max-width: 85%; transform: translateX(-50%); padding: var(--space-2) var(--space-3); border-radius: var(--radius-sm); text-align: center; text-shadow: var(--sub-text-shadow); transition: bottom var(--motion-fast); }
  .mock-controls { position: absolute; inset: auto var(--space-4) var(--space-3); color: white; font: var(--type-caption); } .mock-controls > span { display: block; height: 3px; margin-bottom: var(--space-3); background: linear-gradient(90deg, var(--accent) 35%, #ffffff44 35%); } .mock-controls div { display: flex; justify-content: space-between; }
  @media (max-width: 960px) { .appearance-grid { grid-template-columns: 1fr; gap: var(--space-6); } .preview-column { padding-top: var(--space-5); padding-left: 0; border-top: 1px solid var(--border-subtle); border-left: 0; } }
  @media (max-width: 560px) { .save-row, .language-editor form, .custom-font { align-items: stretch; flex-direction: column; } .save-row > span { margin-right: 0; } .preview-screen { min-height: 240px; } }
</style>

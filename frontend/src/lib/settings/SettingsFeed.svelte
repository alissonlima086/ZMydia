<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../i18n/i18n.svelte';
  import Button from '../ui/Button.svelte';
  import Notice from '../ui/Notice.svelte';
  import SettingGroup from '../ui/SettingGroup.svelte';
  import SettingRow from '../ui/SettingRow.svelte';
  import { type HomeSettings, type RecommendationSettings, type TagSummary, messageFrom, request } from './types';

  const defaults: RecommendationSettings = { enabled: false, max_videos_per_channel: 3, boost_unwatched: true, unwatched_bonus: 5, fatigue_filter: true, serendipity_percent: 15 };
  let homeSettings = $state<HomeSettings>({ hidden_tags: [] });
  let recSettings = $state<RecommendationSettings>({ ...defaults });
  let savedRecSettings = $state<RecommendationSettings>({ ...defaults });
  let libraryTags = $state<TagSummary[]>([]);
  let newTagInput = $state('');
  let savingHomeSettings = $state(false);
  let savingRecSettings = $state(false);
  let feedback = $state<{ text: string; tone: 'success' | 'error' } | null>(null);
  let feedbackTimeout: number | null = null;

  const recDirty = $derived(JSON.stringify(recSettings) !== JSON.stringify(savedRecSettings));

  function showFeedback(text: string, tone: 'success' | 'error' = 'success') {
    feedback = { text, tone };
    if (feedbackTimeout) clearTimeout(feedbackTimeout);
    feedbackTimeout = window.setTimeout(() => (feedback = null), 4500);
  }

  onMount(() => { void Promise.all([loadHomeSettings(), loadRecSettings(), loadLibraryTags()]); });

  async function loadHomeSettings() { try { homeSettings = await request<HomeSettings>('/api/settings/home'); } catch {} }
  async function loadLibraryTags() { try { libraryTags = await request<TagSummary[]>('/api/tags'); } catch {} }
  async function loadRecSettings() {
    try {
      recSettings = await request<RecommendationSettings>('/api/settings/recommendation');
      savedRecSettings = { ...recSettings };
    } catch {}
  }

  async function saveRecSettings() {
    savingRecSettings = true;
    try {
      recSettings = await request<RecommendationSettings>('/api/settings/recommendation', { method: 'PUT', body: JSON.stringify(recSettings) });
      savedRecSettings = { ...recSettings };
      showFeedback(t('settings.feed.rec_saved'));
    } catch (error) { showFeedback(messageFrom(error), 'error'); }
    finally { savingRecSettings = false; }
  }

  async function addHiddenTag(event: SubmitEvent) {
    event.preventDefault();
    const tag = newTagInput.trim().replace(/^#/, '').toLowerCase();
    if (!tag) return;
    if (homeSettings.hidden_tags.some((item) => item.toLowerCase() === tag)) {
      showFeedback(t('settings.feed.already_hidden', { tag }), 'error'); return;
    }
    await updateHiddenTags([...homeSettings.hidden_tags, tag], t('settings.feed.hidden_feedback', { tag }));
    newTagInput = '';
  }

  async function removeHiddenTag(tag: string) {
    await updateHiddenTags(homeSettings.hidden_tags.filter((item) => item.toLowerCase() !== tag.toLowerCase()), t('settings.feed.visible_feedback', { tag }));
  }

  async function updateHiddenTags(hidden_tags: string[], success: string) {
    savingHomeSettings = true;
    try {
      homeSettings = await request<HomeSettings>('/api/settings/home', { method: 'PUT', body: JSON.stringify({ hidden_tags }) });
      showFeedback(success);
    } catch (error) { showFeedback(messageFrom(error), 'error'); }
    finally { savingHomeSettings = false; }
  }
</script>

<div class="feed-settings">
  {#if feedback}<Notice tone={feedback.tone}>{feedback.text}</Notice>{/if}

  <SettingGroup title={t('settings.feed.rec_card_title')} description={t('settings.feed.rec_card_desc')}>
    <SettingRow label={t('settings.feed.rec_enable_label')} description={t('settings.feed.rec_enable_sub')}>
      {#snippet control()}<label class="switch"><input type="checkbox" bind:checked={recSettings.enabled} /><span aria-hidden="true"></span></label>{/snippet}
    </SettingRow>
    {#if recSettings.enabled}
      <SettingRow label={t('settings.feed.rec_max_per_channel')} description={t('settings.feed.rec_max_per_channel_desc')}>
        {#snippet control()}<input class="number-input" type="number" min="0" max="20" bind:value={recSettings.max_videos_per_channel} aria-label={t('settings.feed.rec_max_per_channel')} />{/snippet}
      </SettingRow>
      <SettingRow label={t('settings.feed.rec_serendipity')} description={t('settings.feed.rec_serendipity_desc')}>
        {#snippet control()}<span class="number-suffix"><input class="number-input" type="number" min="0" max="50" bind:value={recSettings.serendipity_percent} aria-label={t('settings.feed.rec_serendipity')} /><span>%</span></span>{/snippet}
      </SettingRow>
      <SettingRow label={t('settings.feed.rec_boost_unwatched')} description={t('settings.feed.rec_boost_unwatched_desc')}>
        {#snippet control()}<label class="switch"><input type="checkbox" bind:checked={recSettings.boost_unwatched} /><span aria-hidden="true"></span></label>{/snippet}
      </SettingRow>
      <SettingRow label={t('settings.feed.rec_fatigue_filter')} description={t('settings.feed.rec_fatigue_filter_desc')}>
        {#snippet control()}<label class="switch"><input type="checkbox" bind:checked={recSettings.fatigue_filter} /><span aria-hidden="true"></span></label>{/snippet}
      </SettingRow>
    {/if}
    <div class="save-row">
      <span class:dirty={recDirty}>{recDirty ? t('common.unsaved_changes') : t('common.preferences_saved')}</span>
      <Button variant="primary" onclick={saveRecSettings} disabled={!recDirty || savingRecSettings}>{savingRecSettings ? t('settings.feed.saving') : t('settings.feed.rec_save_btn')}</Button>
    </div>
  </SettingGroup>

  <SettingGroup title={t('settings.feed.title')} description={t('settings.feed.card_desc')}>
    <form class="filter-form" onsubmit={addHiddenTag}>
      <label for="home-tag-input">{t('settings.feed.input_label')}</label>
      <div class="filter-input-row">
        <span class="hash-input"><span>#</span><input id="home-tag-input" list="available-library-tags" bind:value={newTagInput} placeholder={t('settings.feed.input_placeholder')} autocomplete="off" spellcheck="false" disabled={savingHomeSettings} /></span>
        <Button type="submit" variant="secondary" disabled={savingHomeSettings || !newTagInput.trim()}>{savingHomeSettings ? t('settings.feed.saving') : t('settings.feed.hide_btn')}</Button>
      </div>
      <datalist id="available-library-tags">{#each libraryTags as tag}<option value={tag.display_name}>{tag.display_name}</option>{/each}</datalist>
    </form>
    <div class="tags-area">
      <strong>{t('settings.feed.active_words_count', { count: homeSettings.hidden_tags.length })}</strong>
      {#if homeSettings.hidden_tags.length === 0}
        <p>{t('settings.feed.no_tags')}</p>
      {:else}
        <div class="tags-list">
          {#each homeSettings.hidden_tags as tag}
            <span class="tag">#{tag}<button type="button" onclick={() => removeHiddenTag(tag)} disabled={savingHomeSettings} aria-label={t('settings.feed.remove_tag_aria', { tag })}><span class="material-symbols-outlined" aria-hidden="true">close</span></button></span>
          {/each}
        </div>
      {/if}
    </div>
  </SettingGroup>
</div>

<style>
  .feed-settings { display: grid; gap: var(--space-6); }
  .switch { display: inline-flex; cursor: pointer; }
  .switch input { position: absolute; opacity: 0; pointer-events: none; }
  .switch > span { width: 42px; height: 24px; padding: 3px; border-radius: 999px; background: var(--surface-3); transition: background var(--motion-fast); }
  .switch > span::after { content: ''; display: block; width: 18px; height: 18px; border-radius: 50%; background: var(--text-muted); transition: transform var(--motion-fast), background var(--motion-fast); }
  .switch input:checked + span { background: var(--accent); }
  .switch input:checked + span::after { transform: translateX(18px); background: white; }
  .switch input:focus-visible + span { outline: 2px solid var(--accent); outline-offset: 2px; }
  .number-input { width: 78px; min-height: 36px; padding: 0 var(--space-2); border: 1px solid var(--border-default); border-radius: var(--radius-md); background: var(--surface-0); color: var(--text-primary); }
  .number-suffix { display: inline-flex; align-items: center; gap: var(--space-2); color: var(--text-muted); }
  .save-row { display: flex; align-items: center; justify-content: flex-end; gap: var(--space-4); padding: var(--space-3) var(--space-4); border-top: 1px solid var(--border-subtle); }
  .save-row > span { margin-right: auto; color: var(--text-muted); font: var(--type-caption); }
  .save-row > span.dirty { color: var(--warning); }
  .filter-form, .tags-area { display: grid; gap: var(--space-3); padding: var(--space-4); }
  .filter-form label, .tags-area strong { color: var(--text-primary); font: var(--type-label); }
  .filter-input-row { display: flex; gap: var(--space-2); }
  .hash-input { flex: 1; display: flex; align-items: center; min-width: 0; border: 1px solid var(--border-default); border-radius: var(--radius-md); background: var(--surface-0); color: var(--text-muted); }
  .hash-input > span { padding-left: var(--space-3); }
  .hash-input input { flex: 1; min-width: 0; min-height: var(--control-height); padding: 0 var(--space-3) 0 var(--space-1); border: 0; outline: 0; background: transparent; color: var(--text-primary); }
  .tags-area { border-top: 1px solid var(--border-subtle); }
  .tags-area p { margin: 0; color: var(--text-muted); font: var(--type-caption); }
  .tags-list { display: flex; flex-wrap: wrap; gap: var(--space-2); }
  .tag { display: inline-flex; align-items: center; gap: var(--space-1); min-height: 30px; padding-left: var(--space-3); border: 1px solid var(--border-default); border-radius: 999px; color: var(--text-secondary); font: var(--type-caption); }
  .tag button { width: 30px; height: 30px; border: 0; border-radius: 50%; background: transparent; color: var(--text-muted); cursor: pointer; }
  .tag button:hover { background: var(--overlay); color: var(--text-primary); }
  .tag .material-symbols-outlined { font-size: 16px; }
  @media (max-width: 560px) { .filter-input-row, .save-row { align-items: stretch; flex-direction: column; } .save-row > span { margin-right: 0; } }
</style>

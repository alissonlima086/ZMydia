<script lang="ts">
  import type { Component } from 'svelte';
  import { t } from '../i18n/i18n.svelte';
  import PageState from './PageState.svelte';

  type LoadedComponent = { default: Component<any> };

  let {
    loader,
    props = {},
    loadingLabel = t('common.page_loading')
  }: {
    loader: () => Promise<LoadedComponent>;
    props?: Record<string, any>;
    loadingLabel?: string;
  } = $props();

  const pending = $derived(loader());
</script>

{#await pending}
  <PageState kind="loading" title={loadingLabel} />
{:then module}
  {@const ResolvedPage = module.default}
  <ResolvedPage {...props} />
{:catch}
  <PageState
    kind="error"
    icon="error"
    title={t('common.page_load_error')}
    description={t('common.page_load_error_hint')}
  />
{/await}

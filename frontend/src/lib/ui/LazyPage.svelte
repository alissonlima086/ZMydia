<script lang="ts">
  import type { Component } from 'svelte';
  import PageState from './PageState.svelte';

  type LoadedComponent = { default: Component<any> };

  let {
    loader,
    props = {},
    loadingLabel = 'Carregando página…'
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
    title="Não foi possível abrir esta página"
    description="Atualize a página para tentar novamente."
  />
{/await}

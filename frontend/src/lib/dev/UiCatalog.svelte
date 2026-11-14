<script lang="ts">
  import Button from '../ui/Button.svelte';
  import PageHeader from '../ui/PageHeader.svelte';
  import PageShell from '../ui/PageShell.svelte';
  import PageState from '../ui/PageState.svelte';
  import Section from '../ui/Section.svelte';
</script>

<svelte:head><title>ZMydia · Catálogo de interface</title></svelte:head>

{#snippet catalogActions()}
  <Button variant="secondary" size="sm">Ação secundária</Button>
  <Button variant="primary" size="sm">Nova playlist</Button>
{/snippet}

<div class="catalog-frame">
  <PageShell width="wide">
    <PageHeader
      eyebrow="Sprint 8 · referência interna"
      title="Sistema visual ZMydia"
      description="Tokens e componentes compartilhados usados nas páginas principais."
      actions={catalogActions}
    />

    <Section title="Ações" description="Hierarquia, tamanhos e estados interativos.">
      <div class="button-row">
        <Button variant="primary">Primária</Button>
        <Button variant="secondary">Secundária</Button>
        <Button variant="ghost">Discreta</Button>
        <Button variant="danger">Destrutiva</Button>
        <Button variant="secondary" disabled>Indisponível</Button>
      </div>
    </Section>

    <Section title="Superfícies e conteúdo" description="Cartão de mídia e controles de formulário.">
      <div class="surface-grid">
        <article class="sample-card">
          <div class="sample-card__art" aria-hidden="true">
            <span class="material-symbols-outlined">play_circle</span>
          </div>
          <div>
            <p class="sample-card__eyebrow">Recomendado para você</p>
            <h3>Starlight · apresentação ao vivo</h3>
            <p class="sample-card__meta">Canal de exemplo · 8:42</p>
          </div>
        </article>

        <form class="sample-form" onsubmit={(event) => event.preventDefault()}>
          <label for="catalog-search">Buscar na biblioteca</label>
          <div class="sample-search">
            <span class="material-symbols-outlined" aria-hidden="true">search</span>
            <input id="catalog-search" placeholder="Artista, canal ou título" />
          </div>
          <p>Pressione Tab para conferir o foco visível.</p>
        </form>
      </div>
    </Section>

    <Section title="Estados de página" description="Loading, vazio e erro seguem a mesma anatomia.">
      <div class="state-grid">
        <PageState kind="loading" title="Carregando vídeos…" />
        <PageState kind="empty" icon="video_library" title="Nada por aqui" description="Os novos itens aparecerão neste espaço." />
        <PageState kind="error" icon="error" title="Falha ao carregar" description="Tente novamente em alguns instantes." />
      </div>
    </Section>

    <Section title="Tokens" description="Escala semântica de cor e espaçamento.">
      <div class="token-grid" aria-label="Amostras de cores">
        <div class="token token--accent"><span>accent</span></div>
        <div class="token token--surface-1"><span>surface-1</span></div>
        <div class="token token--surface-2"><span>surface-2</span></div>
        <div class="token token--success"><span>success</span></div>
        <div class="token token--warning"><span>warning</span></div>
        <div class="token token--danger"><span>danger</span></div>
      </div>
    </Section>
  </PageShell>
</div>

<style>
  .catalog-frame {
    min-height: 100vh;
    padding: var(--space-8);
    background: var(--canvas);
    color: var(--text-primary);
  }
  .button-row { display: flex; flex-wrap: wrap; gap: var(--space-3); }
  .surface-grid { display: grid; grid-template-columns: minmax(0, 1.4fr) minmax(280px, 1fr); gap: var(--space-4); }
  .sample-card, .sample-form {
    padding: var(--space-4);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-lg);
    background: var(--surface-1);
  }
  .sample-card { display: grid; grid-template-columns: 160px 1fr; align-items: center; gap: var(--space-4); }
  .sample-card__art {
    aspect-ratio: 16 / 9;
    display: grid;
    place-items: center;
    border-radius: var(--radius-md);
    background: linear-gradient(135deg, color-mix(in srgb, var(--accent) 36%, var(--surface-2)), var(--overlay));
  }
  .sample-card__art span { font-size: 40px; }
  .sample-card__eyebrow { margin: 0 0 var(--space-1); color: var(--accent); font: var(--type-caption); text-transform: uppercase; }
  h3 { margin: 0; font: var(--type-section-title); }
  .sample-card__meta, .sample-form p { margin: var(--space-2) 0 0; color: var(--text-muted); font: var(--type-caption); }
  .sample-form { display: flex; flex-direction: column; justify-content: center; gap: var(--space-2); }
  .sample-form label { font: var(--type-label); }
  .sample-search {
    min-height: var(--control-height);
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0 var(--space-3);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    background: var(--canvas);
  }
  .sample-search input { min-width: 0; flex: 1; border: 0; outline: 0; background: transparent; color: var(--text-primary); font: var(--type-body); }
  .state-grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: var(--space-4); }
  .state-grid :global(.ui-page-state) { min-height: 220px; }
  .token-grid { display: grid; grid-template-columns: repeat(6, minmax(100px, 1fr)); gap: var(--space-3); }
  .token { min-height: 96px; display: flex; align-items: end; padding: var(--space-3); border: 1px solid var(--border-subtle); border-radius: var(--radius-md); }
  .token span { padding: var(--space-1) var(--space-2); border-radius: var(--radius-sm); background: rgb(0 0 0 / 60%); color: white; font: var(--type-caption); }
  .token--accent { background: var(--accent); }
  .token--surface-1 { background: var(--surface-1); }
  .token--surface-2 { background: var(--surface-2); }
  .token--success { background: var(--success); }
  .token--warning { background: var(--warning); }
  .token--danger { background: var(--danger); }
  @media (max-width: 800px) {
    .catalog-frame { padding: var(--space-4); }
    .surface-grid, .state-grid { grid-template-columns: 1fr; }
    .token-grid { grid-template-columns: repeat(3, 1fr); }
  }
  @media (max-width: 480px) {
    .sample-card { grid-template-columns: 1fr; }
    .token-grid { grid-template-columns: repeat(2, 1fr); }
  }
</style>

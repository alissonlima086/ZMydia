/**
 * ZMydia Downloader - YouTube Content Script
 * Injeta o botão nativo tanto na página de vídeo tradicional (/watch)
 * quanto na página de Shorts (/shorts/<id>) no lado esquerdo do vídeo.
 * Arquitetura baseada em Reconciliador Contínuo Idempotente (padrão SponsorBlock / Return YouTube Dislike).
 */

(function () {
  'use strict';

  const DOWNLOAD_ICON_SVG = `
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
      <polyline points="7 10 12 15 17 10"></polyline>
      <line x1="12" y1="15" x2="12" y2="3"></line>
    </svg>
  `;

  const SHORTS_DOWNLOAD_ICON_SVG = `
    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
      <polyline points="7 10 12 15 17 10"></polyline>
      <line x1="12" y1="15" x2="12" y2="3"></line>
    </svg>
  `;

  const SPINNER_SVG = `
    <svg class="mymedia-spinner" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3">
      <circle cx="12" cy="12" r="10" stroke-opacity="0.25"></circle>
      <path d="M12 2a10 10 0 0 1 10 10" stroke="#ff5b43"></path>
    </svg>
  `;

  const CHECK_SVG = `
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#22c55e" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
      <polyline points="20 6 9 17 4 12"></polyline>
    </svg>
  `;

  let appObserver = null;
  let rafId = null;
  let lastDiagnosticState = null;

  function collectOpenRoots(startRoot = document) {
    const roots = [startRoot];
    for (let index = 0; index < roots.length; index += 1) {
      const root = roots[index];
      for (const element of root.querySelectorAll('*')) {
        if (element.shadowRoot && !roots.includes(element.shadowRoot)) {
          roots.push(element.shadowRoot);
        }
      }
    }
    return roots;
  }

  function queryAllDeep(selector, startRoot = document) {
    return collectOpenRoots(startRoot).flatMap((root) => [...root.querySelectorAll(selector)]);
  }

  function queryDeep(selector, startRoot = document) {
    return queryAllDeep(selector, startRoot)[0] || null;
  }

  function setDiagnosticState(state, detail = '') {
    const root = document.documentElement;
    if (root) {
      root.dataset.mymediaExtension = 'loaded';
      root.dataset.mymediaShortsState = state;
      root.dataset.mymediaShortsDetail = detail;
    }
    const signature = `${state}:${detail}`;
    if (signature !== lastDiagnosticState) {
      lastDiagnosticState = signature;
      console.info('[ZMydia][Shorts diagnostics]', { state, detail, url: window.location.href });
    }
  }

  function parseSvg(svgText) {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svgText.trim(), 'image/svg+xml');
    return document.importNode(doc.documentElement, true);
  }

  function setWatchButtonDefaultContent(btn) {
    btn.replaceChildren();

    const iconSpan = document.createElement('span');
    iconSpan.className = 'mymedia-btn-icon';
    iconSpan.appendChild(parseSvg(DOWNLOAD_ICON_SVG));

    const labelSpan = document.createElement('span');
    labelSpan.className = 'mymedia-btn-label';
    labelSpan.textContent = 'ZMydia';

    btn.appendChild(iconSpan);
    btn.appendChild(labelSpan);
  }

  function setShortsButtonDefaultContent(btn) {
    btn.replaceChildren();

    const iconSpan = document.createElement('span');
    iconSpan.className = 'mymedia-shorts-icon';
    iconSpan.appendChild(parseSvg(SHORTS_DOWNLOAD_ICON_SVG));

    btn.appendChild(iconSpan);
  }

  function restoreButton(buttonEl) {
    if (buttonEl.classList.contains('mymedia-shorts-action-btn')) {
      setShortsButtonDefaultContent(buttonEl);
      buttonEl.title = 'Baixar este Short no ZMydia';
    } else {
      setWatchButtonDefaultContent(buttonEl);
      buttonEl.title = 'Baixar este vídeo no ZMydia';
    }
  }

  // Extrai o ID do vídeo atual a partir da URL (/watch?v=... ou /shorts/...)
  function getVideoId(urlStr) {
    try {
      const url = new URL(urlStr || window.location.href);
      if (url.pathname === '/watch') {
        return url.searchParams.get('v') || null;
      }
      if (url.pathname.startsWith('/shorts/')) {
        const parts = url.pathname.split('/shorts/');
        if (parts[1]) {
          return parts[1].split('/')[0].split('?')[0] || null;
        }
      }
    } catch {
      // fallback
    }
    return null;
  }

  // Localiza o container ativo de ações do vídeo watch (/watch)
  function findWatchActionsContainer() {
    const watchRoots = [
      'ytd-watch-flexy:not([hidden])',
      'ytd-watch-grid:not([hidden])',
      'ytd-watch-flexy',
      'ytd-watch-grid'
    ];

    const subSelectors = [
      'ytd-watch-metadata #actions #top-level-buttons-computed',
      'ytd-watch-metadata #actions-inner #top-level-buttons-computed',
      'ytd-watch-metadata #top-level-buttons-computed',
      '#actions #top-level-buttons-computed',
      '#menu #top-level-buttons-computed',
      '#top-level-buttons-computed'
    ];

    for (const rootSel of watchRoots) {
      const rootEl = document.querySelector(rootSel);
      if (!rootEl) continue;
      if (rootEl.hasAttribute('hidden') || rootEl.style.display === 'none') continue;

      for (const subSel of subSelectors) {
        const container = rootEl.querySelector(subSel);
        if (container && !container.closest('[hidden]')) {
          return container;
        }
      }
    }

    for (const subSel of subSelectors) {
      const container = document.querySelector(subSel);
      if (container && !container.closest('[hidden]')) {
        return container;
      }
    }

    return null;
  }

  // Localiza o renderer ativo do YouTube Shorts (/shorts/...)
  function findActiveShortsRenderer() {
    const rendererSelectors = [
      'ytd-reel-video-renderer',
      'ytd-shorts-video-renderer',
      'yt-reel-video-renderer',
      'yt-shorts-video-renderer',
      '.reel-video-in-sequence-new',
      '[class*="reel-video-renderer"]'
    ];
    const rendererSelector = rendererSelectors.join(',');

    // O YouTube moveu partes do Shorts para shadow roots em algumas variantes.
    const activeSelector = rendererSelectors
      .flatMap((selector) => [`${selector}[is-active]`, `${selector}[active]`])
      .join(',');
    const active = queryDeep(activeSelector);
    if (active) return active;

    // Fallback de visibilidade: escolhe o renderer que cruza o centro da tela.
    const allReels = queryAllDeep(rendererSelector);
    for (const reel of allReels) {
      if (reel.hasAttribute('hidden') || reel.style.display === 'none') continue;
      const rect = reel.getBoundingClientRect();
      if (rect.top <= window.innerHeight / 2 && rect.bottom >= window.innerHeight / 2) {
        return reel;
      }
    }

    return allReels[0] || null;
  }

  // Injeta o botão estilo pílula na barra de ações do watch
  function injectWatchButton(container, currentVideoId) {
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'mymedia-watch-pill-btn mymedia-download-trigger';
    btn.title = 'Baixar este vídeo no ZMydia';
    btn.setAttribute('aria-label', 'Baixar este vídeo no ZMydia');
    if (currentVideoId) {
      btn.dataset.videoId = currentVideoId;
    }

    setWatchButtonDefaultContent(btn);

    btn.addEventListener('click', (e) => {
      e.preventDefault();
      e.stopPropagation();

      const videoUrl = window.location.href;
      const titleEl = document.querySelector('h1.ytd-watch-metadata, #container h1 yt-formatted-string, h1 yt-formatted-string');
      const title = titleEl ? titleEl.textContent.trim() : document.title.replace(/ - YouTube$/, '').trim();

      handleClick({
        buttonEl: btn,
        videoUrl,
        title
      });
    });

    container.appendChild(btn);
    console.log('[ZMydia] ZMydia button injected into watch video:', window.location.href);
  }

  function isVisibleShortsElement(element) {
    if (!element || element.closest('[hidden]')) return false;
    const style = window.getComputedStyle(element);
    if (style.display === 'none' || style.visibility === 'hidden') return false;
    const rect = element.getBoundingClientRect();
    return rect.width > 0
      && rect.height > 0
      && rect.bottom > 0
      && rect.top < window.innerHeight;
  }

  function findShortsActionsContainer(renderer) {
    const selectors = [
      'yt-reel-player-overlay-view-model reel-action-bar-view-model',
      '.ytReelPlayerOverlayViewModelActionsContainer reel-action-bar-view-model',
      'reel-action-bar-view-model.ytwReelActionBarViewModelHost',
      'reel-action-bar-view-model',
      '#button-bar',
      '.ytReelPlayerOverlayViewModelActionsContainer',
      'ytd-reel-player-overlay-renderer #actions',
      'ytd-reel-player-overlay-renderer .action-buttons',
      '#actions',
      '#action-buttons',
      '.action-buttons',
      '.reel-player-overlay-actions',
      '[class*="reel-player-overlay-actions"]'
    ];
    for (const selector of selectors) {
      const candidate = queryDeep(selector, renderer);
      if (isVisibleShortsElement(candidate)) return candidate;
    }

    // O YouTube também monta a coluna fora do renderer ativo em algumas
    // variantes do layout desktop. Escolhe a coluna visível mais próxima do
    // centro vertical da viewport para não atingir um Short pré-carregado.
    const globalSelectors = [
      '.reel-video-in-sequence-new yt-reel-player-overlay-view-model reel-action-bar-view-model',
      'yt-reel-player-overlay-view-model reel-action-bar-view-model',
      '.ytReelPlayerOverlayViewModelActionsContainer reel-action-bar-view-model',
      'reel-action-bar-view-model.ytwReelActionBarViewModelHost',
      'reel-action-bar-view-model',
      '#button-bar > reel-action-bar-view-model',
      '.ytReelPlayerOverlayViewModelActionsContainer',
      'ytd-reel-player-overlay-renderer #actions',
      'ytd-reel-player-overlay-renderer .action-buttons',
      'ytd-shorts-video-renderer #actions',
      'ytd-shorts-video-renderer .action-buttons',
      'ytd-shorts #actions',
      '#shorts-container #actions',
      'ytd-reel-video-renderer #actions',
      'ytd-reel-video-renderer .action-buttons',
      '.reel-player-overlay-actions',
      '[class*="reel-player-overlay-actions"]'
    ];
    const candidates = queryAllDeep(globalSelectors.join(','))
      .filter(isVisibleShortsElement)
      .sort((a, b) => {
        const aRect = a.getBoundingClientRect();
        const bRect = b.getBoundingClientRect();
        const center = window.innerHeight / 2;
        return Math.abs((aRect.top + aRect.bottom) / 2 - center)
          - Math.abs((bRect.top + bRect.bottom) / 2 - center);
      });
    if (candidates.length > 0) {
      return candidates[0];
    }
    return null;
  }

  // Injeta a ação dentro da coluna nativa do Short.
  function injectShortsButton(targetContainer, currentVideoId) {
    const targetRoot = targetContainer.getRootNode();
    if (targetRoot instanceof ShadowRoot && !targetRoot.querySelector('#mymedia-shorts-shadow-styles')) {
      const style = document.createElement('style');
      style.id = 'mymedia-shorts-shadow-styles';
      style.textContent = `
        .mymedia-shorts-action-wrapper { display:flex!important; flex-direction:column!important; align-items:center!important; gap:6px!important; width:64px!important; margin:0 auto 12px!important; pointer-events:auto!important; }
        .mymedia-shorts-action-btn { display:flex!important; align-items:center!important; justify-content:center!important; width:48px!important; height:48px!important; min-width:48px!important; min-height:48px!important; padding:0!important; border-radius:50%!important; border:1px solid rgba(255,255,255,.16)!important; background:rgba(0,0,0,.6)!important; color:#fff!important; cursor:pointer!important; box-sizing:border-box!important; }
        .mymedia-shorts-action-btn:hover { background:#ff5b43!important; border-color:#ff5b43!important; }
        .mymedia-shorts-icon { display:flex!important; align-items:center!important; justify-content:center!important; }
        .mymedia-shorts-action-label { font:500 11.5px Roboto,Arial,sans-serif!important; color:#f1f1f1!important; text-shadow:0 1px 3px rgba(0,0,0,.85)!important; }
      `;
      targetRoot.appendChild(style);
    }

    const wrapper = document.createElement('div');
    wrapper.className = 'mymedia-shorts-action-wrapper';
    if (currentVideoId) {
      wrapper.dataset.videoId = currentVideoId;
    }

    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'mymedia-shorts-action-btn mymedia-download-trigger';
    btn.title = 'Baixar este Short no ZMydia';
    btn.setAttribute('aria-label', 'Baixar este Short no ZMydia');

    setShortsButtonDefaultContent(btn);

    const label = document.createElement('span');
    label.className = 'mymedia-shorts-action-label';
    label.textContent = 'Baixar';

    wrapper.appendChild(btn);
    wrapper.appendChild(label);

    btn.addEventListener('click', (e) => {
      e.preventDefault();
      e.stopPropagation();

      const videoUrl = window.location.href;
      const titleEl = targetContainer.closest('ytd-reel-video-renderer, .reel-video-in-sequence-new')?.querySelector('h2.title, yt-formatted-string.title, #overlay yt-formatted-string, yt-reel-metapanel-view-model [role="heading"]')
        || document.querySelector('ytd-reel-video-renderer[is-active] h2');
      const title = titleEl ? titleEl.textContent.trim() : document.title.replace(/ - YouTube$/, '').trim();

      handleClick({
        buttonEl: btn,
        videoUrl,
        title
      });
    });

    // O final da coluna fica abaixo da viewport em telas mais baixas. Manter o
    // ZMydia no topo garante que ele permaneça visível sem sobrepor o vídeo.
    targetContainer.prepend(wrapper);
    console.log('[ZMydia] Action inserted into the native Shorts column:', window.location.href);
  }

  // Reconciliador contínuo e idempotente: monitora rota e integridade visual
  function reconcile() {
    const isWatch = window.location.pathname.startsWith('/watch');
    const isShorts = window.location.pathname.startsWith('/shorts/');

    // Se estiver fora do watch e shorts, remove quaisquer botões órfãos
    if (!isWatch && !isShorts) {
      queryAllDeep('.mymedia-watch-pill-btn, .mymedia-shorts-action-wrapper').forEach((el) => el.remove());
      setDiagnosticState('idle');
      return;
    }

    const currentVideoId = getVideoId(window.location.href);

    // 1. Caso: Página de Vídeo Tradicional (/watch)
    if (isWatch) {
      document.querySelectorAll('.mymedia-shorts-action-wrapper').forEach((el) => el.remove());

      const container = findWatchActionsContainer();
      if (!container) return;

      const existingInContainer = container.querySelector('.mymedia-watch-pill-btn');
      if (existingInContainer) {
        if (currentVideoId && existingInContainer.dataset.videoId === currentVideoId) {
          return;
        }
        existingInContainer.remove();
      }

      document.querySelectorAll('.mymedia-watch-pill-btn').forEach((b) => {
        if (b !== existingInContainer) b.remove();
      });

      injectWatchButton(container, currentVideoId);
      return;
    }

    // 2. Caso: Página do YouTube Shorts (/shorts/<id>)
    if (isShorts) {
      queryAllDeep('.mymedia-watch-pill-btn').forEach((el) => el.remove());

      const activeReel = findActiveShortsRenderer();
      if (!activeReel) {
        setDiagnosticState('renderer-not-found');
        return;
      }

      const targetMount = findShortsActionsContainer(activeReel);
      if (!targetMount) {
        setDiagnosticState('actions-not-found', activeReel.localName);
        return;
      }
      const existingWrapper = queryDeep('.mymedia-shorts-action-wrapper', targetMount);

      if (existingWrapper) {
        if (currentVideoId && existingWrapper.dataset.videoId === currentVideoId) {
          setDiagnosticState('injected', targetMount.localName);
          return;
        }
        existingWrapper.remove();
      }

      // Remove wrappers órfãos de outros reels
      queryAllDeep('.mymedia-shorts-action-wrapper').forEach((w) => {
        if (w !== existingWrapper) w.remove();
      });

      injectShortsButton(targetMount, currentVideoId);
      setDiagnosticState('injected', targetMount.localName);
    }
  }

  // Agendador de reconciliação via requestAnimationFrame
  function scheduleReconcile() {
    if (rafId) return;
    rafId = requestAnimationFrame(() => {
      rafId = null;
      reconcile();
    });
  }

  // Inicia observador de mutações contínuo
  function startContinuousObserver() {
    if (appObserver) return;

    appObserver = new MutationObserver(() => {
      scheduleReconcile();
    });

    const targetRoot = document.querySelector('ytd-app') || document.body || document.documentElement;
    appObserver.observe(targetRoot, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ['hidden', 'is-active']
    });
  }

  // Tratamento do clique com verificação de preferência de 1-clique
  async function handleClick({ buttonEl, videoUrl, title }) {
    let config = null;
    try {
      config = await browser.runtime.sendMessage({ action: 'GET_CONFIG' });
    } catch (err) {
      console.warn('[ZMydia] Failed to read settings:', err);
    }

    if (config?.skipConfirmation) {
      await executeDownload({
        buttonEl,
        videoUrl,
        title,
        container: config.defaultContainer || 'mkv',
        quality: config.defaultQuality || 'best'
      });
    } else {
      if (window.MyMediaBalloon) {
        window.MyMediaBalloon.show({
          anchorEl: buttonEl,
          videoUrl,
          title,
          onConfirm: (opts) => {
            void executeDownload({
              buttonEl,
              videoUrl,
              title,
              container: opts.container,
              quality: opts.quality
            });
          }
        });
      } else {
        await executeDownload({
          buttonEl,
          videoUrl,
          title,
          container: 'mkv',
          quality: 'best'
        });
      }
    }
  }

  // Executa o envio para o worker de download em segundo plano
  async function executeDownload({ buttonEl, videoUrl, title, container, quality }) {
    buttonEl.classList.add('loading');
    buttonEl.disabled = true;
    buttonEl.setAttribute('aria-busy', 'true');
    buttonEl.replaceChildren(parseSvg(SPINNER_SVG));

    try {
      const res = await browser.runtime.sendMessage({
        action: 'START_DOWNLOAD',
        payload: {
          url: videoUrl,
          title,
          container,
          quality
        }
      });

      if (res && res.ok) {
        buttonEl.classList.remove('loading');
        buttonEl.classList.add('success');
        buttonEl.replaceChildren(parseSvg(CHECK_SVG));
        buttonEl.title = 'Download iniciado no ZMydia!';
        buttonEl.setAttribute('aria-label', 'Download iniciado no ZMydia');

        setTimeout(() => {
          buttonEl.classList.remove('success');
          buttonEl.disabled = false;
          buttonEl.removeAttribute('aria-busy');
          restoreButton(buttonEl);
        }, 3000);
      } else {
        throw new Error(res?.error || 'Falha ao iniciar download');
      }
    } catch (err) {
      buttonEl.classList.remove('loading');
      buttonEl.classList.add('error');
      buttonEl.replaceChildren();
      buttonEl.textContent = '✕';
      buttonEl.title = `Erro: ${err.message || 'Falha na conexão com ZMydia'}`;
      buttonEl.setAttribute('aria-label', buttonEl.title);

      setTimeout(() => {
        buttonEl.classList.remove('error');
        buttonEl.disabled = false;
        buttonEl.removeAttribute('aria-busy');
        restoreButton(buttonEl);
      }, 4000);
    }
  }

  function init() {
    setDiagnosticState('loaded');
    scheduleReconcile();
    startContinuousObserver();

    try {
      browser.runtime.onMessage.addListener((message) => {
        if (message?.action === 'YT_SPA_NAVIGATE') {
          scheduleReconcile();
        }
      });
    } catch {}

    ['yt-navigate-finish', 'yt-page-data-updated'].forEach((evt) => {
      document.addEventListener(evt, scheduleReconcile, true);
      window.addEventListener(evt, scheduleReconcile, true);
    });
    window.addEventListener('popstate', scheduleReconcile);

    // Watcher de URL para mudanças de rota no YouTube SPA
    let lastUrl = window.location.href;
    setInterval(() => {
      const currentUrl = window.location.href;
      if (currentUrl !== lastUrl) {
        lastUrl = currentUrl;
        scheduleReconcile();
      }
    }, 150);

    // MutationObserver não atravessa shadow roots. Esta verificação lenta cobre
    // componentes criados depois do carregamento sem gerar polling agressivo.
    setInterval(scheduleReconcile, 1000);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
})();

/**
 * ZMydia Downloader - Floating Confirmation Balloon
 * Exibe um popover rápido próximo ao botão para confirmação de formato/qualidade
 * com opção de salvar como padrão para downloads em 1 clique.
 */

window.MyMediaBalloon = (function () {
  let activeBalloon = null;
  let repositionHandler = null;

  const DOWNLOAD_HEADER_SVG = `
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="#ff5b43" stroke-width="2.5">
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
      <polyline points="7 10 12 15 17 10"></polyline>
      <line x1="12" y1="15" x2="12" y2="3"></line>
    </svg>
  `;

  const ACTION_BTN_SVG = `
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
      <polyline points="7 10 12 15 17 10"></polyline>
      <line x1="12" y1="15" x2="12" y2="3"></line>
    </svg>
  `;

  function parseSvg(svgText) {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svgText.trim(), 'image/svg+xml');
    return document.importNode(doc.documentElement, true);
  }

  function close() {
    if (repositionHandler) {
      window.removeEventListener('resize', repositionHandler);
      window.removeEventListener('scroll', repositionHandler, true);
      repositionHandler = null;
    }
    if (activeBalloon) {
      activeBalloon.remove();
      activeBalloon = null;
    }
  }

  // Fecha ao clicar fora
  document.addEventListener('click', (e) => {
    if (activeBalloon && !activeBalloon.contains(e.target) && !e.target.closest('.mymedia-download-trigger')) {
      close();
    }
  });

  // Fecha ao apertar Esc
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') close();
  });

  function show({ anchorEl, videoUrl, title, onConfirm }) {
    close();

    // Lê configurações atuais para preencher os padrões
    browser.runtime.sendMessage({ action: 'GET_CONFIG' })
      .then((config) => {
        render({ anchorEl, videoUrl, title, config, onConfirm });
      })
      .catch((err) => {
        console.warn('[ZMydia] Failed to load balloon settings:', err);
        render({ anchorEl, videoUrl, title, config: null, onConfirm });
      });
  }

  function render({ anchorEl, videoUrl, title, config, onConfirm }) {
    const balloon = document.createElement('div');
    balloon.id = 'mymedia-download-balloon';
    balloon.className = 'mymedia-balloon';
    balloon.setAttribute('role', 'dialog');
    balloon.setAttribute('aria-label', 'Opções de download do ZMydia');

    let selectedFormat = config?.defaultContainer || 'mkv';
    let selectedQuality = config?.defaultQuality || 'best';

    // Header
    const header = document.createElement('div');
    header.className = 'mymedia-balloon-header';

    const headerTitle = document.createElement('div');
    headerTitle.className = 'mymedia-balloon-title';
    headerTitle.appendChild(parseSvg(DOWNLOAD_HEADER_SVG));
    const headerTitleText = document.createElement('strong');
    headerTitleText.textContent = 'Baixar no ZMydia';
    headerTitle.appendChild(headerTitleText);

    const closeBtn = document.createElement('button');
    closeBtn.type = 'button';
    closeBtn.className = 'mymedia-balloon-close';
    closeBtn.title = 'Fechar';
    closeBtn.textContent = '✕';
    closeBtn.addEventListener('click', close);

    header.appendChild(headerTitle);
    header.appendChild(closeBtn);
    balloon.appendChild(header);

    // Título do Vídeo (se presente)
    if (title) {
      const titleDiv = document.createElement('div');
      titleDiv.className = 'mymedia-balloon-video-title';
      titleDiv.title = title;
      titleDiv.textContent = title;
      balloon.appendChild(titleDiv);
    }

    // Container de Opções
    const optionsDiv = document.createElement('div');
    optionsDiv.className = 'mymedia-balloon-options';

    // Campo: Formato
    const formatField = document.createElement('div');
    formatField.className = 'mymedia-balloon-field';
    const formatLabel = document.createElement('label');
    formatLabel.textContent = 'Formato';
    formatField.appendChild(formatLabel);

    const pillsDiv = document.createElement('div');
    pillsDiv.className = 'mymedia-format-pills';

    const mkvBtn = document.createElement('button');
    mkvBtn.type = 'button';
    mkvBtn.className = `mymedia-pill-btn ${selectedFormat === 'mkv' ? 'active' : ''}`;
    mkvBtn.dataset.format = 'mkv';
    mkvBtn.textContent = 'MKV';

    const mp4Btn = document.createElement('button');
    mp4Btn.type = 'button';
    mp4Btn.className = `mymedia-pill-btn ${selectedFormat === 'mp4' ? 'active' : ''}`;
    mp4Btn.dataset.format = 'mp4';
    mp4Btn.textContent = 'MP4';

    [mkvBtn, mp4Btn].forEach((btn) => {
      btn.addEventListener('click', () => {
        mkvBtn.classList.remove('active');
        mp4Btn.classList.remove('active');
        btn.classList.add('active');
        selectedFormat = btn.dataset.format;
      });
      pillsDiv.appendChild(btn);
    });

    formatField.appendChild(pillsDiv);
    optionsDiv.appendChild(formatField);

    // Campo: Qualidade
    const qualityField = document.createElement('div');
    qualityField.className = 'mymedia-balloon-field';
    const qualityLabel = document.createElement('label');
    qualityLabel.htmlFor = 'mymedia-balloon-quality';
    qualityLabel.textContent = 'Qualidade';
    qualityField.appendChild(qualityLabel);

    const qualitySelect = document.createElement('select');
    qualitySelect.id = 'mymedia-balloon-quality';
    qualitySelect.className = 'mymedia-select';

    const qualities = [
      { val: 'best', label: 'Melhor (Automático)' },
      { val: '1440p', label: '1440p (2K)' },
      { val: '1080p', label: '1080p (Full HD)' },
      { val: '720p', label: '720p (HD)' },
      { val: '480p', label: '480p' },
      { val: '360p', label: '360p' }
    ];

    qualities.forEach((q) => {
      const opt = document.createElement('option');
      opt.value = q.val;
      opt.textContent = q.label;
      if (q.val === selectedQuality) opt.selected = true;
      qualitySelect.appendChild(opt);
    });

    qualityField.appendChild(qualitySelect);
    optionsDiv.appendChild(qualityField);
    balloon.appendChild(optionsDiv);

    // Checkbox: Lembrar opções
    const checkboxLabel = document.createElement('label');
    checkboxLabel.className = 'mymedia-balloon-checkbox-label';

    const checkboxInput = document.createElement('input');
    checkboxInput.type = 'checkbox';
    checkboxInput.id = 'mymedia-balloon-remember';

    const checkboxSpan = document.createElement('span');
    checkboxSpan.textContent = 'Sempre usar estas opções e baixar direto (1 clique)';

    checkboxLabel.appendChild(checkboxInput);
    checkboxLabel.appendChild(checkboxSpan);
    balloon.appendChild(checkboxLabel);

    // Botão de Confirmação
    const confirmBtn = document.createElement('button');
    confirmBtn.type = 'button';
    confirmBtn.className = 'mymedia-balloon-action-btn';
    confirmBtn.id = 'mymedia-balloon-confirm';
    confirmBtn.appendChild(parseSvg(ACTION_BTN_SVG));
    const confirmBtnText = document.createTextNode(' Baixar agora');
    confirmBtn.appendChild(confirmBtnText);

    confirmBtn.addEventListener('click', async () => {
      const remember = checkboxInput.checked;
      const quality = qualitySelect.value;

      if (remember) {
        try {
          await browser.runtime.sendMessage({
            action: 'SAVE_CONFIG',
            config: {
              defaultContainer: selectedFormat,
              defaultQuality: quality,
              skipConfirmation: true
            }
          });
        } catch (err) {
          console.warn('[ZMydia] Failed to save settings:', err);
        }
      }

      close();

      if (onConfirm) {
        onConfirm({
          container: selectedFormat,
          quality: quality
        });
      }
    });

    balloon.appendChild(confirmBtn);

    document.body.appendChild(balloon);
    activeBalloon = balloon;

    positionBalloon(balloon, anchorEl);
    repositionHandler = () => positionBalloon(balloon, anchorEl);
    window.addEventListener('resize', repositionHandler);
    window.addEventListener('scroll', repositionHandler, true);
    closeBtn.focus({ preventScroll: true });
  }

  function positionBalloon(balloon, anchorEl) {
    const rect = anchorEl.getBoundingClientRect();
    const bounds = balloon.getBoundingClientRect();
    const bWidth = bounds.width || 280;
    const bHeight = bounds.height || 240;

    let left = rect.left;
    let top = rect.bottom + 8;

    if (left + bWidth > window.innerWidth - 16) {
      left = window.innerWidth - bWidth - 16;
    }
    if (left < 16) left = 16;

    if (top + bHeight > window.innerHeight - 16) {
      top = rect.top - bHeight - 8;
    }
    top = Math.max(12, Math.min(top, window.innerHeight - bHeight - 12));

    balloon.style.left = `${Math.round(left)}px`;
    balloon.style.top = `${Math.round(top)}px`;
  }

  return {
    show,
    close
  };
})();

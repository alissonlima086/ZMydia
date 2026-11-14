/**
 * ZMydia Downloader - Background Service Worker
 * Gerencia a comunicação segura com o servidor local ZMydia via API Token exclusivo e Bearer Auth.
 */

const DEFAULT_CONFIG = {
  serverUrl: 'http://127.0.0.1:4173',
  apiToken: '',
  tokenHint: '',
  defaultContainer: 'mkv',
  defaultQuality: 'best',
  skipConfirmation: false
};

// Obtém as configurações salvas ou valores padrão
async function getConfig() {
  try {
    const data = await browser.storage.local.get(DEFAULT_CONFIG);
    return { ...DEFAULT_CONFIG, ...data };
  } catch (err) {
    console.warn('[ZMydia] Failed to read storage:', err);
    return { ...DEFAULT_CONFIG };
  }
}

// Testa a conexão e autenticação com o servidor ZMydia
async function checkHealth(serverUrl) {
  const config = await getConfig();
  const cleanUrl = (serverUrl || config.serverUrl).replace(/\/+$/, '');

  const headers = { 'Accept': 'application/json' };
  if (config.apiToken) {
    headers['Authorization'] = `Bearer ${config.apiToken}`;
  }

  try {
    const res = await fetch(`${cleanUrl}/api/health`, {
      method: 'GET',
      headers
    });

    if (!res.ok) {
      if (res.status === 401 || res.status === 403) {
        return {
          ok: false,
          isPaired: false,
          error: 'Token de pareamento inválido ou revogado.'
        };
      }
      return { ok: false, error: `HTTP ${res.status}` };
    }

    const data = await res.json();
    if (config.apiToken) {
      const authRes = await fetch(`${cleanUrl}/api/downloads`, {
        method: 'GET',
        headers: {
          'Accept': 'application/json',
          'Authorization': `Bearer ${config.apiToken}`
        }
      });
      if (!authRes.ok) {
        return {
          ok: false,
          serverOnline: true,
          isPaired: false,
          error: authRes.status === 401 || authRes.status === 403
            ? 'A autorização expirou ou pertence a uma versão antiga. Pareie novamente.'
            : `Falha ao validar autorização (HTTP ${authRes.status}).`
        };
      }
    }
    return {
      ok: true,
      data,
      isPaired: Boolean(config.apiToken),
      tokenHint: config.tokenHint || (config.apiToken ? `••••${config.apiToken.slice(-4)}` : '')
    };
  } catch (err) {
    return {
      ok: false,
      serverOnline: false,
      isPaired: Boolean(config.apiToken),
      tokenHint: config.tokenHint,
      error: err.message || 'Sem conexão com o servidor'
    };
  }
}

// Inicia um download no worker do ZMydia utilizando o Token de API
async function startDownload({ url, quality, container, title }) {
  const config = await getConfig();
  const serverUrl = config.serverUrl.replace(/\/+$/, '');

  if (!config.apiToken) {
    throw new Error('Extensão não pareada com o ZMydia. Abra as configurações do ZMydia para parear.');
  }

  const targetQuality = quality || config.defaultQuality || 'best';
  const targetContainer = container || config.defaultContainer || 'mkv';

  const payload = {
    url,
    quality: targetQuality,
    container: targetContainer
  };

  const headers = {
    'Content-Type': 'application/json',
    'Authorization': `Bearer ${config.apiToken}`
  };

  const res = await fetch(`${serverUrl}/api/downloads`, {
    method: 'POST',
    headers,
    body: JSON.stringify(payload)
  });

  if (res.status === 401 || res.status === 403) {
    throw new Error('Autorização negada: token revogado ou inválido. Por favor, re-pareie a extensão no ZMydia.');
  }

  if (!res.ok) {
    const errorText = await res.text();
    throw new Error(errorText || `Erro no servidor (${res.status})`);
  }

  const job = await res.json();

  // Exibe notificação no navegador
  try {
    const label = title ? `"${title}"` : 'Vídeo do YouTube';
    await browser.notifications.create({
      type: 'basic',
      iconUrl: browser.runtime.getURL('icons/icon-48.png'),
      title: 'Download iniciado no ZMydia',
      message: `${label} (${targetContainer.toUpperCase()} · ${targetQuality})`
    });
  } catch {
    // Notificação opcional
  }

  return { ok: true, job };
}

// Receptor de mensagens do Content Script e Popup
browser.runtime.onMessage.addListener((message, sender) => {
  if (!message || !message.action) return;

  switch (message.action) {
    case 'CHECK_HEALTH':
      return checkHealth(message.serverUrl);

    case 'GET_CONFIG':
      return getConfig();

    case 'SAVE_CONFIG':
      return browser.storage.local.set(message.config).then(() => ({ ok: true }));

    case 'CLEAR_PAIRING':
      return browser.storage.local.set({ apiToken: '', tokenHint: '' }).then(() => ({ ok: true }));

    case 'START_DOWNLOAD':
      return startDownload(message.payload);

    case 'OPEN_URL':
      return browser.tabs.create({ url: message.url });

    default:
      return Promise.resolve({ ok: false, error: 'Ação desconhecida' });
  }
});

// Monitora transições SPA do YouTube na mesma aba (watch e shorts) e notifica o Content Script
if (browser.tabs && browser.tabs.onUpdated) {
  browser.tabs.onUpdated.addListener((tabId, changeInfo, tab) => {
    if (
      changeInfo.url &&
      (changeInfo.url.includes('youtube.com/watch') || changeInfo.url.includes('youtube.com/shorts/'))
    ) {
      browser.tabs.sendMessage(tabId, {
        action: 'YT_SPA_NAVIGATE',
        url: changeInfo.url
      }).catch(() => {
        // Ignora caso a aba ainda não tenha o content script pronto
      });
    }
  });
}

console.log('[ZMydia] Background worker active (Bearer authentication enabled).');

/**
 * ZMydia Downloader - Content Script para Pareamento Automático
 * Escuta mensagens originadas da própria aba do ZMydia (localhost / 127.0.0.1)
 * para realizar o pareamento de credenciais de forma segura e 100% automatizada.
 */

(function () {
  'use strict';

  // Responde a pings e solicitações de pareamento da aplicação web ZMydia
  window.addEventListener('message', async (event) => {
    if (event.source !== window) return;
    if (!event.data || typeof event.data !== 'object') return;

    // 1. Handshake / Detecção: Permite que o ZMydia saiba que a extensão está instalada
    if (event.data.type === 'MYMEDIA_PING_EXTENSION') {
      try {
        const manifest = browser.runtime.getManifest();
        window.postMessage({
          type: 'MYMEDIA_PONG_EXTENSION',
          version: manifest.version,
          name: manifest.name,
          extensionOrigin: new URL(browser.runtime.getURL('')).origin
        }, '*');
      } catch {
        // Fallback
      }
      return;
    }

    // 2. Pareamento: Recebe token de API exclusivo gerado pelo usuário autenticado
    if (event.data.type === 'MYMEDIA_EXTENSION_PAIR_TOKEN') {
      const { token, serverUrl, tokenHint } = event.data;

      if (!token || typeof token !== 'string' || !token.startsWith('mm_ext_')) {
        console.warn('[ZMydia Extension] Pairing token rejected because its format is invalid.');
        window.postMessage({
          type: 'MYMEDIA_EXTENSION_PAIRED_ERROR',
          error: 'Token inválido ou incompatível.'
        }, '*');
        return;
      }

      try {
        const cleanUrl = (serverUrl && typeof serverUrl === 'string')
          ? serverUrl.replace(/\/+$/, '')
          : window.location.origin;

        const hint = tokenHint || (token.length >= 4 ? `••••${token.slice(-4)}` : '••••');

        // Envia para o background worker persistir em storage.local
        const res = await browser.runtime.sendMessage({
          action: 'SAVE_CONFIG',
          config: {
            serverUrl: cleanUrl,
            apiToken: token,
            tokenHint: hint
          }
        });

        if (res && res.ok) {
          window.postMessage({
            type: 'MYMEDIA_EXTENSION_PAIRED_SUCCESS',
            tokenHint: hint,
            serverUrl: cleanUrl
          }, '*');
          console.log('[ZMydia Extension] Pairing completed successfully.');
        } else {
          throw new Error(res?.error || 'Erro ao gravar credenciais');
        }
      } catch (err) {
        console.error('[ZMydia Extension] Failed to pair extension:', err);
        window.postMessage({
          type: 'MYMEDIA_EXTENSION_PAIRED_ERROR',
          error: err.message || 'Falha ao salvar credenciais'
        }, '*');
      }
    }
  });

  // Notifica imediatamente caso a aplicação web já esteja carregada
  try {
    const manifest = browser.runtime.getManifest();
    window.postMessage({
      type: 'MYMEDIA_EXTENSION_LOADED',
      version: manifest.version,
      extensionOrigin: new URL(browser.runtime.getURL('')).origin
    }, '*');
  } catch {}
})();

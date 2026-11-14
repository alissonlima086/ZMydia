/** ZMydia Downloader popup controller. */

const REQUIRED_ORIGINS = [
  '*://*.youtube.com/*',
  'http://127.0.0.1/*',
  'http://localhost/*'
];

document.addEventListener('DOMContentLoaded', async () => {
  const $ = (id) => document.getElementById(id);
  const serverUrlInput = $('serverUrl');
  const formatSelect = $('defaultFormat');
  const qualitySelect = $('defaultQuality');
  const skipConfirmation = $('skipConfirmation');
  const pairingCard = $('pairingCard');
  const pairingLabel = $('pairingLabel');
  const pairingDesc = $('pairingDesc');
  const autoPairSection = $('autoPairSection');
  const unpairBtn = $('unpairBtn');
  const permissionCard = $('ytPermissionCard');
  const statusBadge = $('statusBadge');
  const statusText = $('statusText');
  const feedback = $('feedbackMessage');

  let config = await browser.runtime.sendMessage({ action: 'GET_CONFIG' }) || {};
  serverUrlInput.value = config.serverUrl || 'http://127.0.0.1:4173';
  formatSelect.value = config.defaultContainer || 'mkv';
  qualitySelect.value = config.defaultQuality || 'best';
  skipConfirmation.checked = Boolean(config.skipConfirmation);

  function renderPairing(isPaired) {
    pairingCard.classList.toggle('paired', isPaired);
    pairingLabel.textContent = isPaired
      ? `Conectada ${config.tokenHint ? `(${config.tokenHint})` : ''}`
      : 'Conexão necessária';
    pairingDesc.textContent = isPaired
      ? 'Esta extensão pode enviar downloads com segurança.'
      : 'Conecte esta extensão ao ZMydia para autorizar downloads.';
    autoPairSection.style.display = isPaired ? 'none' : 'block';
    unpairBtn.style.display = isPaired ? 'inline-block' : 'none';
  }

  function showMessage(text, type) {
    feedback.textContent = text;
    feedback.className = `feedback-msg ${type}`;
    feedback.style.display = 'block';
    window.setTimeout(() => { feedback.style.display = 'none'; }, 5000);
  }

  async function saveConfig(extra = {}) {
    await browser.runtime.sendMessage({
      action: 'SAVE_CONFIG',
      config: {
        serverUrl: serverUrlInput.value.trim() || 'http://127.0.0.1:4173',
        defaultContainer: formatSelect.value,
        defaultQuality: qualitySelect.value,
        skipConfirmation: skipConfirmation.checked,
        ...extra
      }
    });
    config = await browser.runtime.sendMessage({ action: 'GET_CONFIG' }) || {};
  }

  async function refreshPermissionState() {
    try {
      const granted = await browser.permissions.contains({ origins: REQUIRED_ORIGINS });
      permissionCard.style.display = granted ? 'none' : 'flex';
      return granted;
    } catch {
      permissionCard.style.display = 'flex';
      return false;
    }
  }

  async function requestPermissions() {
    const granted = await browser.permissions.request({ origins: REQUIRED_ORIGINS });
    await refreshPermissionState();
    if (!granted) showMessage('A permissão é necessária para usar o botão no YouTube.', 'error');
    return granted;
  }

  async function checkConnection(showResult = false) {
    statusBadge.className = 'status-badge connecting';
    statusText.textContent = 'Verificando…';
    const hasPermissions = await refreshPermissionState();
    if (!hasPermissions) {
      statusBadge.className = 'status-badge offline';
      statusText.textContent = 'Permissão pendente';
      renderPairing(false);
      return;
    }

    const result = await browser.runtime.sendMessage({
      action: 'CHECK_HEALTH',
      serverUrl: serverUrlInput.value
    });
    const paired = Boolean(result?.ok && result?.isPaired);
    renderPairing(paired);
    statusBadge.className = result?.ok ? 'status-badge online' : 'status-badge offline';
    statusText.textContent = paired ? 'Pronta' : (result?.serverOnline ? 'Reparear' : 'Offline');
    if (showResult) {
      showMessage(result?.ok ? 'Servidor e autorização validados.' : (result?.error || 'Sem conexão com o ZMydia.'), result?.ok ? 'success' : 'error');
    }
  }

  $('grantYtBtn').addEventListener('click', async () => {
    if (await requestPermissions()) {
      showMessage('Permissão concedida. Recarregue abas do YouTube que já estavam abertas.', 'success');
      await checkConnection();
    }
  });

  $('autoPairBtn').addEventListener('click', async () => {
    if (!(await requestPermissions())) return;
    await saveConfig();
    const base = serverUrlInput.value.trim().replace(/\/+$/, '') || 'http://127.0.0.1:4173';
    await browser.runtime.sendMessage({
      action: 'OPEN_URL',
      url: `${base}/configuracoes?tab=extensao&action=pair`
    });
    window.close();
  });

  unpairBtn.addEventListener('click', async () => {
    await browser.runtime.sendMessage({ action: 'CLEAR_PAIRING' });
    config = await browser.runtime.sendMessage({ action: 'GET_CONFIG' }) || {};
    renderPairing(false);
    await checkConnection();
  });

  $('testBtn').addEventListener('click', async () => {
    await saveConfig();
    await checkConnection(true);
  });
  $('openLibraryBtn').addEventListener('click', async () => {
    await saveConfig();
    await browser.runtime.sendMessage({ action: 'OPEN_URL', url: serverUrlInput.value });
    window.close();
  });
  serverUrlInput.addEventListener('change', async () => { await saveConfig(); await checkConnection(); });
  formatSelect.addEventListener('change', () => saveConfig());
  qualitySelect.addEventListener('change', () => saveConfig());
  skipConfirmation.addEventListener('change', () => saveConfig());

  renderPairing(Boolean(config.apiToken));
  await checkConnection();
});

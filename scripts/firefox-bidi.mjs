#!/usr/bin/env node

const endpoint = process.env.FIREFOX_BIDI_URL || 'ws://127.0.0.1:9230/session';
const targetUrl = process.argv[2] || 'about:blank';

const socket = new WebSocket(endpoint);
const pending = new Map();
let nextId = 1;

socket.addEventListener('message', (event) => {
  const message = JSON.parse(event.data);
  if (!message.id) return;
  const request = pending.get(message.id);
  if (!request) return;
  pending.delete(message.id);
  if (message.type === 'error') {
    request.reject(new Error(`${message.error}: ${message.message}`));
  } else {
    request.resolve(message.result);
  }
});

function command(method, params = {}) {
  const id = nextId++;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params }));
  });
}

function remoteValue(value) {
  if (!value || typeof value !== 'object' || !('type' in value)) return value;
  if (value.type === 'array') return value.value.map(remoteValue);
  if (value.type === 'object') {
    return Object.fromEntries(value.value.map(([key, entry]) => [key, remoteValue(entry)]));
  }
  if (value.type === 'null') return null;
  if (value.type === 'undefined') return undefined;
  return value.value;
}

await new Promise((resolve, reject) => {
  socket.addEventListener('open', resolve, { once: true });
  socket.addEventListener('error', reject, { once: true });
});

let sessionStarted = false;
try {
  await command('session.new', { capabilities: {} });
  sessionStarted = true;
  const created = await command('browsingContext.create', { type: 'tab' });
  const context = created.context;
  await command('browsingContext.navigate', {
    context,
    url: targetUrl,
    wait: 'complete'
  });
  const inspected = await command('script.evaluate', {
    expression: `({
      title: document.title,
      url: location.href,
      text: document.body?.innerText?.slice(0, 12000) || '',
      links: [...document.querySelectorAll('a')].slice(0, 100).map((link) => ({
        text: link.innerText.trim(),
        href: link.href
      })),
      inputs: [...document.querySelectorAll('input, button, select')].slice(0, 100).map((element) => ({
        tag: element.tagName,
        type: element.type || '',
        name: element.name || '',
        id: element.id || '',
        text: element.innerText || element.value || '',
        disabled: element.disabled
      }))
    })`,
    target: { context },
    awaitPromise: true,
    resultOwnership: 'none'
  });
  process.stdout.write(`${JSON.stringify(remoteValue(inspected.result), null, 2)}\n`);
} finally {
  if (sessionStarted) {
    await command('session.end').catch(() => {});
  }
  socket.close();
}

#!/usr/bin/env node

import { readFile, writeFile } from 'node:fs/promises';
import { basename } from 'node:path';

const endpoint = process.env.FIREFOX_BIDI_URL || 'ws://127.0.0.1:9230/session';
const action = process.argv[2] || 'start';
const packagePath = process.argv[3];

const socket = new WebSocket(endpoint);
const pending = new Map();
let nextId = 1;
let sessionStarted = false;

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

async function evaluate(context, expression, resultOwnership = 'none') {
  const response = await command('script.evaluate', {
    expression,
    target: { context },
    awaitPromise: true,
    resultOwnership
  });
  return response.result;
}

async function snapshot(context) {
  const result = await evaluate(context, `({
    title: document.title,
    url: location.href,
    text: document.body?.innerText?.slice(0, 16000) || '',
    inputs: [...document.querySelectorAll('input, button, select, textarea')].map((element) => ({
      tag: element.tagName,
      type: element.type || '',
      name: element.name || '',
      id: element.id || '',
      value: element.value || '',
      text: element.innerText || '',
      checked: Boolean(element.checked),
      disabled: Boolean(element.disabled)
    }))
  })`);
  return remoteValue(result);
}

async function waitFor(context, expression, timeoutMs = 30000) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const result = remoteValue(await evaluate(context, expression));
    if (result) return result;
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error(`Timed out waiting for: ${expression}`);
}

await new Promise((resolve, reject) => {
  socket.addEventListener('open', resolve, { once: true });
  socket.addEventListener('error', reject, { once: true });
});

try {
  await command('session.new', { capabilities: {} });
  sessionStarted = true;
  const { context } = await command('browsingContext.create', { type: 'tab' });

  if (action === 'start') {
    const distributionUrl = 'https://addons.mozilla.org/en-US/developers/addon/submit/distribution';
    await command('browsingContext.navigate', {
      context,
      url: distributionUrl,
      wait: 'complete'
    });
    await evaluate(context, `(() => {
      const choice = document.querySelector('input[name="channel"][value="unlisted"]');
      if (!choice) throw new Error('unlisted distribution option not found');
      choice.click();
      choice.form.requestSubmit();
      return true;
    })()`);
    await waitFor(context, `location.href !== ${JSON.stringify(distributionUrl)}`);
    await waitFor(context, `document.readyState === 'complete'`);
    process.stdout.write(`${JSON.stringify(await snapshot(context), null, 2)}\n`);
  } else if (action === 'upload') {
    if (!packagePath) throw new Error('package path is required for upload');
    const uploadUrl = 'https://addons.mozilla.org/en-US/developers/addon/submit/upload-unlisted';
    await command('browsingContext.navigate', {
      context,
      url: uploadUrl,
      wait: 'complete'
    });
    const encodedPackage = (await readFile(packagePath)).toString('base64');
    await evaluate(context, `(() => {
      const input = document.querySelector('input[type="file"]');
      if (!input) throw new Error('file input not found');
      const encoded = ${JSON.stringify(encodedPackage)};
      const binary = atob(encoded);
      const bytes = new Uint8Array(binary.length);
      for (let index = 0; index < binary.length; index += 1) {
        bytes[index] = binary.charCodeAt(index);
      }
      const transfer = new DataTransfer();
      transfer.items.add(new File([bytes], ${JSON.stringify(basename(packagePath))}, {
        type: 'application/zip'
      }));
      input.files = transfer.files;
      input.dispatchEvent(new Event('change', { bubbles: true }));
      return true;
    })()`);
    await waitFor(context, `!document.querySelector('#submit-upload-file-finish')?.disabled`);
    await evaluate(context, `(() => {
      document.querySelector('#submit-upload-file-finish').form.requestSubmit();
      return true;
    })()`);
    await new Promise((resolve) => setTimeout(resolve, 5000));
    process.stdout.write(`${JSON.stringify(await snapshot(context), null, 2)}\n`);
  } else if (action === 'source-no') {
    const sourceUrl = process.argv[3];
    if (!sourceUrl) throw new Error('source declaration URL is required');
    await command('browsingContext.navigate', {
      context,
      url: sourceUrl,
      wait: 'complete'
    });
    await evaluate(context, `(() => {
      const choice = document.querySelector('input[name="has_source"][value="no"]');
      if (!choice) throw new Error('source declaration option not found');
      choice.click();
      choice.form.requestSubmit();
      return true;
    })()`);
    await new Promise((resolve) => setTimeout(resolve, 5000));
    process.stdout.write(`${JSON.stringify(await snapshot(context), null, 2)}\n`);
  } else if (action === 'monitor') {
    const versionsUrl = process.argv[3];
    if (!versionsUrl) throw new Error('versions URL is required');
    for (let attempt = 1; attempt <= 30; attempt += 1) {
      await command('browsingContext.navigate', {
        context,
        url: versionsUrl,
        wait: 'complete'
      });
      const status = remoteValue(await evaluate(context, `({
        text: document.body?.innerText || '',
        downloads: [...document.querySelectorAll('a[href]')]
          .map((link) => ({ text: link.innerText.trim(), href: link.href }))
          .filter((link) => link.href.includes('.xpi') || link.href.includes('/downloads/file/'))
      })`));
      const state = status.text.includes('Awaiting Review')
        ? 'Awaiting Review'
        : status.text.includes('Approved')
          ? 'Approved'
          : status.text.includes('Rejected')
            ? 'Rejected'
            : 'Unknown';
      process.stdout.write(`${JSON.stringify({ attempt, state, downloads: status.downloads })}\n`);
      if (state === 'Approved' || state === 'Rejected' || status.downloads.length > 0) break;
      await new Promise((resolve) => setTimeout(resolve, 20000));
    }
  } else if (action === 'download') {
    const downloadUrl = process.argv[3];
    const outputPath = process.argv[4];
    if (!downloadUrl || !outputPath) {
      throw new Error('download URL and output path are required');
    }
    await command('browsingContext.navigate', {
      context,
      url: 'https://addons.mozilla.org/en-US/developers/',
      wait: 'complete'
    });
    const downloaded = remoteValue(await evaluate(context, `(async () => {
      const response = await fetch(${JSON.stringify(downloadUrl)}, { credentials: 'include' });
      if (!response.ok) throw new Error(\`download failed with HTTP \${response.status}\`);
      const bytes = new Uint8Array(await response.arrayBuffer());
      let binary = '';
      const chunkSize = 0x8000;
      for (let offset = 0; offset < bytes.length; offset += chunkSize) {
        binary += String.fromCharCode(...bytes.subarray(offset, offset + chunkSize));
      }
      return {
        contentType: response.headers.get('content-type'),
        bytes: bytes.length,
        base64: btoa(binary)
      };
    })()`));
    const file = Buffer.from(downloaded.base64, 'base64');
    if (file.length !== downloaded.bytes) throw new Error('download size mismatch');
    await writeFile(outputPath, file, { flag: 'wx' });
    process.stdout.write(`${JSON.stringify({
      outputPath,
      bytes: file.length,
      contentType: downloaded.contentType
    })}\n`);
  } else {
    throw new Error(`unknown action: ${action}`);
  }
} finally {
  if (sessionStarted) await command('session.end').catch(() => {});
  socket.close();
}

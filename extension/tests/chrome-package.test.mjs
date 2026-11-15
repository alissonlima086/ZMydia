import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { promisify } from 'node:util';
import { execFile } from 'node:child_process';
import test from 'node:test';

const execFileAsync = promisify(execFile);

test('pacote local do Chrome usa Manifest V3 compatível', async (context) => {
  const output = await mkdtemp(join(tmpdir(), 'zmydia-chrome-test-'));
  await rm(output, { recursive: true });
  context.after(() => rm(output, { recursive: true, force: true }));

  await execFileAsync(process.execPath, [
    new URL('../../scripts/package-chrome-extension.mjs', import.meta.url).pathname,
    output
  ]);

  const manifest = JSON.parse(await readFile(join(output, 'manifest.json'), 'utf8'));
  const popup = await readFile(join(output, 'popup/popup.html'), 'utf8');
  const worker = await readFile(join(output, 'service-worker.js'), 'utf8');

  assert.equal(manifest.version, '0.1.0');
  assert.deepEqual(manifest.background, { service_worker: 'service-worker.js' });
  assert.equal(manifest.browser_specific_settings, undefined);
  assert.ok(manifest.content_scripts.every((entry) => entry.js[0] === 'compat.js'));
  assert.match(popup, /<script src="\.\.\/compat\.js"><\/script>/);
  assert.match(worker, /importScripts\('background\/background\.js'\)/);
});

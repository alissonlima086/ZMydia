import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const manifest = JSON.parse(
  await readFile(new URL('../manifest.json', import.meta.url), 'utf8')
);
const shortsScript = await readFile(new URL('../content/content.js', import.meta.url), 'utf8');

test('manifesto registra os scripts esperados no YouTube', () => {
  const youtube = manifest.content_scripts.find((entry) =>
    entry.matches.includes('*://*.youtube.com/*')
  );

  assert.ok(youtube);
  assert.deepEqual(youtube.js, ['content/balloon.js', 'content/content.js']);
  assert.deepEqual(youtube.css, ['content/content.css']);
});

test('permissoes permanecem limitadas aos hosts usados', () => {
  assert.deepEqual(manifest.permissions, ['storage', 'notifications']);
  assert.deepEqual(manifest.host_permissions, [
    'http://127.0.0.1/*',
    'http://localhost/*',
    '*://*.youtube.com/*'
  ]);
});

test('integracao de Shorts cobre o action bar atual', () => {
  assert.match(shortsScript, /reel-action-bar-view-model/);
  assert.match(shortsScript, /ytwReelActionBarViewModelHost/);
  assert.match(shortsScript, /actions-not-found/);
});

test('versao e identidade publicadas sao validas', () => {
  assert.equal(manifest.name, 'ZMydia Downloader');
  assert.equal(manifest.version, '0.1.0');
  assert.equal(manifest.browser_specific_settings.gecko.id, 'zmydia-downloader@local');
});

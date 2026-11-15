#!/usr/bin/env node

import { access, cp, mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const sourceDir = resolve(repoRoot, 'extension');
const outputDir = resolve(process.argv[2] || resolve(sourceDir, 'chrome'));

try {
  await access(outputDir);
  throw new Error(`A pasta de destino já existe: ${outputDir}`);
} catch (error) {
  if (error.code !== 'ENOENT') throw error;
}

const sourceManifest = JSON.parse(
  await readFile(resolve(sourceDir, 'manifest.json'), 'utf8')
);
const manifest = structuredClone(sourceManifest);

delete manifest.browser_specific_settings;
manifest.background = { service_worker: 'service-worker.js' };
manifest.content_scripts = manifest.content_scripts.map((entry) => ({
  ...entry,
  js: ['compat.js', ...entry.js]
}));

await mkdir(outputDir, { recursive: true });
for (const directory of ['background', 'content', 'icons', 'popup']) {
  await cp(resolve(sourceDir, directory), resolve(outputDir, directory), { recursive: true });
}

const popupPath = resolve(outputDir, 'popup', 'popup.html');
const popup = await readFile(popupPath, 'utf8');
const chromePopup = popup.replace(
  '  <script src="popup.js"></script>',
  '  <script src="../compat.js"></script>\n  <script src="popup.js"></script>'
);

if (popup === chromePopup) {
  throw new Error('Não foi possível inserir a compatibilidade do Chrome no popup.');
}

await Promise.all([
  writeFile(resolve(outputDir, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`),
  writeFile(
    resolve(outputDir, 'compat.js'),
    "globalThis.browser ??= globalThis.chrome;\n"
  ),
  writeFile(
    resolve(outputDir, 'service-worker.js'),
    "globalThis.browser = globalThis.chrome;\nimportScripts('background/background.js');\n"
  ),
  writeFile(popupPath, chromePopup),
  writeFile(
    resolve(outputDir, 'COMO-INSTALAR.txt'),
    [
      'ZMydia Downloader para Chrome/Chromium (instalação local)',
      '',
      'REQUER ZMydia 0.1.1 ou posterior. O servidor 0.1.0 aceita apenas a extensão Firefox.',
      '',
      '1. Abra chrome://extensions',
      '2. Ative o Modo do desenvolvedor.',
      '3. Clique em Carregar sem compactação.',
      '4. Selecione esta pasta.',
      '',
      'Mantenha esta pasta no mesmo lugar enquanto usar a extensão.',
      'Após atualizar os arquivos, use o botão Recarregar na página de extensões.',
      ''
    ].join('\n')
  )
]);

console.log(outputDir);

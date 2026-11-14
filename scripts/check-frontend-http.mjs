#!/usr/bin/env node
import assert from 'node:assert/strict';
import { readdir, readFile } from 'node:fs/promises';

const root = new URL('../frontend/src/', import.meta.url);
const violations = [];

async function visit(directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const url = new URL(entry.name, directory);
    if (entry.isDirectory()) await visit(new URL(`${entry.name}/`, directory));
    else if (/\.(?:ts|svelte)$/.test(entry.name) && entry.name !== 'api.test.ts') {
      const source = await readFile(url, 'utf8');
      if (entry.name !== 'api.ts' && /\bfetch\s*\(/.test(source)) {
        violations.push(url.pathname);
      }
    }
  }
}

await visit(root);
assert.deepEqual(
  violations,
  [],
  `Chamadas fetch devem passar por frontend/src/lib/api.ts: ${violations.join(', ')}`
);
console.log('frontend HTTP access is unified in src/lib/api.ts');

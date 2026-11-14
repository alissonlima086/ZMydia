#!/usr/bin/env node
import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';

const sourceUrl = new URL('../crates/server/src/web.rs', import.meta.url);
const contractUrl = new URL('../tests/api-contract.json', import.meta.url);
const source = await readFile(sourceUrl, 'utf8');

function routeBlocks(input) {
  const blocks = [];
  let cursor = 0;
  while ((cursor = input.indexOf('.route(', cursor)) !== -1) {
    const start = cursor;
    let depth = 0;
    let quote = null;
    let escaped = false;
    for (; cursor < input.length; cursor += 1) {
      const char = input[cursor];
      if (quote) {
        if (escaped) escaped = false;
        else if (char === '\\') escaped = true;
        else if (char === quote) quote = null;
        continue;
      }
      if (char === '"' || char === "'") quote = char;
      else if (char === '(') depth += 1;
      else if (char === ')' && --depth === 0) {
        blocks.push(input.slice(start, cursor + 1));
        cursor += 1;
        break;
      }
    }
  }
  return blocks;
}

const paths = {};
for (const block of routeBlocks(source)) {
  const path = block.match(/\.route\(\s*"([^"]+)"/)?.[1];
  if (!path || !path.startsWith('/api/')) continue;
  const methods = [...block.matchAll(/(?:\b|::)(get|post|put|patch|delete|head)\s*\(/g)]
    .map((match) => match[1].toUpperCase());
  paths[path] = [...new Set([...(paths[path] ?? []), ...methods])].sort();
}

const generated = {
  schema_version: 1,
  description: 'Contrato de compatibilidade das rotas HTTP públicas do ZMydia.',
  error: { error: { code: 'string?', message: 'string', details: 'unknown?' } },
  pagination: {
    cursor: { items: 'T[]', next_cursor: 'string|null' },
    offset: { items: 'T[]', limit: 'number', offset: 'number', total: 'number?' }
  },
  paths: Object.fromEntries(Object.entries(paths).sort(([a], [b]) => a.localeCompare(b)))
};
const serialized = `${JSON.stringify(generated, null, 2)}\n`;

if (process.argv.includes('--write')) {
  await writeFile(contractUrl, serialized);
  console.log(`contract updated: ${Object.keys(paths).length} routes`);
} else {
  const current = await readFile(contractUrl, 'utf8');
  assert.equal(
    current,
    serialized,
    'The HTTP contract changed. Review compatibility and run: node scripts/check-api-contract.mjs --write'
  );
  console.log(`HTTP contract compatible: ${Object.keys(paths).length} routes`);
}

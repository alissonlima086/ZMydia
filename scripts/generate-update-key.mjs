#!/usr/bin/env node

import { generateKeyPairSync } from 'node:crypto';
import { writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

function parseArgs(argv) {
  const values = new Map();
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key?.startsWith('--') || !value) {
      throw new Error(`invalid argument near ${key ?? '<end>'}`);
    }
    values.set(key.slice(2), value);
  }
  return values;
}

const args = parseArgs(process.argv.slice(2));
const privatePath = args.get('private-output');
const publicPath = args.get('public-output');
if (!privatePath || !publicPath) {
  throw new Error('--private-output and --public-output are required');
}

const { privateKey, publicKey } = generateKeyPairSync('ed25519');
const privateDer = privateKey.export({ format: 'der', type: 'pkcs8' });
const publicDer = publicKey.export({ format: 'der', type: 'spki' });
const spkiPrefix = Buffer.from('302a300506032b6570032100', 'hex');
if (!publicDer.subarray(0, spkiPrefix.length).equals(spkiPrefix)) {
  throw new Error('unexpected Ed25519 public key encoding');
}

const privateOutput = resolve(privatePath);
const publicOutput = resolve(publicPath);
await writeFile(privateOutput, `${privateDer.toString('base64')}\n`, {
  flag: 'wx',
  mode: 0o600
});
try {
  await writeFile(publicOutput, `${publicDer.subarray(spkiPrefix.length).toString('base64')}\n`, {
    flag: 'wx',
    mode: 0o644
  });
} catch (error) {
  throw new Error(
    `public key was not written; remove the newly created private key at ${privateOutput} before retrying`,
    { cause: error }
  );
}

console.log(`Private signing key written to ${privateOutput}`);
console.log(`Public verification key written to ${publicOutput}`);
console.log('Keep the private file offline and never commit it.');

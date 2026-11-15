#!/usr/bin/env node

import {
  createHash,
  createPrivateKey,
  createPublicKey,
  sign
} from 'node:crypto';
import { readFile, stat, writeFile } from 'node:fs/promises';
import { basename, join, resolve } from 'node:path';

function parseArgs(argv) {
  const values = new Map();
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key?.startsWith('--') || !value) throw new Error(`invalid argument near ${key ?? '<end>'}`);
    values.set(key.slice(2), value);
  }
  return values;
}

async function asset(directory, version, platform, kind, suffix) {
  const filename = `zmydia-${version}-${platform}${suffix}`;
  const path = join(directory, filename);
  const [contents, metadata] = await Promise.all([readFile(path), stat(path)]);
  if (!metadata.isFile() || metadata.size === 0) throw new Error(`invalid release asset: ${path}`);
  return {
    kind,
    url: `https://github.com/alissonlima086/ZMydia/releases/download/v${version}/${filename}`,
    size: metadata.size,
    sha256: createHash('sha256').update(contents).digest('hex')
  };
}

async function debAsset(directory, version) {
  const filename = `zmydia_${version}_amd64.deb`;
  const path = join(directory, filename);
  const [contents, metadata] = await Promise.all([readFile(path), stat(path)]);
  if (!metadata.isFile() || metadata.size === 0) throw new Error(`invalid release asset: ${path}`);
  return {
    kind: 'deb',
    url: `https://github.com/alissonlima086/ZMydia/releases/download/v${version}/${filename}`,
    size: metadata.size,
    sha256: createHash('sha256').update(contents).digest('hex')
  };
}

const args = parseArgs(process.argv.slice(2));
const assetsDirectory = resolve(args.get('assets') || 'release-assets');
const outputDirectory = resolve(args.get('output') || assetsDirectory);
const version = args.get('version');
const minimumUpdaterVersion = args.get('min-updater-version') || '0.1.1';
const publishedAt = args.get('published-at') || new Date().toISOString();

if (!version || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error('invalid --version');
}
if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(minimumUpdaterVersion)) {
  throw new Error('invalid --min-updater-version');
}

const privateKeyBase64 = process.env.ZMYDIA_UPDATE_SIGNING_KEY?.trim();
const expectedPublicKeyBase64 = process.env.ZMYDIA_UPDATE_PUBLIC_KEY?.trim();
if (!privateKeyBase64 || !expectedPublicKeyBase64) {
  throw new Error('ZMYDIA_UPDATE_SIGNING_KEY and ZMYDIA_UPDATE_PUBLIC_KEY are required');
}

const privateKey = createPrivateKey({
  key: Buffer.from(privateKeyBase64, 'base64'),
  format: 'der',
  type: 'pkcs8'
});
if (privateKey.asymmetricKeyType !== 'ed25519') throw new Error('signing key must be Ed25519');

const publicKeyDer = createPublicKey(privateKey).export({ format: 'der', type: 'spki' });
const ed25519SpkiPrefix = Buffer.from('302a300506032b6570032100', 'hex');
if (!publicKeyDer.subarray(0, ed25519SpkiPrefix.length).equals(ed25519SpkiPrefix)) {
  throw new Error('unexpected Ed25519 public key encoding');
}
const publicKey = publicKeyDer.subarray(ed25519SpkiPrefix.length);
const expectedPublicKey = Buffer.from(expectedPublicKeyBase64, 'base64');
if (publicKey.length !== 32 || expectedPublicKey.length !== 32 || !publicKey.equals(expectedPublicKey)) {
  throw new Error('private signing key does not match ZMYDIA_UPDATE_PUBLIC_KEY');
}

const manifest = {
  schema: 1,
  channel: 'stable',
  version,
  published_at: publishedAt,
  min_updater_version: minimumUpdaterVersion,
  notes_url: `https://github.com/alissonlima086/ZMydia/releases/tag/v${version}`,
  assets: {
    'windows-x86_64': await asset(
      assetsDirectory,
      version,
      'windows-x86_64',
      'nsis',
      '-installer.exe'
    ),
    'linux-x86_64-user': await asset(
      assetsDirectory,
      version,
      'linux-x86_64',
      'tar',
      '.tar.gz'
    ),
    'linux-x86_64-deb': await debAsset(assetsDirectory, version)
  }
};

const manifestBytes = Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`);
const signature = sign(null, manifestBytes, privateKey);
if (signature.length !== 64) throw new Error('unexpected Ed25519 signature length');

const manifestPath = join(outputDirectory, 'zmydia-update.json');
const signaturePath = `${manifestPath}.sig`;
await Promise.all([
  writeFile(manifestPath, manifestBytes, { flag: 'wx' }),
  writeFile(signaturePath, `${signature.toString('base64')}\n`, { flag: 'wx' })
]);

console.log(JSON.stringify({
  manifest: basename(manifestPath),
  signature: basename(signaturePath),
  version
}));

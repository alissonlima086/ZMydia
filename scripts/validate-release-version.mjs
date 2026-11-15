#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { basename, resolve } from 'node:path';

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

const args = parseArgs(process.argv.slice(2));
const version = args.get('version');
const binary = args.get('binary');
const packageDirectory = args.get('package');
const platform = args.get('platform');
const cargoManifest = await readFile(resolve('Cargo.toml'), 'utf8');
const workspaceVersion = cargoManifest.match(/\[workspace\.package\][\s\S]*?\nversion\s*=\s*"([^"]+)"/)?.[1];

if (!version || !workspaceVersion) throw new Error('release or workspace version not found');
if (process.env.GITHUB_REF_TYPE === 'tag') {
  const expectedTag = `v${workspaceVersion}`;
  if (process.env.GITHUB_REF_NAME !== expectedTag || version !== workspaceVersion) {
    throw new Error(
      `release version mismatch: tag=${process.env.GITHUB_REF_NAME}, workspace=${workspaceVersion}, build=${version}`
    );
  }
  const updatePublicKey = process.env.ZMYDIA_UPDATE_PUBLIC_KEY?.trim();
  if (!updatePublicKey || Buffer.from(updatePublicKey, 'base64').length !== 32) {
    throw new Error('tagged releases require a 32-byte ZMYDIA_UPDATE_PUBLIC_KEY');
  }
} else if (!version.startsWith(`${workspaceVersion}-dev.`)) {
  throw new Error(`development version must start with ${workspaceVersion}-dev.`);
}

if (binary) {
  const result = spawnSync(resolve(binary), ['--version'], { encoding: 'utf8' });
  if (result.status !== 0) throw new Error(`packaged binary --version failed: ${result.stderr}`);
  if (result.stdout.trim() !== version) {
    throw new Error(`binary reports ${result.stdout.trim()}, expected ${version}`);
  }
}

if (packageDirectory || platform) {
  if (!packageDirectory || !platform) throw new Error('--package and --platform must be used together');
  const directory = resolve(packageDirectory);
  const expectedName = `zmydia-${version}-${platform}`;
  if (basename(directory) !== expectedName) {
    throw new Error(`package directory must be named ${expectedName}`);
  }
  const buildInfo = JSON.parse(await readFile(resolve(directory, 'BUILD-INFO.json'), 'utf8'));
  if (buildInfo.version !== version || buildInfo.platform !== platform) {
    throw new Error('BUILD-INFO.json does not match the release package');
  }
}

console.log(JSON.stringify({ version, workspaceVersion, validated: true }));

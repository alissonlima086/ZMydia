import assert from 'node:assert/strict';
import { generateKeyPairSync, verify } from 'node:crypto';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { promisify } from 'node:util';
import { execFile } from 'node:child_process';
import test from 'node:test';

const execFileAsync = promisify(execFile);
const cargoManifest = await readFile(new URL('../../Cargo.toml', import.meta.url), 'utf8');
const workspaceVersion = cargoManifest.match(
  /\[workspace\.package\][\s\S]*?\nversion\s*=\s*"([^"]+)"/
)?.[1];

assert.ok(workspaceVersion, 'workspace version not found in Cargo.toml');

test('gera manifesto de update assinado com assets exatos', async (context) => {
  const root = await mkdtemp(join(tmpdir(), 'zmydia-update-manifest-'));
  context.after(() => rm(root, { recursive: true, force: true }));
  const assets = join(root, 'assets');
  await mkdir(assets);
  await Promise.all([
    writeFile(join(assets, `zmydia-${workspaceVersion}-windows-x86_64-installer.exe`), 'windows'),
    writeFile(join(assets, `zmydia-${workspaceVersion}-linux-x86_64.tar.gz`), 'linux'),
    writeFile(join(assets, `zmydia_${workspaceVersion}_amd64.deb`), 'debian')
  ]);

  const { privateKey, publicKey } = generateKeyPairSync('ed25519');
  const privateDer = privateKey.export({ format: 'der', type: 'pkcs8' });
  const publicDer = publicKey.export({ format: 'der', type: 'spki' });
  const publicRaw = publicDer.subarray(-32);
  await execFileAsync(process.execPath, [
    new URL('../generate-update-manifest.mjs', import.meta.url).pathname,
    '--assets', assets,
    '--output', root,
    '--version', workspaceVersion,
    '--published-at', '2026-11-20T12:00:00Z'
  ], {
    env: {
      ...process.env,
      ZMYDIA_UPDATE_SIGNING_KEY: privateDer.toString('base64'),
      ZMYDIA_UPDATE_PUBLIC_KEY: publicRaw.toString('base64')
    }
  });

  const manifestBytes = await readFile(join(root, 'zmydia-update.json'));
  const signature = Buffer.from(
    (await readFile(join(root, 'zmydia-update.json.sig'), 'utf8')).trim(),
    'base64'
  );
  const manifest = JSON.parse(manifestBytes);

  assert.equal(verify(null, manifestBytes, publicKey, signature), true);
  assert.equal(manifest.version, workspaceVersion);
  assert.equal(manifest.assets['windows-x86_64'].kind, 'nsis');
  assert.equal(manifest.assets['windows-x86_64'].size, 7);
  assert.equal(manifest.assets['linux-x86_64-user'].kind, 'tar');
  assert.equal(manifest.assets['linux-x86_64-user'].size, 5);
  assert.equal(manifest.assets['linux-x86_64-deb'].kind, 'deb');
  assert.equal(manifest.assets['linux-x86_64-deb'].size, 6);
  assert.match(manifest.assets['windows-x86_64'].sha256, /^[0-9a-f]{64}$/);
});

test('valida a identidade de builds de desenvolvimento e releases tagueadas', async () => {
  const validator = new URL('../validate-release-version.mjs', import.meta.url).pathname;
  const developmentEnv = { ...process.env };
  delete developmentEnv.GITHUB_REF_TYPE;
  delete developmentEnv.GITHUB_REF_NAME;
  await execFileAsync(process.execPath, [validator, '--version', `${workspaceVersion}-dev.42`], {
    env: developmentEnv
  });

  await execFileAsync(process.execPath, [validator, '--version', workspaceVersion], {
    env: {
      ...developmentEnv,
      GITHUB_REF_TYPE: 'tag',
      GITHUB_REF_NAME: `v${workspaceVersion}`,
      ZMYDIA_UPDATE_PUBLIC_KEY: Buffer.alloc(32, 9).toString('base64')
    }
  });

  await assert.rejects(
    execFileAsync(process.execPath, [validator, '--version', workspaceVersion], {
      env: {
        ...developmentEnv,
        GITHUB_REF_TYPE: 'tag',
        GITHUB_REF_NAME: 'v999.0.0',
        ZMYDIA_UPDATE_PUBLIC_KEY: Buffer.alloc(32, 9).toString('base64')
      }
    }),
    /release version mismatch/
  );
});

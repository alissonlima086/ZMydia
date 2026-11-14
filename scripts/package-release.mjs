#!/usr/bin/env node

import { chmod, cp, mkdir, readFile, rm, stat, writeFile } from 'node:fs/promises';
import { basename, dirname, join, resolve } from 'node:path';

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

async function requireFile(path, label) {
  const info = await stat(path).catch(() => null);
  if (!info?.isFile()) throw new Error(`${label} not found: ${path}`);
}

async function copyIfPresent(source, destination) {
  const info = await stat(source).catch(() => null);
  if (info) {
    await mkdir(dirname(destination), { recursive: true });
    await cp(source, destination, { recursive: true });
  }
}

const args = parseArgs(process.argv.slice(2));
const platform = args.get('platform');
const version = args.get('version');
const binary = resolve(args.get('binary') ?? '');
const runtime = resolve(args.get('runtime') ?? '');
const output = resolve(args.get('output') ?? 'dist');

if (!['linux-x86_64', 'windows-x86_64'].includes(platform)) {
  throw new Error('--platform must be linux-x86_64 or windows-x86_64');
}
if (!version || !/^[0-9A-Za-z._+-]+$/.test(version)) throw new Error('invalid --version');

const isWindows = platform.startsWith('windows');
const executableSuffix = isWindows ? '.exe' : '';
const packageName = `zmydia-${version}-${platform}`;
const packageRoot = join(output, packageName);
const appBin = join(packageRoot, 'bin');
const assetRoot = join(packageRoot, 'lib', 'mymedia');
const runtimeBin = join(assetRoot, 'bin');

await requireFile(binary, 'ZMydia binary');
for (const command of ['ffmpeg', 'ffprobe', 'yt-dlp', 'curl']) {
  await requireFile(join(runtime, 'bin', `${command}${executableSuffix}`), `bundled ${command}`);
}
await requireFile(resolve('frontend/dist/index.html'), 'frontend build');

await rm(packageRoot, { recursive: true, force: true });
await mkdir(appBin, { recursive: true });
await mkdir(assetRoot, { recursive: true });

await cp(binary, join(appBin, `zmydia-server${executableSuffix}`));
await cp(resolve('frontend/dist'), join(assetRoot, 'frontend'), { recursive: true });
await cp(join(runtime, 'bin'), runtimeBin, { recursive: true });
await copyIfPresent(join(runtime, 'licenses'), join(packageRoot, 'licenses'));

for (const directory of ['background', 'content', 'icons', 'popup']) {
  await cp(resolve('extension', directory), join(assetRoot, 'extension', directory), { recursive: true });
}
await cp(resolve('extension/manifest.json'), join(assetRoot, 'extension/manifest.json'));
await copyIfPresent(resolve('extension/signed'), join(assetRoot, 'extension/signed'));

for (const document of ['README.md', 'LICENSE', 'THIRD-PARTY-NOTICES.md']) {
  await cp(resolve(document), join(packageRoot, basename(document)));
}

const versionsPath = join(runtime, 'versions.json');
const runtimeVersions = JSON.parse(await readFile(versionsPath, 'utf8'));
await writeFile(
  join(packageRoot, 'BUILD-INFO.json'),
  `${JSON.stringify({
    version,
    platform,
    commit: process.env.GITHUB_SHA ?? null,
    built_at: process.env.SOURCE_DATE_EPOCH ?? new Date().toISOString(),
    runtime: runtimeVersions
  }, null, 2)}\n`
);

if (isWindows) {
  await writeFile(
    join(packageRoot, 'start-zmydia.cmd'),
    '@echo off\r\nsetlocal\r\nset "PATH=%~dp0lib\\mymedia\\bin;%PATH%"\r\n"%~dp0bin\\zmydia-server.exe" serve\r\n'
  );
} else {
  const launcher = join(packageRoot, 'start-zmydia.sh');
  await writeFile(
    launcher,
    '#!/usr/bin/env sh\nset -eu\nROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)\nPATH="$ROOT/lib/mymedia/bin:$PATH" exec "$ROOT/bin/zmydia-server" serve\n'
  );
  await chmod(launcher, 0o755);
  await chmod(join(appBin, 'zmydia-server'), 0o755);
  for (const command of ['ffmpeg', 'ffprobe', 'yt-dlp', 'curl']) {
    await chmod(join(runtimeBin, command), 0o755);
  }
}

console.log(packageRoot);

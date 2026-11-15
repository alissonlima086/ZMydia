import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

test('NSIS stops the scheduled service before replacing Windows files', async () => {
  const source = await readFile(new URL('../../installer/windows.nsi', import.meta.url), 'utf8');
  const stop = source.indexOf('-Action uninstall');
  const copy = source.indexOf('File /r "${PACKAGE_DIR}\\*"');
  const start = source.indexOf('-Action install');

  assert.ok(stop >= 0, 'pre-update stop command is missing');
  assert.ok(copy > stop, 'package files must only be replaced after the service stops');
  assert.ok(start > copy, 'the new scheduled service must be registered after files are replaced');
  assert.match(source, /Rename "\$INSTDIR" "\$INSTDIR\.previous"/);
  assert.match(source, /ExpectedVersion/);
});

test('Windows task removal waits for the executable to exit', async () => {
  const source = await readFile(new URL('../windows-task.ps1', import.meta.url), 'utf8');
  const stop = source.indexOf('Stop-ScheduledTask');
  const processWait = source.indexOf('Get-CimInstance Win32_Process');
  const timeout = source.indexOf("did not stop within 30 seconds");

  assert.ok(stop >= 0);
  assert.ok(processWait > stop);
  assert.ok(timeout > processWait);
  assert.match(source, /api\/health\/ready/);
  assert.match(source, /health\.version -eq \$ExpectedVersion/);
});

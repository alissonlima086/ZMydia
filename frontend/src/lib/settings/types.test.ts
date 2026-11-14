import { afterEach, describe, expect, it, vi } from 'vitest';
import { request } from './types';

describe('settings request errors', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('extracts the message from a structured API error', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json({
      error: { code: 'invalid_source', message: 'Generic source validation error' }
    }, { status: 400 })));

    await expect(request('/api/example')).rejects.toThrow('Generic source validation error');
  });
});

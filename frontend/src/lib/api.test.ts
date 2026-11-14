import { afterEach, describe, expect, it, vi } from 'vitest';

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.resetModules();
});

describe('API client', () => {
  it('adds a CSRF token to mutating requests', async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(Response.json({ token: 'csrf-123' }))
      .mockResolvedValueOnce(Response.json({ saved: true }));
    vi.stubGlobal('fetch', fetchMock);
    const { apiFetch } = await import('./api');

    const response = await apiFetch('/api/settings', { method: 'POST' });

    expect(response.ok).toBe(true);
    expect(fetchMock).toHaveBeenCalledTimes(2);
    const request = fetchMock.mock.calls[1];
    expect(request[0]).toBe('/api/settings');
    expect(new Headers(request[1]?.headers).get('X-CSRF-Token')).toBe('csrf-123');
  });

  it('refreshes a stale CSRF token once after a forbidden mutation', async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(Response.json({ token: 'stale' }))
      .mockResolvedValueOnce(new Response(null, { status: 403 }))
      .mockResolvedValueOnce(Response.json({ token: 'fresh' }))
      .mockResolvedValueOnce(Response.json({ saved: true }));
    vi.stubGlobal('fetch', fetchMock);
    const { apiFetch } = await import('./api');

    const response = await apiFetch('/api/settings', { method: 'PUT' });

    expect(response.ok).toBe(true);
    expect(fetchMock).toHaveBeenCalledTimes(4);
    expect(new Headers(fetchMock.mock.calls[3][1]?.headers).get('X-CSRF-Token')).toBe('fresh');
  });

  it('notifies the auth store for protected unauthorized requests', async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(new Response(null, { status: 401 }));
    vi.stubGlobal('fetch', fetchMock);
    const { apiFetch, setUnauthorizedHandler } = await import('./api');
    const handler = vi.fn();
    setUnauthorizedHandler(handler);

    await apiFetch('/api/media');

    expect(handler).toHaveBeenCalledOnce();
  });

  it('returns typed JSON and normalizes legacy error envelopes', async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(Response.json({ items: [1, 2, 3] }))
      .mockResolvedValueOnce(Response.json({ error: 'registro não encontrado' }, { status: 404 }));
    vi.stubGlobal('fetch', fetchMock);
    const { apiJson } = await import('./api');

    await expect(apiJson<{ items: number[] }>('/api/items')).resolves.toEqual({ items: [1, 2, 3] });
    await expect(apiJson('/api/missing')).rejects.toEqual(
      expect.objectContaining({
        name: 'ApiRequestError',
        status: 404,
        message: 'registro não encontrado'
      })
    );
  });

  it('preserves structured error metadata and handles empty success responses', async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(new Response(null, { status: 204 }))
      .mockResolvedValueOnce(
        Response.json(
          { error: { code: 'invalid_source', message: 'fonte inválida', details: { field: 'path' } } },
          { status: 422 }
        )
      );
    vi.stubGlobal('fetch', fetchMock);
    const { apiJson } = await import('./api');

    await expect(apiJson<void>('/api/empty')).resolves.toBeUndefined();
    await expect(apiJson('/api/source')).rejects.toEqual(
      expect.objectContaining({
        status: 422,
        code: 'invalid_source',
        message: 'fonte inválida',
        details: { field: 'path' }
      })
    );
  });

  it('renders structured API errors as their message instead of object text', async () => {
    const { apiErrorMessage } = await import('./api');
    const response = Response.json(
      { error: { code: 'invalid_password', message: 'Senha atual incorreta' } },
      { status: 401 }
    );

    await expect(apiErrorMessage(response)).resolves.toBe('Senha atual incorreta');
  });

  it('falls back to HTTP status text for non-JSON errors', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn<typeof fetch>().mockResolvedValue(new Response('bad gateway', { status: 502, statusText: 'Bad Gateway' }))
    );
    const { apiJson } = await import('./api');

    await expect(apiJson('/api/unavailable')).rejects.toEqual(
      expect.objectContaining({ status: 502, message: 'Bad Gateway' })
    );
  });

  it('does not request CSRF for reads or notify on login failures', async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(new Response(null, { status: 401 }));
    vi.stubGlobal('fetch', fetchMock);
    const { apiFetch, setUnauthorizedHandler } = await import('./api');
    const handler = vi.fn();
    setUnauthorizedHandler(handler);

    await apiFetch('/api/auth/login');

    expect(fetchMock).toHaveBeenCalledOnce();
    expect(handler).not.toHaveBeenCalled();
  });

  it('implements typed reads and serializes mutating helpers', async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(Response.json({ status: 'ok' }))
      .mockResolvedValueOnce(Response.json({ token: 'csrf-wrapper' }))
      .mockResolvedValueOnce(Response.json({ id: 1 }))
      .mockResolvedValueOnce(Response.json({ id: 2 }))
      .mockResolvedValueOnce(Response.json({ id: 3 }))
      .mockResolvedValueOnce(new Response(null, { status: 204 }));
    vi.stubGlobal('fetch', fetchMock);
    const { api } = await import('./api');

    await expect(api.get<{ status: string }>('/api/health')).resolves.toEqual({ status: 'ok' });
    await api.post('/api/items', { title: 'novo' });
    await api.put('/api/items/1', { title: 'inteiro' });
    await api.patch('/api/items/1', { title: 'parcial' });
    await api.delete('/api/items/1');

    expect(fetchMock).toHaveBeenCalledTimes(6);
    expect(fetchMock.mock.calls[0][1]?.method).toBe('GET');
    expect(fetchMock.mock.calls[2][1]?.body).toBe('{"title":"novo"}');
    expect(fetchMock.mock.calls[3][1]?.body).toBe('{"title":"inteiro"}');
    expect(fetchMock.mock.calls[4][1]?.body).toBe('{"title":"parcial"}');
    expect(fetchMock.mock.calls[5][1]?.body).toBeUndefined();
  });
});

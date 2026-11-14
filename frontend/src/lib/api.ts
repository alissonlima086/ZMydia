import type { ApiErrorDetail, ApiErrorEnvelope } from './api-contract';

export type { ApiErrorDetail, ApiErrorEnvelope } from './api-contract';

let cachedCsrfToken: string | null = null;
let onUnauthorizedHandler: (() => void) | null = null;

type CompatibleErrorEnvelope = { error: string | ApiErrorEnvelope['error'] };

export class ApiRequestError extends Error {
  readonly status: number;
  readonly code?: string;
  readonly details?: unknown;

  constructor(status: number, detail: ApiErrorDetail) {
    super(detail.message);
    this.name = 'ApiRequestError';
    this.status = status;
    this.code = detail.code;
    this.details = detail.details;
  }
}

export function setUnauthorizedHandler(fn: () => void) {
  onUnauthorizedHandler = fn;
}

export async function getCsrfToken(forceRefresh = false): Promise<string> {
  if (!forceRefresh && cachedCsrfToken) return cachedCsrfToken;
  try {
    const res = await fetch('/api/auth/csrf');
    if (res.ok) {
      const data = (await res.json()) as { token: string };
      cachedCsrfToken = data.token;
      return cachedCsrfToken;
    }
  } catch {
    // Fallback silencioso caso a API ainda esteja iniciando
  }
  return '';
}

export async function apiFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  const method = (init?.method ?? 'GET').toUpperCase();
  const isMutating = ['POST', 'PUT', 'PATCH', 'DELETE'].includes(method);

  const headers = new Headers(init?.headers);
  let csrfToken = '';
  if (isMutating) {
    csrfToken = await getCsrfToken();
    if (csrfToken) {
      headers.set('X-CSRF-Token', csrfToken);
    }
  }

  let res = await fetch(input, {
    ...init,
    headers
  });

  // Se uma requisição com mutação retornar 403, pode ser CSRF desatualizado (servidor reiniciou).
  // Tenta renovar o token e repetir a requisição automaticamente.
  if (isMutating && res.status === 403) {
    const freshToken = await getCsrfToken(true);
    if (freshToken && freshToken !== csrfToken) {
      headers.set('X-CSRF-Token', freshToken);
      res = await fetch(input, {
        ...init,
        headers
      });
    }
  }

  if (res.status === 401) {
    const urlStr =
      typeof input === 'string'
        ? input
        : input instanceof URL
        ? input.pathname
        : (input as Request).url;
    if (!urlStr.includes('/api/auth/login') && !urlStr.includes('/api/auth/status')) {
      onUnauthorizedHandler?.();
    }
  }

  return res;
}

async function readError(response: Response): Promise<ApiErrorDetail> {
  try {
    const payload = (await response.clone().json()) as Partial<CompatibleErrorEnvelope>;
    if (typeof payload.error === 'string') return { message: payload.error };
    if (payload.error && typeof payload.error.message === 'string') return payload.error;
  } catch {
    // Respostas antigas e erros de proxy podem não conter JSON.
  }
  return { message: response.statusText || `HTTP ${response.status}` };
}

export async function apiErrorMessage(response: Response): Promise<string> {
  return (await readError(response)).message;
}

export async function apiJson<T>(input: RequestInfo | URL, init?: RequestInit): Promise<T> {
  const response = await apiFetch(input, init);
  if (!response.ok) throw new ApiRequestError(response.status, await readError(response));
  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

export const api = {
  response: apiFetch,
  json: apiJson,
  get<T>(url: string, init?: RequestInit): Promise<T> {
    return apiJson<T>(url, { ...init, method: 'GET' });
  },
  post<T>(url: string, body?: unknown, init?: RequestInit): Promise<T> {
    return apiJson<T>(url, jsonRequest('POST', body, init));
  },
  put<T>(url: string, body?: unknown, init?: RequestInit): Promise<T> {
    return apiJson<T>(url, jsonRequest('PUT', body, init));
  },
  patch<T>(url: string, body?: unknown, init?: RequestInit): Promise<T> {
    return apiJson<T>(url, jsonRequest('PATCH', body, init));
  },
  delete<T = void>(url: string, init?: RequestInit): Promise<T> {
    return apiJson<T>(url, { ...init, method: 'DELETE' });
  }
};

function jsonRequest(method: string, body: unknown, init?: RequestInit): RequestInit {
  const headers = new Headers(init?.headers);
  if (body !== undefined) headers.set('Content-Type', 'application/json');
  return {
    ...init,
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body)
  };
}

export async function updateMediaNotes(
  mediaId: number,
  notes: string | null
): Promise<{ user_notes: string | null }> {
  return apiJson<{ user_notes: string | null }>(`/api/media/${mediaId}/notes`, {
    method: 'PUT',
    headers: {
      'Content-Type': 'application/json'
    },
    body: JSON.stringify({ notes })
  });
}

export interface ApiErrorDetail {
  code?: string;
  message: string;
  details?: unknown;
}

export interface ApiErrorEnvelope {
  error: ApiErrorDetail;
}

export interface CursorPage<T> {
  items: T[];
  next_cursor: string | null;
}

export interface OffsetPage<T> {
  items: T[];
  limit: number;
  offset: number;
  total?: number;
}

export type AsyncState<T> =
  | { status: 'idle' }
  | { status: 'loading'; previous?: T }
  | { status: 'success'; data: T }
  | { status: 'error'; error: ApiErrorDetail; previous?: T };

export interface ApiRoutes {
  '/api/health': { GET: { status: string } };
  '/api/auth/csrf': { GET: { token: string } };
  '/api/media': { GET: unknown[] };
  '/api/feed': { GET: CursorPage<unknown> & { seed: number } };
  '/api/search': { GET: unknown[] };
  '/api/tags': { GET: unknown[] };
  '/api/channels': { GET: unknown[] };
  '/api/playlists': { GET: unknown[] };
  '/api/favorites': { GET: unknown[] };
  '/api/history': { GET: unknown[]; DELETE: void };
  '/api/downloads': { GET: unknown[]; POST: unknown };
}

export type ApiPath = keyof ApiRoutes;
export type ApiMethod<P extends ApiPath> = keyof ApiRoutes[P];
export type ApiResponse<P extends ApiPath, M extends ApiMethod<P>> = ApiRoutes[P][M];

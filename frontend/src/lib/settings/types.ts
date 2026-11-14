import { apiErrorMessage, apiFetch } from '../api';
import { i18n, t } from '../i18n/i18n.svelte';

export type MediaSource = {
  id: number;
  path: string;
  recursive: boolean;
  enabled: boolean;
  mode: 'read_only' | 'managed';
  last_scanned_at: string | null;
  status: 'pending' | 'scanning' | 'ready' | 'error' | 'disabled';
  error_message: string | null;
  file_count: number;
  available?: boolean;
  readable?: boolean;
  writable?: boolean;
  free_space_bytes?: number | null;
};

export type DashboardStats = {
  total_videos: number;
  official_thumbnails: number;
  generated_thumbnails: number;
  with_youtube_id: number;
  videos_with_subtitles: number;
  total_subtitles: number;
  total_channels: number;
  total_playlists: number;
  total_favorites: number;
  total_size_bytes: number;
  total_duration_ms: number;
};

export type YoutubeSettings = {
  use_browser_cookies: boolean;
  browser: string;
};

export type YoutubeStatus = {
  is_running: boolean;
  total_candidates: number;
  processed: number;
  successful: number;
  failed: number;
  bot_blocked?: boolean;
  current_title: string | null;
  last_run_at: string | null;
};

export type HomeSettings = {
  hidden_tags: string[];
};

export type RecommendationSettings = {
  enabled: boolean;
  max_videos_per_channel: number;
  boost_unwatched: boolean;
  unwatched_bonus: number;
  fatigue_filter: boolean;
  serendipity_percent: number;
};

export type DownloadSettings = {
  target_source_id: number | null;
  preferred_container: string;
};

export type ListenBrainzSettings = {
  enabled: boolean;
  user_token: string;
  username: string | null;
  auto_scrobble: boolean;
  has_token?: boolean;
  token_hint?: string;
};

export type TokenValidationResult = {
  valid: boolean;
  username: string | null;
  message: string;
};

export type ExtensionInfo = {
  has_signed: boolean;
  has_unsigned: boolean;
  active_type: 'signed' | 'unsigned' | null;
  download_url: string | null;
};

export type ApiTokenItem = {
  id: number;
  name: string;
  token_hint: string;
  scopes: string;
  last_used_at: string | null;
  created_at: string;
  expires_at: string | null;
};

export type CreateApiTokenResponse = {
  id: number;
  name: string;
  token: string;
  token_hint: string;
  scopes: string;
  created_at: string;
  expires_at: string;
};

export type SubtitleSettings = {
  auto_download: boolean;
  languages: string[];
  include_auto_generated: boolean;
};

export type TagSummary = {
  id: number;
  display_name: string;
  normalized_name: string;
  media_count: number;
};

export const COMMON_SUB_LANGS = [
  { code: 'pt', label: 'Português' },
  { code: 'en', label: 'Inglês' },
  { code: 'es', label: 'Espanhol' },
  { code: 'ja', label: 'Japonês' },
  { code: 'fr', label: 'Francês' },
  { code: 'de', label: 'Alemão' },
  { code: 'it', label: 'Italiano' },
  { code: 'ko', label: 'Coreano' }
];

export const STATUS_LABELS: Record<MediaSource['status'], string> = {
  pending: 'Na fila',
  scanning: 'Escaneando',
  ready: 'Atualizada',
  error: 'Com erro',
  disabled: 'Desativada'
};

export function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`;
}

export function formatDuration(ms: number): string {
  if (!ms || ms <= 0) return '0m';
  const totalSec = Math.floor(ms / 1000);
  const hours = Math.floor(totalSec / 3600);
  const minutes = Math.floor((totalSec % 3600) / 60);
  if (hours > 0) {
    return `${hours}h ${minutes}m`;
  }
  return `${minutes}m`;
}

export function formatDate(value: string | null): string {
  if (!value) return t('settings.folders.never_scanned');
  return new Intl.DateTimeFormat(i18n.currentLocale, {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit'
  }).format(new Date(value));
}

export function messageFrom(error: unknown): string {
  return error instanceof Error ? error.message : t('common.operation_failed');
}

export async function request<T>(url: string, options?: RequestInit): Promise<T> {
  const response = await apiFetch(url, {
    ...options,
    headers: options?.body ? { 'content-type': 'application/json', ...options?.headers } : options?.headers
  });
  if (!response.ok) {
    throw new Error(await apiErrorMessage(response));
  }
  if (response.status === 204 || response.status === 202) {
    const text = await response.text();
    if (!text) return undefined as T;
    try {
      return JSON.parse(text) as T;
    } catch {
      return undefined as T;
    }
  }
  const text = await response.text();
  if (!text) return undefined as T;
  return JSON.parse(text) as T;
}

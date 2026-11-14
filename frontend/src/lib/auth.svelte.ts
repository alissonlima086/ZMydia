import { apiErrorMessage, apiFetch, setUnauthorizedHandler } from './api';
import { t } from './i18n/i18n.svelte';

export interface AuthStatusResponse {
  authenticated: boolean;
  username: string | null;
  setup_required: boolean;
  must_change_password: boolean;
}

class AuthStore {
  authenticated = $state<boolean>(false);
  username = $state<string | null>(null);
  loading = $state<boolean>(true);
  error = $state<string | null>(null);
  setupRequired = $state<boolean>(false);
  recoveryCode = $state<string | null>(null);
  private pendingSetupCredentials: { username: string; password: string } | null = null;

  constructor() {
    setUnauthorizedHandler(() => {
      this.setUnauthenticated();
    });
  }

  async checkStatus(): Promise<boolean> {
    this.loading = true;
    try {
      const res = await apiFetch('/api/auth/status');
      if (res.ok) {
        const data: AuthStatusResponse = await res.json();
        this.authenticated = data.authenticated;
        this.username = data.username;
        this.setupRequired = data.setup_required;
        return data.authenticated;
      } else {
        this.authenticated = false;
        this.username = null;
        return false;
      }
    } catch {
      this.authenticated = false;
      this.username = null;
      return false;
    } finally {
      this.loading = false;
    }
  }

  async setup(username: string, password: string): Promise<boolean> {
    this.loading = true;
    this.error = null;
    try {
      const response = await apiFetch('/api/auth/setup', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ username, password })
      });
      if (!response.ok) {
        this.error = await apiErrorMessage(response);
        return false;
      }
      const result = (await response.json()) as { recovery_code: string };
      this.setupRequired = false;
      this.recoveryCode = result.recovery_code;
      this.pendingSetupCredentials = { username, password };
      return true;
    } finally {
      this.loading = false;
    }
  }

  async finishSetup(): Promise<boolean> {
    const credentials = this.pendingSetupCredentials;
    if (!credentials) return false;
    this.recoveryCode = null;
    this.pendingSetupCredentials = null;
    return this.login(credentials.username, credentials.password, false);
  }

  async recover(username: string, recoveryCode: string, newPassword: string): Promise<string | null> {
    this.loading = true;
    this.error = null;
    try {
      const response = await apiFetch('/api/auth/recover', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ username, recovery_code: recoveryCode, new_password: newPassword })
      });
      if (!response.ok) {
        this.error = await apiErrorMessage(response);
        return null;
      }
      const result = (await response.json()) as { recovery_code: string };
      return result.recovery_code;
    } catch (error: any) {
      this.error = error.message || t('auth.connection_error');
      return null;
    } finally {
      this.loading = false;
    }
  }

  async login(username: string, password: string, rememberMe = false): Promise<boolean> {
    this.loading = true;
    this.error = null;
    try {
      const res = await apiFetch('/api/auth/login', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ username, password, remember_me: rememberMe })
      });

      if (res.ok) {
        const data = await res.json();
        this.authenticated = true;
        this.username = data.username ?? username;
        this.error = null;
        return true;
      }

      if (res.status === 429) {
        this.error = t('auth.too_many_attempts');
      } else if (res.status === 401) {
        this.error = t('auth.invalid_credentials');
      } else {
        this.error = await apiErrorMessage(res);
      }
      return false;
    } catch (e: any) {
      this.error = e.message || t('auth.connection_error');
      return false;
    } finally {
      this.loading = false;
    }
  }

  async logout(): Promise<void> {
    try {
      await apiFetch('/api/auth/logout', { method: 'POST' });
    } catch {
      // Ignora erro no logout
    } finally {
      this.authenticated = false;
      this.username = null;
    }
  }

  setUnauthenticated() {
    this.authenticated = false;
    this.username = null;
  }
}

export const authStore = new AuthStore();

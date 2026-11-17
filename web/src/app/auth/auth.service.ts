import { computed, Injectable, signal } from '@angular/core';

export const SESSION_COOKIE = 'ff_session';

interface AuthSessionResponse {
  player_id: number;
  session_token: string;
}

function readCookie(name: string): string | null {
  if (typeof document === 'undefined' || !document.cookie) {
    return null;
  }
  const entries = document.cookie.split(';');
  for (const raw of entries) {
    const trimmed = raw.trim();
    const eqIdx = trimmed.indexOf('=');
    if (eqIdx <= 0) {
      continue;
    }
    const key = trimmed.slice(0, eqIdx).trim();
    if (key === name) {
      const val = trimmed.slice(eqIdx + 1).trim();
      if (!val) {
        return null;
      }
      try {
        return decodeURIComponent(val);
      } catch {
        return val;
      }
    }
  }
  return null;
}

function writeSessionCookie(token: string): void {
  if (typeof document === 'undefined') {
    return;
  }
  document.cookie = `${SESSION_COOKIE}=${encodeURIComponent(token)}; Path=/; Max-Age=2592000; SameSite=Lax`;
}

function clearSessionCookie(): void {
  if (typeof document === 'undefined') {
    return;
  }
  document.cookie = `${SESSION_COOKIE}=; Path=/; Max-Age=0; SameSite=Lax`;
}

@Injectable({ providedIn: 'root' })
export class AuthService {
  readonly sessionToken = signal<string | null>(readCookie(SESSION_COOKIE));
  readonly hasSession = computed(() => !!this.sessionToken());

  refreshFromCookie(): void {
    this.sessionToken.set(readCookie(SESSION_COOKIE));
  }

  async startFreshPlay(): Promise<string> {
    const res = await fetch('/api/auth/play', {
      method: 'POST',
      credentials: 'same-origin',
    });
    if (!res.ok) {
      throw new Error(`Could not start a new session (HTTP ${res.status}).`);
    }
    const data = (await res.json()) as AuthSessionResponse;
    writeSessionCookie(data.session_token);
    this.sessionToken.set(data.session_token);
    return data.session_token;
  }

  async login(username: string, password: string): Promise<string> {
    const res = await fetch('/api/auth/login', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      credentials: 'same-origin',
      body: JSON.stringify({ username, password }),
    });
    if (res.status === 401) {
      throw new Error('Invalid username or password.');
    }
    if (res.status === 400) {
      throw new Error('Please enter both a username and password.');
    }
    if (!res.ok) {
      throw new Error(`Login failed (HTTP ${res.status}).`);
    }
    const data = (await res.json()) as AuthSessionResponse;
    writeSessionCookie(data.session_token);
    this.sessionToken.set(data.session_token);
    return data.session_token;
  }

  async logout(notifyServer = true): Promise<void> {
    clearSessionCookie();
    this.sessionToken.set(null);
    if (notifyServer) {
      try {
        await fetch('/api/auth/logout', {
          method: 'POST',
          credentials: 'same-origin',
        });
      } catch {
        // Best-effort cookie/session revocation on server.
      }
    }
  }
}

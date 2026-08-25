export const APP_SESSION_COOKIE = "__Host-layerx-session";
export const APP_CSRF_COOKIE = "__Host-layerx_csrf";
export const ACTIVE_ACCOUNT_STORAGE_KEY = "layerx.account";

export interface VerifiedSessionIdentity {
  readonly accountId: string;
  readonly sessionId: string;
  readonly principalScope: string;
}

export function singleCurrentSessionId(
  sessions: readonly Readonly<{ session_id: string; current: boolean }>[],
): string | undefined {
  const current = sessions.filter((session) => session.current && session.session_id.length > 0);
  return current.length === 1 ? current[0]!.session_id : undefined;
}

export function csrfTokenFromCookie(cookie: string): string | undefined {
  for (const entry of cookie.split(";")) {
    const [name, ...value] = entry.trim().split("=");
    if (name === APP_CSRF_COOKIE) {
      const encoded = value.join("=");
      return encoded.length === 0 ? undefined : encoded;
    }
  }
  return undefined;
}

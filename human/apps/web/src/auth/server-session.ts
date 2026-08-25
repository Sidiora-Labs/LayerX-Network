import "server-only";

import { createHash, timingSafeEqual } from "node:crypto";

import { humanApiForServer } from "../api/server.ts";
import { csrfTokenFromCookie, singleCurrentSessionId, type VerifiedSessionIdentity } from "./session.ts";

export async function verifiedWebSession(cookie: string): Promise<VerifiedSessionIdentity | undefined> {
  if (cookie.length === 0) return undefined;
  try {
    const client = humanApiForServer({ cookie });
    const [balance, sessions] = await Promise.all([client.accountBalance(), client.sessionList()]);
    const sessionId = singleCurrentSessionId(sessions.sessions);
    if (sessionId === undefined || balance.account_id.length === 0) return undefined;
    return Object.freeze({
      accountId: balance.account_id,
      sessionId,
      principalScope: createHash("sha256")
        .update("layerx-web-principal-scope/v1\0", "utf8")
        .update(balance.account_id, "utf8")
        .digest("base64url"),
    });
  } catch {
    return undefined;
  }
}

export function validRequestCsrf(request: Request): boolean {
  const supplied = request.headers.get("x-layerx-csrf") ?? "";
  const expected = csrfTokenFromCookie(request.headers.get("cookie") ?? "") ?? "";
  if (supplied.length < 16 || expected.length < 16) return false;
  const suppliedDigest = createHash("sha256").update(supplied, "utf8").digest();
  const expectedDigest = createHash("sha256").update(expected, "utf8").digest();
  return timingSafeEqual(suppliedDigest, expectedDigest);
}

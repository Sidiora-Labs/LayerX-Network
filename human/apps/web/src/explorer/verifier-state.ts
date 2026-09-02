export type ExplorerVerifierRetryAfter =
  | Readonly<{ kind: "known"; seconds: number }>
  | Readonly<{ kind: "unknown" }>;

export type ExplorerVerifierFailure =
  | Readonly<{ kind: "refused" }>
  | Readonly<{ kind: "unavailable" }>
  | Readonly<{ kind: "divergent" }>
  | Readonly<{ kind: "overloaded"; retryAfter: ExplorerVerifierRetryAfter }>;

export const EXPLORER_VERIFIER_REFUSED_STATUSES: readonly number[] = Object.freeze([400, 413, 422]);

const UNKNOWN_RETRY_AFTER: ExplorerVerifierRetryAfter = Object.freeze({ kind: "unknown" });
const REFUSED: ExplorerVerifierFailure = Object.freeze({ kind: "refused" });
const UNAVAILABLE: ExplorerVerifierFailure = Object.freeze({ kind: "unavailable" });
const DIVERGENT: ExplorerVerifierFailure = Object.freeze({ kind: "divergent" });

export function explorerVerifierRetryAfter(header: string | null, now: number): ExplorerVerifierRetryAfter {
  if (header === null) {
    return UNKNOWN_RETRY_AFTER;
  }
  const value = header.trim();
  if (/^\d+$/.test(value)) {
    const seconds = Number(value);
    return Number.isSafeInteger(seconds) ? Object.freeze({ kind: "known", seconds }) : UNKNOWN_RETRY_AFTER;
  }
  const at = Date.parse(value);
  if (Number.isNaN(at)) {
    return UNKNOWN_RETRY_AFTER;
  }
  return Object.freeze({ kind: "known", seconds: Math.max(0, Math.ceil((at - now) / 1000)) });
}

export function explorerVerifierFailure(
  status: number,
  retryAfterHeader: string | null,
  now: number = Date.now(),
): ExplorerVerifierFailure {
  if (status === 429) {
    return Object.freeze({ kind: "overloaded", retryAfter: explorerVerifierRetryAfter(retryAfterHeader, now) });
  }
  if (status === 409) {
    return DIVERGENT;
  }
  if (EXPLORER_VERIFIER_REFUSED_STATUSES.includes(status)) {
    return REFUSED;
  }
  return UNAVAILABLE;
}

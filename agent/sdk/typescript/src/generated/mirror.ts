// Generated from platform/sdk/schema/mirror-v2.kvx.
export const MIRROR_SCHEMA_VERSION = 2 as const;
export const MIRROR_ARCHIVE_MAGIC = "4c584d4952524f52" as const;
export const MIRROR_MAX_ARCHIVE_BYTES = 67_108_864 as const;
export const MIRROR_MAX_SOURCES = 8 as const;
export const MIRROR_MAX_JSON_DEPTH = 32 as const;
export type MirrorPolicyKind = "exact" | "ordered-preference" | "agreement";
export type MirrorErrorCode =
  | "configuration" | "unavailable" | "rate-limited" | "missing"
  | "target-mismatch" | "source-mismatch" | "malformed" | "bounds"
  | "commitment" | "authorization" | "proof" | "checkpoint-unavailable"
  | "divergent" | "insufficient-agreement" | "reorged";

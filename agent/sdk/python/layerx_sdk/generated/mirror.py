"""Generated from platform/sdk/schema/mirror-v2.kvx."""

from enum import Enum

MIRROR_SCHEMA_VERSION = 2
MIRROR_ARCHIVE_MAGIC = bytes.fromhex("4c584d4952524f52")
MIRROR_MAX_ARCHIVE_BYTES = 67_108_864
MIRROR_MAX_SOURCES = 8
MIRROR_MAX_JSON_DEPTH = 32

class MirrorPolicyKind(str, Enum):
    EXACT = "exact"
    ORDERED_PREFERENCE = "ordered-preference"
    AGREEMENT = "agreement"

class MirrorErrorCode(str, Enum):
    CONFIGURATION = "configuration"
    UNAVAILABLE = "unavailable"
    RATE_LIMITED = "rate-limited"
    MISSING = "missing"
    TARGET_MISMATCH = "target-mismatch"
    SOURCE_MISMATCH = "source-mismatch"
    MALFORMED = "malformed"
    BOUNDS = "bounds"
    COMMITMENT = "commitment"
    AUTHORIZATION = "authorization"
    PROOF = "proof"
    CHECKPOINT_UNAVAILABLE = "checkpoint-unavailable"
    DIVERGENT = "divergent"
    INSUFFICIENT_AGREEMENT = "insufficient-agreement"
    REORGED = "reorged"

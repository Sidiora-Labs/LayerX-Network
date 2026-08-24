from enum import Enum
MIRROR_SCHEMA_VERSION: int
MIRROR_ARCHIVE_MAGIC: bytes
MIRROR_MAX_ARCHIVE_BYTES: int
MIRROR_MAX_SOURCES: int
MIRROR_MAX_JSON_DEPTH: int
class MirrorPolicyKind(str, Enum):
    EXACT: str
    ORDERED_PREFERENCE: str
    AGREEMENT: str
class MirrorErrorCode(str, Enum):
    CONFIGURATION: str
    UNAVAILABLE: str
    RATE_LIMITED: str
    MISSING: str
    TARGET_MISMATCH: str
    SOURCE_MISMATCH: str
    MALFORMED: str
    BOUNDS: str
    COMMITMENT: str
    AUTHORIZATION: str
    PROOF: str
    CHECKPOINT_UNAVAILABLE: str
    DIVERGENT: str
    INSUFFICIENT_AGREEMENT: str
    REORGED: str

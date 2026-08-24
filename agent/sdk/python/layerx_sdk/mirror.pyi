from pathlib import Path
from typing import Literal
from dataclasses import dataclass

@dataclass(frozen=True)
class MirrorCandidate:
    source: int
    commitment: bytes
@dataclass(frozen=True)
class MirrorPolicy:
    kind: Literal["exact", "ordered-preference", "agreement"]
    candidates: tuple[MirrorCandidate, ...]
    minimum: int = ...
@dataclass(frozen=True)
class MirrorVerification:
    level: str
    batch_number: int
    header_digest: bytes
    evidence_digest: bytes
    source_id: str
    target: str
    canonical_position: str
    provenance: str
    latest_batch: int | None
    batch_lag: str
    failover_count: int
    agreeing_sources: int
    checkpoint_level: Literal["unavailable"]
class MirrorVerificationError(RuntimeError):
    code: object
class MirrorVerifier:
    def __init__(self, executable: Path, configuration: Path, timeout_seconds: float = ...) -> None: ...
    def receipt(self, batch_number: int, policy: MirrorPolicy, canonical_receipt: bytes) -> MirrorVerification: ...
    def state(self, batch_number: int, policy: MirrorPolicy, canonical_state: bytes, canonical_proof: bytes) -> MirrorVerification: ...

"""Pinned mirror verification through the local production verifier executable."""

from __future__ import annotations

from dataclasses import dataclass
import json
from pathlib import Path
import subprocess
from typing import Literal

from .generated.mirror import MIRROR_MAX_SOURCES, MirrorErrorCode

_MAX_U64 = (1 << 64) - 1

@dataclass(frozen=True)
class MirrorCandidate:
    source: int
    commitment: bytes

@dataclass(frozen=True)
class MirrorPolicy:
    kind: Literal["exact", "ordered-preference", "agreement"]
    candidates: tuple[MirrorCandidate, ...]
    minimum: int = 1

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
    def __init__(self, code: MirrorErrorCode | str) -> None:
        super().__init__(f"mirror verification refused: {code}")
        self.code = code

class MirrorVerifier:
    def __init__(self, executable: Path, configuration: Path, timeout_seconds: float = 120.0) -> None:
        if not executable.is_absolute() or not configuration.is_absolute() or not (0.1 <= timeout_seconds <= 120.0):
            raise MirrorVerificationError(MirrorErrorCode.CONFIGURATION)
        self._executable = executable
        self._configuration = configuration
        self._timeout = timeout_seconds

    def receipt(self, batch_number: int, policy: MirrorPolicy, canonical_receipt: bytes) -> MirrorVerification:
        return self._verify(batch_number, policy, {"kind": "receipt", "canonical_hex": canonical_receipt.hex()})

    def state(self, batch_number: int, policy: MirrorPolicy, canonical_state: bytes, canonical_proof: bytes) -> MirrorVerification:
        return self._verify(batch_number, policy, {"kind": "state", "canonical_hex": canonical_state.hex(), "proof_hex": canonical_proof.hex()})

    def _verify(self, batch_number: int, policy: MirrorPolicy, evidence: dict[str, object]) -> MirrorVerification:
        if isinstance(batch_number, bool) or batch_number <= 0 or batch_number > _MAX_U64 or not policy.candidates or len(policy.candidates) > MIRROR_MAX_SOURCES:
            raise MirrorVerificationError(MirrorErrorCode.CONFIGURATION)
        candidates = [{"source": value.source, "commitment_hex": _fixed(value.commitment, 32).hex()} for value in policy.candidates]
        if len({value["source"] for value in candidates}) != len(candidates):
            raise MirrorVerificationError(MirrorErrorCode.CONFIGURATION)
        wire_policy: dict[str, object]
        if policy.kind == "exact":
            if len(candidates) != 1:
                raise MirrorVerificationError(MirrorErrorCode.CONFIGURATION)
            wire_policy = {"kind": "exact", "candidate": candidates[0]}
        elif policy.kind == "ordered-preference":
            wire_policy = {"kind": policy.kind, "candidates": candidates}
        elif policy.kind == "agreement" and 0 < policy.minimum <= len(candidates):
            wire_policy = {"kind": policy.kind, "candidates": candidates, "minimum": policy.minimum}
        else:
            raise MirrorVerificationError(MirrorErrorCode.CONFIGURATION)
        request = json.dumps({"batch_number": str(batch_number), "evidence": evidence, "policy": wire_policy}, separators=(",", ":")).encode()
        if len(request) > 40 * 1024 * 1024:
            raise MirrorVerificationError(MirrorErrorCode.BOUNDS)
        try:
            result = subprocess.run([self._executable, self._configuration], input=request, stdout=subprocess.PIPE,
                                    stderr=subprocess.DEVNULL, timeout=self._timeout, check=False)
        except (OSError, subprocess.TimeoutExpired) as error:
            raise MirrorVerificationError(MirrorErrorCode.UNAVAILABLE) from error
        if len(result.stdout) > 1_048_576:
            raise MirrorVerificationError(MirrorErrorCode.BOUNDS)
        try:
            response = json.loads(result.stdout)
            if not isinstance(response, dict):
                raise ValueError("response is not an object")
            if not response.get("ok"):
                raise MirrorVerificationError(str(response.get("error", "unavailable")))
            value = response["verification"]
            if not isinstance(value, dict) or value.get("provenance") not in ("Canonical", "Reorged") or value.get("checkpointLevel") != "unavailable":
                raise ValueError("invalid verification response")
            return MirrorVerification(_text(value["level"], 64), _decimal(value["batchNumber"]), _digest(value["headerDigest"]),
                _digest(value["evidenceDigest"]), _text(value["sourceId"], 64), _text(value["target"], 2048),
                _text(value["canonicalPosition"], 2048), value["provenance"], _optional_decimal(value.get("latestBatch")),
                _text(value["batchLag"], 64), _small(value["failoverCount"], 0, 8), _small(value["agreeingSources"], 1, 8), "unavailable")
        except (AttributeError, KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
            raise MirrorVerificationError(MirrorErrorCode.MALFORMED) from error

def _fixed(value: bytes, length: int) -> bytes:
    if len(value) != length:
        raise MirrorVerificationError(MirrorErrorCode.BOUNDS)
    return value

def _decimal(value: object) -> int:
    if not isinstance(value, str) or not value or value == "0" or value.startswith("0") or not value.isascii() or not value.isdecimal():
        raise ValueError("non-canonical unsigned integer")
    result = int(value)
    if result > _MAX_U64:
        raise ValueError("unsigned integer exceeds u64")
    return result

def _optional_decimal(value: object) -> int | None:
    return None if value is None else _decimal(value)

def _digest(value: object) -> bytes:
    if not isinstance(value, str):
        raise ValueError("digest is not text")
    result = bytes.fromhex(value)
    if len(result) != 32 or value != result.hex():
        raise ValueError("digest is not canonical")
    return result

def _text(value: object, maximum: int) -> str:
    if not isinstance(value, str) or not value or len(value.encode()) > maximum:
        raise ValueError("text field is malformed")
    return value

def _small(value: object, minimum: int, maximum: int) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < minimum or value > maximum:
        raise ValueError("bounded integer is malformed")
    return value

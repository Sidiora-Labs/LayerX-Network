from __future__ import annotations

from dataclasses import dataclass
from typing import Literal, Mapping, NoReturn

from .production import PlatformSdkError, SdkErrorCode
from .verifier import (
    AuthorizedReceiptBatch,
    CheckpointVerificationInput,
    LocalSignatureVerifier,
    MerkleProof,
    ReceiptVerification,
    SequencerAuthorization,
    verify_batch_inclusion,
    verify_checkpoint,
    verify_receipt,
)

PaymentCommitment = Literal["executed", "batched", "finalised"]


@dataclass(frozen=True)
class PaymentCheckpointEvidence:
    verification: CheckpointVerificationInput
    required_guarantors: int


@dataclass(frozen=True)
class PaymentCommitmentEvidence:
    network_id: int
    canonical_header: bytes
    header_signature: bytes
    authorization: SequencerAuthorization
    proof: MerkleProof
    checkpoint: PaymentCheckpointEvidence | None = None


def _failure() -> NoReturn:
    raise PlatformSdkError(SdkErrorCode.VERIFICATION_FAILURE, "never")


def payment_commitment(extra: object = None) -> PaymentCommitment:
    if not isinstance(extra, Mapping) or "layerx" not in extra:
        return "executed"
    layerx = extra["layerx"]
    if not isinstance(layerx, Mapping):
        _failure()
    commitment = layerx.get("commitment")
    if commitment not in ("executed", "batched", "finalised"):
        _failure()
    return commitment


def verify_payment_commitment_evidence(
    verified: ReceiptVerification,
    sequencer_public_key: bytes,
    commitment: PaymentCommitment,
    evidence: PaymentCommitmentEvidence,
    signatures: LocalSignatureVerifier,
) -> None:
    if commitment == "executed":
        return
    if commitment not in ("batched", "finalised"):
        _failure()
    version = verified.receipt.protocol_version
    if (
        version not in (2, 3)
        or type(evidence.network_id) is not int
        or not 0 < evidence.network_id <= 0xFFFF_FFFF
        or evidence.authorization.public_key != sequencer_public_key
    ):
        _failure()
    inclusion = verify_batch_inclusion(
        "receipt",
        verified.canonical_bytes,
        evidence.proof,
        evidence.canonical_header,
        evidence.header_signature,
        evidence.authorization,
        signatures,
        protocol_version=version,
    )
    if (
        inclusion.header.network_id != evidence.network_id
        or not inclusion.header.first_sequence
        <= verified.receipt.global_sequence
        <= inclusion.header.last_sequence
    ):
        _failure()
    if commitment == "batched":
        return
    checkpoint = evidence.checkpoint
    if (
        checkpoint is None
        or type(checkpoint.required_guarantors) is not int
        or checkpoint.required_guarantors <= 0
        or checkpoint.verification.certificate.threshold
        != checkpoint.required_guarantors
        or checkpoint.verification.certificate.canonical_header
        != evidence.canonical_header
    ):
        _failure()
    verify_checkpoint(checkpoint.verification, signatures, protocol_version=version)


def verify_payment_receipt(
    canonical_receipt: bytes,
    authorized: AuthorizedReceiptBatch,
    signatures: LocalSignatureVerifier,
    *,
    amount: str,
    asset: str,
    pay_to: str,
    commitment: PaymentCommitment = "executed",
    evidence: PaymentCommitmentEvidence | None = None,
) -> ReceiptVerification:
    if (
        not isinstance(amount, str)
        or not 0 < len(amount) <= 39
        or any(c not in "0123456789" for c in amount)
        or amount[0] == "0"
        or not 0 < int(amount) < 1 << 128
    ):
        _failure()
    for identifier in (asset, pay_to):
        if (
            not isinstance(identifier, str)
            or len(identifier) != 64
            or any(c not in "0123456789abcdef" for c in identifier)
        ):
            _failure()
    verified = verify_receipt(canonical_receipt, authorized, signatures)
    if (
        verified.receipt.amount != int(amount)
        or verified.receipt.asset.hex() != asset
        or verified.receipt.to_account.hex() != pay_to
    ):
        _failure()
    if commitment not in ("executed", "batched", "finalised"):
        _failure()
    if commitment != "executed":
        if evidence is None:
            _failure()
        verify_payment_commitment_evidence(
            verified, authorized.sequencer_public_key, commitment, evidence, signatures
        )
    return verified

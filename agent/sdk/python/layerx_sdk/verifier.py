from __future__ import annotations

from dataclasses import dataclass
from hashlib import sha256
from typing import Literal, Protocol, cast

from .production import PlatformSdkError, SdkErrorCode

_MERKLE_LEAF_DOMAIN = b"LXP/v1/merkle-leaf\0"
_MERKLE_INTERNAL_DOMAIN = b"LXP/v1/merkle-internal\0"
_BATCH_HEADER_DOMAIN = b"LXP/v1/batch-header\0"
_RECEIPT_DOMAIN = b"LXP/v1/receipt\0"
_CHECKPOINT_DOMAIN = b"LXP/v1/checkpoint-certificate\0"
_GUARANTOR_ATTESTATION_DOMAIN = b"LXP/v1/guarantor-attestation\0"
_BATCH_HEADER_BYTES = 354
_MAX_MESSAGE_BYTES = 1_048_576
_MAX_EFFECTS = 512
_MAX_EFFECT_BODY = 256
_MAX_U128 = 0xFFFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF
_ALL_AVAILABILITY_CLASSES = 0x1F
_PROGRAM_OUTCOME_V1 = 0x5052_4731
_PROGRAM_OUTCOME_V2 = 0x5052_4732
_PROGRAM_OUTCOME_V3 = 0x5052_4733


def _failure() -> None:
    raise PlatformSdkError(SdkErrorCode.VERIFICATION_FAILURE, "never")


def _exact(value: bytes, length: int) -> bytes:
    if len(value) != length:
        _failure()
    return value


def _equal(left: bytes, right: bytes) -> bool:
    if len(left) != len(right):
        return False
    difference = 0
    for left_byte, right_byte in zip(left, right, strict=True):
        difference |= left_byte ^ right_byte
    return difference == 0


def _digest(domain: bytes, *values: bytes) -> bytes:
    digest = sha256()
    digest.update(domain)
    for value in values:
        digest.update(value)
    return digest.digest()


@dataclass(frozen=True)
class MerkleProof:
    leaf_index: int
    leaf_count: int
    siblings: tuple[bytes, ...]


def _proof_depth(leaf_count: int) -> int:
    count = leaf_count
    depth = 0
    while count > 1:
        count = (count + 1) // 2
        depth += 1
    return depth


def verify_merkle_inclusion(canonical_leaf: bytes, proof: MerkleProof, expected_root: bytes) -> None:
    if (
        proof.leaf_count <= 0
        or proof.leaf_count > 0xFFFF_FFFF
        or proof.leaf_index < 0
        or proof.leaf_index >= proof.leaf_count
        or len(proof.siblings) > 32
        or len(proof.siblings) != _proof_depth(proof.leaf_count)
    ):
        _failure()
    current = _digest(_MERKLE_LEAF_DOMAIN, canonical_leaf)
    index = proof.leaf_index
    level_count = proof.leaf_count
    for sibling_value in proof.siblings:
        sibling = _exact(sibling_value, 32)
        if (index ^ 1) >= level_count and not _equal(sibling, current):
            _failure()
        current = (
            _digest(_MERKLE_INTERNAL_DOMAIN, current, sibling)
            if index % 2 == 0
            else _digest(_MERKLE_INTERNAL_DOMAIN, sibling, current)
        )
        index //= 2
        level_count = (level_count + 1) // 2
    if not _equal(current, _exact(expected_root, 32)):
        _failure()


class _Decoder:
    def __init__(self, value: bytes) -> None:
        self._value = value
        self._offset = 0

    def fixed(self, length: int) -> bytes:
        end = self._offset + length
        if length < 0 or end > len(self._value):
            _failure()
        result = self._value[self._offset:end]
        self._offset = end
        return result

    def integer(self, length: int) -> int:
        return int.from_bytes(self.fixed(length), "big", signed=False)

    def u8(self) -> int:
        return self.integer(1)

    def u16(self) -> int:
        return self.integer(2)

    def u32(self) -> int:
        return self.integer(4)

    def u64(self) -> int:
        return self.integer(8)

    def u128(self) -> int:
        return self.integer(16)

    def i32(self) -> int:
        return int.from_bytes(self.fixed(4), "big", signed=True)

    def position(self) -> int:
        return self._offset

    def remaining(self) -> int:
        return len(self._value) - self._offset

    def bounded(self, length: int) -> bytes:
        if self.u32() != length:
            _failure()
        return self.fixed(length)

    def bounded_at_most(self, maximum: int) -> bytes:
        length = self.u32()
        if length > maximum:
            _failure()
        return self.fixed(length)

    def finish(self) -> None:
        if self._offset != len(self._value):
            _failure()


@dataclass(frozen=True)
class BatchHeader:
    protocol_version: int
    network_id: int
    epoch: int
    batch_number: int
    first_sequence: int
    last_sequence: int
    previous_state_root: bytes
    resulting_state_root: bytes
    activity_merkle_root: bytes
    receipt_merkle_root: bytes
    event_merkle_root: bytes
    data_availability_root: bytes
    oracle_root: bytes
    timestamp_ms: int
    sequencer_id: bytes


def _field(decoder: _Decoder, expected: int) -> None:
    if decoder.u8() != expected:
        _failure()


def decode_batch_header(canonical_header: bytes) -> BatchHeader:
    if len(canonical_header) != _BATCH_HEADER_BYTES:
        _failure()
    decoder = _Decoder(canonical_header)
    if decoder.u16() != 1 or decoder.u16() != 0x1701 or decoder.u8() != 15:
        _failure()
    _field(decoder, 1)
    protocol_version = decoder.u16()
    _field(decoder, 2)
    network_id = decoder.u32()
    _field(decoder, 3)
    epoch = decoder.u64()
    _field(decoder, 4)
    batch_number = decoder.u64()
    _field(decoder, 5)
    first_sequence = decoder.u64()
    _field(decoder, 6)
    last_sequence = decoder.u64()
    _field(decoder, 7)
    previous_state_root = decoder.bounded(32)
    _field(decoder, 8)
    resulting_state_root = decoder.bounded(32)
    _field(decoder, 9)
    activity_merkle_root = decoder.bounded(32)
    _field(decoder, 10)
    receipt_merkle_root = decoder.bounded(32)
    _field(decoder, 11)
    event_merkle_root = decoder.bounded(32)
    _field(decoder, 12)
    data_availability_root = decoder.bounded(32)
    _field(decoder, 13)
    oracle_root = decoder.bounded(32)
    _field(decoder, 14)
    timestamp_ms = decoder.u64()
    _field(decoder, 15)
    sequencer_id = decoder.bounded(32)
    decoder.finish()
    return BatchHeader(
        protocol_version=protocol_version,
        network_id=network_id,
        epoch=epoch,
        batch_number=batch_number,
        first_sequence=first_sequence,
        last_sequence=last_sequence,
        previous_state_root=previous_state_root,
        resulting_state_root=resulting_state_root,
        activity_merkle_root=activity_merkle_root,
        receipt_merkle_root=receipt_merkle_root,
        event_merkle_root=event_merkle_root,
        data_availability_root=data_availability_root,
        oracle_root=oracle_root,
        timestamp_ms=timestamp_ms,
        sequencer_id=sequencer_id,
    )


class LocalSignatureVerifier(Protocol):
    def verify_ed25519(self, public_key: bytes, signature: bytes, digest: bytes) -> bool: ...
    def verify_recoverable_secp256k1(
        self,
        public_key: bytes,
        signature: bytes,
        signature_v: int,
        signer: bytes,
        digest: bytes,
    ) -> bool: ...


@dataclass(frozen=True)
class ReceiptEffect:
    module_id: int
    ordinal: int
    event_type: int
    kind: Literal[1, 2, 3]
    monetary: bool
    transfer_set_root: bytes
    body: bytes


@dataclass(frozen=True)
class ProgramReceiptOutcome:
    encoding_version: Literal[1, 2, 3]
    terminal_kind: Literal[1, 2, 3]
    result_code: int
    runtime_version: int
    abi_version: int
    fee_schedule_version: int
    metering_schedule_version: int
    cpu_fuel: int
    memory_bytes: int
    storage_read_bytes: int
    storage_write_bytes: int
    output_values: int
    output_bytes: int
    occupancy_byte_batches: int
    occupancy_fee_units: int
    fee_schedule_prices: tuple[int, int, int, int, int, int, int]
    occupancy_asset_id: bytes
    occupancy_evidence_digest: bytes
    occupancy_transfer_root: bytes
    fee_units: int
    call_graph_root: bytes
    terminal_payload_root: bytes
    transfer_root: bytes


@dataclass(frozen=True)
class ProtocolReceipt:
    protocol_version: int
    activity_id: bytes
    global_sequence: int
    previous_state_root: bytes
    resulting_state_root: bytes
    activity_root: bytes
    result_code: int
    effects: tuple[ReceiptEffect, ...]
    fee_charged: int
    batch_id: bytes
    module_id: int
    module_version: int
    parameter_version: int
    operation: int
    asset: bytes
    amount: int
    from_account: bytes
    from_balance_before: int
    from_balance_after: int
    from_sequence: int
    to_account: bytes
    to_balance_before: int
    to_balance_after: int
    transfer_set_root: bytes
    authorization_hash: bytes
    context_hash: bytes
    timestamp: int
    program_outcome: ProgramReceiptOutcome | None
    sequencer_signature: bytes


@dataclass(frozen=True)
class AuthorizedReceiptBatch:
    batch_id: bytes
    asset: bytes
    previous_state_root: bytes
    resulting_state_root: bytes
    sequencer_public_key: bytes


@dataclass(frozen=True)
class ReceiptVerification:
    level: Literal["sequencer-signed"]
    receipt: ProtocolReceipt
    canonical_bytes: bytes
    receipt_digest: bytes


@dataclass(frozen=True)
class SequencerAuthorization:
    sequencer_id: bytes
    public_key: bytes
    first_batch_number: int
    last_batch_number: int


InclusionKind = Literal["activity", "receipt", "event", "state"]


@dataclass(frozen=True)
class InclusionVerification:
    level: Literal["batch-included", "state-proven"]
    header: BatchHeader
    header_digest: bytes
    root: bytes


def verify_batch_inclusion(
    kind: InclusionKind,
    canonical_leaf: bytes,
    proof: MerkleProof,
    canonical_header: bytes,
    header_signature: bytes,
    authorization: SequencerAuthorization,
    signatures: LocalSignatureVerifier,
) -> InclusionVerification:
    header = decode_batch_header(canonical_header)
    if (
        header.batch_number < authorization.first_batch_number
        or header.batch_number > authorization.last_batch_number
        or not _equal(header.sequencer_id, _exact(authorization.sequencer_id, 32))
    ):
        _failure()
    digest = _digest(_BATCH_HEADER_DOMAIN, canonical_header)
    if not signatures.verify_ed25519(
        _exact(authorization.public_key, 32),
        _exact(header_signature, 64),
        digest,
    ):
        _failure()
    roots = {
        "activity": header.activity_merkle_root,
        "receipt": header.receipt_merkle_root,
        "event": header.event_merkle_root,
        "state": header.resulting_state_root,
    }
    root = roots[kind]
    verify_merkle_inclusion(canonical_leaf, proof, root)
    return InclusionVerification(
        level="state-proven" if kind == "state" else "batch-included",
        header=header,
        header_digest=digest,
        root=root,
    )


@dataclass(frozen=True)
class CheckpointAttestation:
    protocol_version: int
    network_id: int
    paxeer_chain_id: int
    settlement_contract: bytes
    epoch: int
    checkpoint_id: bytes
    checkpoint_hash: bytes
    guarantor_id: bytes
    batch_number: int
    data_availability_root: bytes
    replayed: bool
    data_possessed: bool
    availability_class_mask: int
    attested_at_ms: int
    signer: bytes
    signature: bytes
    signature_v: int


@dataclass(frozen=True)
class GuarantorKey:
    guarantor_id: bytes
    public_key: bytes
    bonded: bool


@dataclass(frozen=True)
class CheckpointCertificate:
    canonical_header: bytes
    validity_proof: bytes
    attestations: tuple[CheckpointAttestation, ...]
    threshold: int
    settlement_reference: bytes | None = None


@dataclass(frozen=True)
class CheckpointVerificationInput:
    certificate: CheckpointCertificate
    bonded_set: tuple[GuarantorKey, ...]
    registered_checkpoint_id: bytes
    expected_paxeer_chain_id: int
    expected_settlement_contract: bytes
    registered_settlement_reference: bytes | None
    availability_obtained: bool


@dataclass(frozen=True)
class CheckpointVerification:
    level: Literal["checkpoint-finalised", "settlement-anchored"]
    checkpoint_id: bytes
    achieved: int
    required: int
    header: BatchHeader


def _u32(value: int) -> bytes:
    if value < 0 or value > 0xFFFF_FFFF:
        _failure()
    return value.to_bytes(4, "big", signed=False)


def _u16(value: int) -> bytes:
    if value < 0 or value > 0xFFFF:
        _failure()
    return value.to_bytes(2, "big", signed=False)


def _u64(value: int) -> bytes:
    if value < 0 or value > 0xFFFF_FFFF_FFFF_FFFF:
        _failure()
    return value.to_bytes(8, "big", signed=False)


def _attestation_message(attestation: CheckpointAttestation) -> bytes:
    return b"".join((
        _u16(attestation.protocol_version),
        _u32(attestation.network_id),
        _u64(attestation.paxeer_chain_id),
        _exact(attestation.settlement_contract, 20),
        _u64(attestation.epoch),
        _exact(attestation.checkpoint_id, 32),
        _exact(attestation.checkpoint_hash, 32),
        _exact(attestation.guarantor_id, 32),
        _u64(attestation.batch_number),
        _exact(attestation.data_availability_root, 32),
        bytes((
            1 if attestation.replayed else 0,
            1 if attestation.data_possessed else 0,
            attestation.availability_class_mask,
        )),
        _u64(attestation.attested_at_ms),
    ))


def verify_checkpoint(
    verification: CheckpointVerificationInput,
    signatures: LocalSignatureVerifier,
) -> CheckpointVerification:
    certificate = verification.certificate
    if not verification.availability_obtained or len(certificate.validity_proof) > 0xFFFF_FFFF:
        _failure()
    header = decode_batch_header(certificate.canonical_header)
    checkpoint_id = _digest(
        _CHECKPOINT_DOMAIN,
        certificate.canonical_header,
        _u32(len(certificate.validity_proof)),
        certificate.validity_proof,
    )
    if not _equal(checkpoint_id, _exact(verification.registered_checkpoint_id, 32)):
        _failure()
    expected_settlement_contract = _exact(verification.expected_settlement_contract, 20)
    if (
        certificate.threshold <= 0
        or verification.expected_paxeer_chain_id <= 0
        or not any(expected_settlement_contract)
    ):
        _failure()
    seen: set[bytes] = set()
    achieved = 0
    for attestation in certificate.attestations:
        guarantor_id = _exact(attestation.guarantor_id, 32)
        if (
            guarantor_id in seen
            or attestation.protocol_version != header.protocol_version
            or attestation.network_id != header.network_id
            or attestation.epoch != header.epoch
            or attestation.paxeer_chain_id != verification.expected_paxeer_chain_id
            or not _equal(_exact(attestation.settlement_contract, 20), expected_settlement_contract)
            or not _equal(attestation.checkpoint_id, checkpoint_id)
            or not _equal(attestation.checkpoint_hash, checkpoint_id)
            or attestation.batch_number != header.batch_number
            or not _equal(attestation.data_availability_root, header.data_availability_root)
            or not attestation.replayed
            or not attestation.data_possessed
            or attestation.availability_class_mask != _ALL_AVAILABILITY_CLASSES
            or attestation.attested_at_ms <= 0
            or not any(_exact(attestation.signer, 20))
            or attestation.signature_v not in (27, 28)
        ):
            _failure()
        seen.add(guarantor_id)
        member = next(
            (
                candidate
                for candidate in verification.bonded_set
                if candidate.bonded and _equal(candidate.guarantor_id, guarantor_id)
            ),
            None,
        )
        if member is None:
            _failure()
        digest = _digest(_GUARANTOR_ATTESTATION_DOMAIN, _attestation_message(attestation))
        if not signatures.verify_recoverable_secp256k1(
            _exact(member.public_key, 33),
            _exact(attestation.signature, 64),
            attestation.signature_v,
            _exact(attestation.signer, 20),
            digest,
        ):
            _failure()
        achieved += 1
    if achieved < certificate.threshold:
        _failure()
    settlement = certificate.settlement_reference
    if settlement is not None and (
        not settlement
        or verification.registered_settlement_reference is None
        or not _equal(settlement, verification.registered_settlement_reference)
    ):
        _failure()
    return CheckpointVerification(
        level="checkpoint-finalised" if settlement is None else "settlement-anchored",
        checkpoint_id=checkpoint_id,
        achieved=achieved,
        required=certificate.threshold,
        header=header,
    )


def _all_zero(value: bytes) -> bool:
    aggregate = 0
    for byte in value:
        aggregate |= byte
    return aggregate == 0


def _decode_program_receipt_outcome_from(
    decoder: _Decoder, protocol_version: int
) -> ProgramReceiptOutcome:
    tags = {
        _PROGRAM_OUTCOME_V1: 1,
        _PROGRAM_OUTCOME_V2: 2,
        _PROGRAM_OUTCOME_V3: 3,
    }
    encoding_value = tags.get(decoder.u32())
    if encoding_value is None:
        _failure()
    encoding_version = cast(Literal[1, 2, 3], encoding_value)
    terminal_value = decoder.u8()
    if terminal_value not in (1, 2, 3):
        _failure()
    terminal_kind = cast(Literal[1, 2, 3], terminal_value)
    result_code = decoder.i32()
    runtime_version = decoder.u16()
    abi_version = decoder.u16()
    fee_schedule_version = decoder.u32()
    metering_schedule_version = decoder.u32() if encoding_version == 3 else 1
    cpu_fuel = decoder.u64()
    memory_bytes = decoder.u64()
    storage_read_bytes = decoder.u64()
    storage_write_bytes = decoder.u64()
    output_values = decoder.u32()
    output_bytes = decoder.u64()
    occupancy_byte_batches = decoder.u128() if encoding_version >= 2 else 0
    occupancy_fee_units = decoder.u128() if encoding_version >= 2 else 0
    fee_schedule_prices = cast(
        tuple[int, int, int, int, int, int, int],
        tuple(decoder.u64() for _ in range(7)) if encoding_version >= 2 else (0,) * 7,
    )
    occupancy_asset_id = decoder.bounded(32) if encoding_version >= 2 else bytes(32)
    occupancy_evidence_digest = decoder.bounded(32) if encoding_version >= 2 else bytes(32)
    occupancy_transfer_root = decoder.bounded(32) if encoding_version >= 2 else bytes(32)
    fee_units = decoder.u128()
    call_graph_root = decoder.bounded(32)
    terminal_payload_root = decoder.bounded(32)
    transfer_root = decoder.bounded(32)
    occupancy_zero = (
        occupancy_byte_batches == 0
        and occupancy_fee_units == 0
        and _all_zero(occupancy_asset_id)
        and _all_zero(occupancy_evidence_digest)
        and _all_zero(occupancy_transfer_root)
    )
    if (
        runtime_version == 0
        or abi_version == 0
        or fee_schedule_version == 0
        or metering_schedule_version != 1
        or _all_zero(terminal_payload_root)
        or (terminal_kind == 1 and result_code != 0)
        or (terminal_kind != 1 and (result_code == 0 or result_code <= -1000))
        or (terminal_kind != 1 and not _all_zero(transfer_root))
        or not (
            (protocol_version == 1 and encoding_version in (1, 3))
            or (protocol_version == 2 and encoding_version in (2, 3))
        )
        or (encoding_version == 1 and not occupancy_zero)
        or (encoding_version >= 2 and terminal_kind != 1 and not occupancy_zero)
        or (
            encoding_version == 2
            and terminal_kind == 1
            and (_all_zero(occupancy_asset_id) or _all_zero(occupancy_evidence_digest))
        )
        or (
            encoding_version == 3
            and _all_zero(occupancy_asset_id) != _all_zero(occupancy_evidence_digest)
        )
        or (protocol_version == 1 and encoding_version == 3 and not occupancy_zero)
        or (
            protocol_version == 2
            and encoding_version == 3
            and terminal_kind == 1
            and (_all_zero(occupancy_asset_id) or _all_zero(occupancy_evidence_digest))
        )
    ):
        _failure()
    return ProgramReceiptOutcome(
        encoding_version=encoding_version,
        terminal_kind=terminal_kind,
        result_code=result_code,
        runtime_version=runtime_version,
        abi_version=abi_version,
        fee_schedule_version=fee_schedule_version,
        metering_schedule_version=metering_schedule_version,
        cpu_fuel=cpu_fuel,
        memory_bytes=memory_bytes,
        storage_read_bytes=storage_read_bytes,
        storage_write_bytes=storage_write_bytes,
        output_values=output_values,
        output_bytes=output_bytes,
        occupancy_byte_batches=occupancy_byte_batches,
        occupancy_fee_units=occupancy_fee_units,
        fee_schedule_prices=fee_schedule_prices,
        occupancy_asset_id=occupancy_asset_id,
        occupancy_evidence_digest=occupancy_evidence_digest,
        occupancy_transfer_root=occupancy_transfer_root,
        fee_units=fee_units,
        call_graph_root=call_graph_root,
        terminal_payload_root=terminal_payload_root,
        transfer_root=transfer_root,
    )


def decode_program_receipt_outcome(
    canonical_outcome: bytes, protocol_version: int
) -> ProgramReceiptOutcome:
    if not canonical_outcome or len(canonical_outcome) > _MAX_MESSAGE_BYTES:
        _failure()
    decoder = _Decoder(bytes(canonical_outcome))
    outcome = _decode_program_receipt_outcome_from(decoder, protocol_version)
    decoder.finish()
    return outcome


def _decode_protocol_receipt(canonical_receipt: bytes) -> tuple[ProtocolReceipt, bytes]:
    if not canonical_receipt or len(canonical_receipt) > _MAX_MESSAGE_BYTES:
        _failure()
    decoder = _Decoder(canonical_receipt)
    envelope_version = decoder.u16()
    if envelope_version not in (1, 2) or decoder.u16() != 0x5201:
        _failure()
    protocol_version = decoder.u16()
    if protocol_version != envelope_version:
        _failure()
    activity_id = decoder.bounded(32)
    global_sequence = decoder.u64()
    previous_state_root = decoder.bounded(32)
    resulting_state_root = decoder.bounded(32)
    activity_root = decoder.bounded(32)
    result_code = decoder.i32()
    effect_count = decoder.u32()
    if effect_count > _MAX_EFFECTS:
        _failure()
    effects: list[ReceiptEffect] = []
    for _ in range(effect_count):
        module_id = decoder.u16()
        ordinal = decoder.u16()
        event_type = decoder.u16()
        kind_value = decoder.u8()
        if kind_value < 1 or kind_value > 3:
            _failure()
        monetary_value = decoder.u8()
        if monetary_value > 1 or (monetary_value == 1 and kind_value != 2):
            _failure()
        effects.append(ReceiptEffect(
            module_id=module_id,
            ordinal=ordinal,
            event_type=event_type,
            kind=cast(Literal[1, 2, 3], kind_value),
            monetary=monetary_value == 1,
            transfer_set_root=decoder.bounded(32),
            body=decoder.bounded_at_most(_MAX_EFFECT_BODY),
        ))
    fee_charged = decoder.u128()
    batch_id = decoder.bounded(32)
    module_id = decoder.u16()
    module_version = decoder.u32()
    parameter_version = decoder.u32()
    operation = decoder.u8()
    asset = decoder.bounded(32)
    amount = decoder.u128()
    from_account = decoder.bounded(32)
    from_balance_before = decoder.u128()
    from_balance_after = decoder.u128()
    from_sequence = decoder.u64()
    to_account = decoder.bounded(32)
    to_balance_before = decoder.u128()
    to_balance_after = decoder.u128()
    transfer_set_root = decoder.bounded(32)
    authorization_hash = decoder.bounded(32)
    context_hash = decoder.bounded(32)
    timestamp = decoder.u64()
    program_outcome = (
        _decode_program_receipt_outcome_from(decoder, protocol_version)
        if decoder.remaining() > 69
        else None
    )
    if program_outcome is not None and (
        module_id != 9
        or program_outcome.result_code != result_code
        or (
            program_outcome.terminal_kind == 1
            and not _equal(program_outcome.transfer_root, transfer_set_root)
        )
        or (program_outcome.terminal_kind != 1 and not _all_zero(transfer_set_root))
    ):
        _failure()
    signature_flag_offset = decoder.position()
    if decoder.u8() != 1:
        _failure()
    sequencer_signature = decoder.bounded(64)
    decoder.finish()
    return (
        ProtocolReceipt(
            protocol_version=protocol_version,
            activity_id=activity_id,
            global_sequence=global_sequence,
            previous_state_root=previous_state_root,
            resulting_state_root=resulting_state_root,
            activity_root=activity_root,
            result_code=result_code,
            effects=tuple(effects),
            fee_charged=fee_charged,
            batch_id=batch_id,
            module_id=module_id,
            module_version=module_version,
            parameter_version=parameter_version,
            operation=operation,
            asset=asset,
            amount=amount,
            from_account=from_account,
            from_balance_before=from_balance_before,
            from_balance_after=from_balance_after,
            from_sequence=from_sequence,
            to_account=to_account,
            to_balance_before=to_balance_before,
            to_balance_after=to_balance_after,
            transfer_set_root=transfer_set_root,
            authorization_hash=authorization_hash,
            context_hash=context_hash,
            timestamp=timestamp,
            program_outcome=program_outcome,
            sequencer_signature=sequencer_signature,
        ),
        canonical_receipt[:signature_flag_offset] + b"\0",
    )


def verify_receipt_outcome(
    canonical_receipt: bytes,
    authorized: AuthorizedReceiptBatch,
    signatures: LocalSignatureVerifier,
) -> ReceiptVerification:
    receipt, unsigned_receipt = _decode_protocol_receipt(canonical_receipt)
    if (
        receipt.operation == 0
        or _all_zero(receipt.activity_id)
        or _all_zero(receipt.asset)
        or not _equal(receipt.batch_id, _exact(authorized.batch_id, 32))
        or not _equal(receipt.asset, _exact(authorized.asset, 32))
        or not _equal(receipt.previous_state_root, _exact(authorized.previous_state_root, 32))
        or not _equal(receipt.resulting_state_root, _exact(authorized.resulting_state_root, 32))
    ):
        _failure()
    if receipt.result_code == 0 and (
        receipt.from_balance_before < receipt.amount
        or receipt.from_balance_before - receipt.amount != receipt.from_balance_after
        or receipt.to_balance_before + receipt.amount > _MAX_U128
        or receipt.to_balance_before + receipt.amount != receipt.to_balance_after
    ):
        _failure()
    receipt_digest = _digest(_RECEIPT_DOMAIN, unsigned_receipt)
    if not signatures.verify_ed25519(
        _exact(authorized.sequencer_public_key, 32),
        receipt.sequencer_signature,
        receipt_digest,
    ):
        _failure()
    return ReceiptVerification(
        level="sequencer-signed",
        receipt=receipt,
        canonical_bytes=bytes(canonical_receipt),
        receipt_digest=receipt_digest,
    )


def verify_receipt(
    canonical_receipt: bytes,
    authorized: AuthorizedReceiptBatch,
    signatures: LocalSignatureVerifier,
) -> ReceiptVerification:
    verified = verify_receipt_outcome(canonical_receipt, authorized, signatures)
    if verified.receipt.result_code != 0:
        _failure()
    return verified

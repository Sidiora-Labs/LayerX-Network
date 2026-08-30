from dataclasses import dataclass
from typing import Literal, Protocol
from .generated.receipt import ReceiptFailureCode
from .production import PlatformSdkError

class ReceiptVerificationError(PlatformSdkError):
    check: ReceiptFailureCode

@dataclass(frozen=True)
class MerkleProof:
    leaf_index: int
    leaf_count: int
    siblings: tuple[bytes, ...]

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

class LocalSignatureVerifier(Protocol):
    def verify_ed25519(self, public_key: bytes, signature: bytes, digest: bytes) -> bool: ...
    def verify_recoverable_secp256k1(self, public_key: bytes, signature: bytes, signature_v: int, signer: bytes, digest: bytes) -> bool: ...

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
    settlement_reference: bytes | None = ...

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

def verify_merkle_inclusion(canonical_leaf: bytes, proof: MerkleProof, expected_root: bytes) -> None: ...
def decode_batch_header(canonical_header: bytes) -> BatchHeader: ...
def decode_program_receipt_outcome(canonical_outcome: bytes, protocol_version: int) -> ProgramReceiptOutcome: ...
def verify_batch_inclusion(kind: InclusionKind, canonical_leaf: bytes, proof: MerkleProof, canonical_header: bytes, header_signature: bytes, authorization: SequencerAuthorization, signatures: LocalSignatureVerifier) -> InclusionVerification: ...
def verify_checkpoint(verification: CheckpointVerificationInput, signatures: LocalSignatureVerifier) -> CheckpointVerification: ...
def verify_receipt_outcome(canonical_receipt: bytes, authorized: AuthorizedReceiptBatch, signatures: LocalSignatureVerifier) -> ReceiptVerification: ...
def verify_receipt(canonical_receipt: bytes, authorized: AuthorizedReceiptBatch, signatures: LocalSignatureVerifier) -> ReceiptVerification: ...

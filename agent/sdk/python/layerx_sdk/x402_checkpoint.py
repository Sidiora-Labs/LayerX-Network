from dataclasses import dataclass, replace

from .verifier import CheckpointAttestation, CheckpointCertificate, decode_batch_header
from .x402 import PaymentCheckpointEvidence
from .x402_rpc import rpc_hex


@dataclass(frozen=True)
class RpcCheckpointAuthority:
    canonical_context: bytes
    bonded_set: tuple
    registered_checkpoint_id: bytes
    expected_paxeer_chain_id: int
    expected_settlement_contract: bytes
    registered_settlement_reference: bytes
    availability_obtained: bool
    required_guarantors: int


class _Reader:
    def __init__(self, value):
        self.value, self.offset = value, 0

    def take(self, size):
        if size < 0 or self.offset + size > len(self.value):
            raise ValueError("invalid-checkpoint-wire")
        result = self.value[self.offset : self.offset + size]
        self.offset += size
        return result

    def integer(self, size):
        return int.from_bytes(self.take(size), "big")

    def boolean(self):
        n = self.integer(1)
        if n > 1:
            raise ValueError("invalid-checkpoint-boolean")
        return n == 1

    def sized(self, width, maximum):
        size = self.integer(width)
        if size > maximum:
            raise ValueError("checkpoint-bound")
        return self.take(size)


def rpc_checkpoint_evidence(result, batch, authority):
    from .verifier import CheckpointVerificationInput

    wire = result["checkpoint_evidence"]
    if (
        type(authority.required_guarantors) is not int
        or not 0 < authority.required_guarantors <= 32
        or not 0 < len(authority.canonical_context) <= 131072
        or rpc_hex(wire["context"]) != authority.canonical_context
        or rpc_hex(wire["checkpoint_id"], 32) != authority.registered_checkpoint_id
    ):
        raise ValueError("checkpoint-authority-mismatch")
    canonical_header = rpc_hex(wire["canonical_header"])
    header = decode_batch_header(canonical_header)
    if (
        canonical_header != batch.canonical_header
        or header.network_id != batch.network_id
    ):
        raise ValueError("checkpoint-header-mismatch")
    reader = _Reader(rpc_hex(wire["checkpoint"]))
    if reader.integer(2) != 1 or reader.sized(4, 4096) != canonical_header:
        raise ValueError("checkpoint-header-mismatch")
    validity = reader.sized(4, 1048576)
    count = reader.integer(1)
    if not 0 < count <= 32:
        raise ValueError("checkpoint-attestation-count")
    attestations = []
    for _ in range(count):
        attestation = CheckpointAttestation(
            reader.integer(2),
            reader.integer(4),
            reader.integer(8),
            reader.take(20),
            reader.integer(8),
            reader.take(32),
            reader.take(32),
            reader.take(32),
            reader.integer(8),
            reader.take(32),
            reader.boolean(),
            reader.boolean(),
            reader.integer(1),
            reader.integer(8),
            reader.take(20),
            reader.take(64),
            reader.integer(1),
        )
        if attestations and attestations[-1].guarantor_id >= attestation.guarantor_id:
            raise ValueError("checkpoint-guarantor-order")
        attestations.append(attestation)
    threshold = reader.integer(1)
    reference = reader.sized(2, 1024)
    if reader.offset != len(reader.value):
        raise ValueError("checkpoint-trailing-bytes")
    if (
        threshold != authority.required_guarantors
        or threshold > count
        or len(reference) != 110
    ):
        raise ValueError("checkpoint-threshold-or-settlement")
    verification = CheckpointVerificationInput(
        CheckpointCertificate(
            canonical_header, validity, tuple(attestations), threshold, reference
        ),
        authority.bonded_set,
        authority.registered_checkpoint_id,
        authority.expected_paxeer_chain_id,
        authority.expected_settlement_contract,
        authority.registered_settlement_reference,
        authority.availability_obtained,
    )
    return replace(
        batch,
        checkpoint=PaymentCheckpointEvidence(
            verification, authority.required_guarantors
        ),
    )

# Checkpoints

LayerX commits ordered activity into periodic checkpoints. Custody remains on
Paxeer. The protocol specification (`spec/layerx-protocol/spec.kvx`,
requirement 1 `ac_2` and requirement 22) collapses every activity accepted in
an epoch into exactly one Paxeer checkpoint registration.

The registration carries only header commitments: `protocol_version`,
`network_id`, `epoch`, `batch_number`, `first_sequence`, `last_sequence`,
`previous_state_root`, `resulting_state_root`, `activity_merkle_root`,
`receipt_merkle_root`, `event_merkle_root`, `data_availability_root`,
`oracle_root`, `timestamp`, and `sequencer_id`. Individual activity envelopes,
payloads, or receipts are not transmitted to Paxeer.

## Guarantors

Requirement 22 requires a bonded quorum of guarantors to download the complete
batch body, verify signatures and authority, independently replay every
transition, and recompute every committed root before attesting. A mismatch
of one byte withholds the signature and publishes dissent.

A guarantor signature is both a correctness attestation and a possession
attestation over the data-availability classes
([Data availability](../protocol/data-availability.md)).
Threshold attestation is a bonded economic guarantee. It is not a validity
proof. The certificate format reserves space for an optional validity proof.

The C17 producer is documented in [Guarantor checkpoint production](../operators/guarantor.md).
The wiki states that the beta uses two identities operated by the same cluster:
there is no operational independence. Production operator independence remains
an onboarding requirement.

## Finality ladder

[Finality](../protocol/finality.md) names L0–L4: accepted, sealed, distributed,
attested, settled. Public submit uses `executed` / `batched` / `finalised`
instead ([Commitment levels](../protocol/commitment-levels.md)).

## Checkpoint identity (beta)

The beta specification (`spec/layerx-beta/spec.kvx`, requirement 6) requires
one checkpoint-certificate domain tag and one guarantor-attestation domain tag
for protocol version 2, byte-identical across the native domain table,
`layerx-wire`, and the Solidity constants. Native and Rust verifiers must
enforce the same header-relative attestation freshness window as
`CheckpointRegistry`.

That identity and freshness repair is specified as a beta fix. Treat
cross-language agreement as a qualification gate of `spec/layerx-beta`, not as
a production certification.

## Settlement evidence

Version-2 witness publication on Paxeer is
[Settlement evidence](../operators/settlement-evidence.md).
The hosted Paxeer JSON-RPC relay is [Paxeer boundary](paxeer-boundary.md).

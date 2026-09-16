# Sequencing, checkpoints, and Paxeer finality

LayerX orders activities. Paxeer holds custody and registers checkpoints.
A normal payment does not require a Paxeer transaction
(`spec/layerx-protocol/spec.kvx` requirement 1).

This page is the protocol read of sequencing, guarantors, and checkpoints.
Operator production of attestations is on [Guarantor](Guarantor.md). The
L0–L4 ladder is on [Finality](Finality.md). Public API names
`executed` / `batched` / `finalised` on [Commitment levels](CommitmentLevels.md).

Sources: `include/layerx/lxp_sequencer.h`, `include/layerx/lxp_guarantor.h`,
`include/layerx/lxp_paxeer.h`, `src/sequencer/`,
`spec/layerx-protocol/spec.kvx` requirements 1, 21, 22, 23, and 24.

---

## Sequencer and batches

The sequencer seals signed batches. Requirement 21: a `BatchHeader` carries
exactly `protocol_version`, `network_id`, `epoch`, `batch_number`,
`first_sequence`, `last_sequence`, `previous_state_root`,
`resulting_state_root`, `activity_merkle_root`, `receipt_merkle_root`,
`event_merkle_root`, `data_availability_root`, `oracle_root`, `timestamp`,
and `sequencer_id`. No optional or floating-point fields.

Global sequence numbers are contiguous across history. `first_sequence` of
batch N is `last_sequence` of batch N−1 plus one. A broken root chain, a
sequence gap, or `last_sequence < first_sequence` refuses the batch.

Time for execution is the sealed batch timestamp only. Grant expiry, escrow
timeouts, stream accrual, funding intervals, and governance activation all
read that timestamp. Wall clocks are not consulted during a transition.

The header is signed with the key bound to `sequencer_id`. Replicas and
guarantors reject a missing, malformed, or unauthorised signature. Signing
two different batches at the same `batch_number`, or two activities at the
same global sequence, is sequencer equivocation: the later batch is
rejected, checkpoint eligibility for that height stops, and both headers
are slashable evidence.

A crash recovers from the append-only activity log. Indexes are rebuilt.
If the sequencer is unavailable, new activities stop rather than being
ordered out of band. A new sequencer seals only after an explicit
governance handover activity (`0x00070009`). See [Governance](Governance.md).

Protocol-3 batches append one `batch-maintenance/v1` receipt and one
`batch-maintenance-effects/v1` event leaf after the activity leaves,
including a zero-frame effects leaf when no maintenance module is enabled.
Escrow deadlines, budget rollover, and service defaults run in that
transition, sharing the journal with Programs occupancy finalization.

---

## Guarantors

Requirement 22: threshold attestation is a bonded economic guarantee. It is
not a validity proof. The certificate format reserves space for an optional
validity proof. The producer currently submits an empty validity proof
([Guarantor](Guarantor.md)).

Before attesting, a guarantor downloads the complete batch body (activities,
receipts, events, oracle inputs, state-diff material, recovery metadata),
verifies every actor, grant, oracle, and sequencer signature, independently
replays from `previous_state_root`, and recomputes every committed root.
One-byte disagreement withholds the signature and publishes a dissent
naming the first divergent global sequence. A signature is also a possession
attestation over the availability data.

A checkpoint is finalised when at least the configured threshold of
attestations over a byte-identical certificate are registered on Paxeer.
Fewer attestations before the deadline leave the previous finalised
checkpoint as the settlement anchor. Bonds below the minimum, unresolved
slashing, or governance removal exclude a signature from the count.
Conflicting attestations at the same height slash.

Implemented domains are `LXP/v2/checkpoint-certificate\0` and
`LXP/v2/guarantor-attestation\0` (also used on occupancy protocol 2).
`spec/layerx-beta/spec.kvx` requirement 6 requires the native table, the
wire Domain enum, and the Solidity constants to be byte-identical for
those tags, and a shared header-relative attestation freshness window.

---

## Data availability

Requirement 23: a state root alone is not enough. The certificate commits
`data_availability_root` covering five classes. Retrieval must re-hash to
the committed roots. A failed availability challenge is slashable. Data
loss behind an unfinalised checkpoint refuses finalisation. Data loss
behind a finalised checkpoint keeps emergency exit valid and halts further
finalisation.

---

## Paxeer inputs

Requirement 1 acceptance 3: every Paxeer contract input is a finalised
checkpoint certificate, a membership or balance proof against that root, a
withdrawal nullifier, guarantor signatures and bonds, challenge-window
state, or emergency-exit eligibility. Contracts do not interpret perps
orders, service agreements, escrow rules, or ordinary transfers.

Custody ABI kinds are the `LXP_PAXEER_INPUT_*` values in
`include/layerx/lxp_paxeer.h`. Deposits credit only after a proven
finalised custody fact ([Bridge](Bridge.md)). Withdrawals debit LayerX
first, then pay on Paxeer at most once.

If Paxeer is unreachable, ordinary LayerX activity continues. Only
checkpoint registration, Paxeer payouts, and dispute or emergency-exit
finalisation suspend.

---

## Start here

- [Finality](Finality.md)
- [Guarantor](Guarantor.md)
- [Commitment levels](CommitmentLevels.md)
- [Bridge](Bridge.md)
- [Paxeer boundary](PaxeerBoundary.md)
- [Data availability](DataAvailability.md)

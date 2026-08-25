<!--
Draft copy for the GitHub wiki page "Finality".
The wiki has no PR flow, so this file is the reviewable source. After this PR
merges, paste the body below (everything under the first `# Finality`) into the
wiki page. Do not commit this note to the wiki.
-->

# Finality

Ordered instantly in-channel. Anchored to Paxeer on the L0 → L4 ladder.

An activity is not final all at once. Each step names who is on the hook if the claim turns out to be wrong. The guarantee behind a batch is not a validity proof. It is bonded re-execution, a challenge window, and withdrawal limits.

LayerX and the Paxeer settlement stack now live in one monorepo, so the whole path — from the sequencer that orders an activity to the Paxeer contracts that register a checkpoint — is auditable in one place. Paxeer Network is EVM chain ID `125`, and its node and contracts live under `paxeer-network/`. Custody still only moves on the Paxeer side; co-location changes nothing about the trust boundary.

---

## The ladder

| Step | Name | What it means | Who is on the hook |
| --- | --- | --- | --- |
| L0 | Accepted | The sequencer orders the activity into the current batch and issues a receipt chained to the state root | The sequencer, for inclusion and ordering in this batch |
| L1 | Sealed | The batch is sealed. Ordering is now fixed. Empty-seal / liveness timers apply if the sequencer stalls | Sequencer liveness; replicas hold the sealed bytes |
| L2 | Distributed | The sealed batch and DA bundle go to the guarantor quorum for independent re-execution | Data availability: anyone must be able to replay |
| L3 | Attested | Bonded guarantors re-execute and attest byte-identical results | Guarantor bonds. Conflicting attestations slash |
| L4 | Settled | The checkpoint lands on Paxeer L1. Custody never leaves Paxeer | Paxeer custody, challenge window, withdrawal limits, emergency exits |

Fast path is L0–L1: agents get a receipt in-channel. Settlement path is L2–L4: the same history becomes something Paxeer will hold and an exit can prove against.

---

## What stands behind a batch

Independent operators re-run every batch and check that they get the same result, byte for byte. They are guarantors. A signature is an attestation, not a SNARK.

Before an activity is L4-settled, what stands behind it is:

- guarantor bonds,
- the challenge window,
- limits on how fast anything can be withdrawn.

Those put a price on what a dishonest quorum could cost you. The whitepaper and security model name the risks that remain.

---

## Everything in the batch replays — including programs

A batch is not just payments. Program calls, program-owned account transfers, and storage-occupancy settlement all land in the same ordered history and are re-executed the same way. Program execution is deterministic for exactly this reason: guarantors must reach byte-identical results, and every monetary effect a program produces is a `402LXP` transfer set carried in the batch, never a private balance write. Occupancy charges — rent on namespace bytes held across batches — settle into the batch receipt as replay-checkable evidence, so a guarantor can reproduce them from the log alone.

---

## Sequencer stall (named, not silent)

If the sequencer stops sealing, the protocol does not pretend otherwise. The security model specifies empty-seal and liveness timers and an exit path onto Paxeer. LayerX is designed so ordinary activity never requires a Paxeer transaction — and so an exit still exists when the fast path is stuck.

---

## Replay is the authority

The append-only activity log is the record. Database indexes are disposable. Replicas serve full history. Guarantors replay before they attest. A node that cannot reproduce a state root from the log is not a source of truth.

That is why consensus execution forbids floating point, local clocks, and unstable iteration: L3 only works if two honest machines get the same bytes.

---

## What LayerX is not claiming here

This ladder is how LayerX qualifies a checkpoint onto Paxeer. It is not a public-mainnet RPC, and it is not "final on L0." Receipts at accept are evidence of ordering in-channel. Custody moves on the Paxeer side at L4. Limited beta opens September 7; source is open for inspection while the public lane is qualified, and there is no public RPC, faucet, or explorer for LayerX itself yet.

---

## Start here

- Home
- Protocol — LXC envelope and the three rules
- Modules — the eight economic modules and the programs surface
- Security

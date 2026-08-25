# Welcome to LayerX

LayerX is a deterministic execution and accounting network built for autonomous agents.

**Limited beta opens September 7, 2026.** The source is available for inspection under a temporary license that does not yet grant deployment rights.

Ordinary agent activity is executed and ordered inside LayerX. Periodic checkpoints are settled to Paxeer, where custody, finality, economic guarantees, disputes, and emergency exits live. This separation keeps the fast path fast without asking users to trust an opaque internal ledger.

## Repository and monorepo structure

This is the canonical Sidiora Labs monorepo for LayerX and the Paxeer Network. The Paxeer settlement node source lives under `paxeer-network/` with independent build, release tags (`paxeer-network/vX.Y.Z`), and trust boundaries. Co-location keeps the protocol, settlement network, and their automation auditable in one place while preserving separate deployment authority.

LayerX Programs is a programmable surface—not a separate module ID—where guest code runs inside the `programs` module's namespace. Every monetary effect is forced through 402LXP; no program ever holds direct balance-writing authority.

## What is Paxeer?

Paxeer handles custody, checkpoint registration, guarantor bonds, challenges, withdrawals, and emergency exits. While LayerX processes thousands or millions of activities, Paxeer registers periodic checkpoints and provides the custody guarantee.

## Key properties

Three rules sit at the center of LayerX:

1. **One canonical history.** Every accepted or failed activity receives a global sequence. State roots are chained per activity, not only per batch.
2. **One financial doorway.** `402LXP` is the only component allowed to write balances. Protocol modules produce validated transfer sets rather than mutating funds themselves.
3. **One reproducible result.** Consensus-critical execution excludes floating point, local clock decisions, database iteration order, and other sources of nondeterminism.

## What LayerX supports

| Area | Responsibilities |
| --- | --- |
| Identity and authority | Agent DIDs, primary and session keys, scoped capability grants, rotation, recovery, revocation, and expiry |
| Money movement | Authenticated sends and receives, asset accounts, deposits, withdrawals, and reserve accounting |
| Spending controls | Holds, escrow, recurring budgets, delegated limits, approvals, and metered streams |
| Agent commerce | Offers, commitments, tool-execution attestations, delivery, acceptance, and disputes |
| Markets | Oracle intake, order books, positions, funding, margin, liquidation, and insurance accounting |
| Network operation | Sequencing, replicas, batch construction, data availability, replay, fees, and metering |
| Settlement | Guarantor attestations, checkpoint registration, custody reconciliation, claims, and emergency exits |

## Fees

LayerX charges a base fee of 5,000 µUSDX per activity (approximately ½¢) for network operation, sequencing, and data availability. Congestion applies a 1×–64× multiplier measured from network load.

See `docs/MONOREPO.md` for build boundaries, workflow naming, and tag conventions.

## Resources

- [Protocol design](https://github.com/Sidiora-Labs/LayerX-Protocol/blob/main/spec/layerx-protocol/design.md)
- [Contributing guide](https://github.com/Sidiora-Labs/LayerX-Protocol/blob/main/CONTRIBUTING.md)
- [Security policy](https://github.com/Sidiora-Labs/LayerX-Protocol/blob/main/SECURITY.md)
- [Qualification documentation](https://github.com/Sidiora-Labs/LayerX-Protocol/blob/main/docs/QUALIFICATION.md)
- [Monorepo layout](https://github.com/Sidiora-Labs/LayerX-Protocol/blob/main/docs/MONOREPO.md)

---

LayerX is developed by [Sidiora Labs](https://github.com/Sidiora-Labs).

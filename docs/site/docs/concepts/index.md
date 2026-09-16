# Concepts

LayerX is a deterministic execution and accounting network. Three rules sit
at the center ([Protocol](../protocol/index.md)):

1. One canonical history. Every accepted or failed activity receives a global
   sequence. State roots chain per activity.
2. One financial doorway. `402LXP` is the only component allowed to write
   balances.
3. One reproducible result. Consensus-critical execution excludes floating
   point, local clocks, and unstable iteration.

| Topic | Page |
| --- | --- |
| Identity, keys, grants | [Identity](identity.md) |
| Named accounts and sequences | [Accounts](accounts.md) |
| Assets, issuance, fees | [Assets](assets.md) |
| Activity envelope | [Activities](activities.md) |
| Receipts and verification | [Receipts](receipts.md) |
| Checkpoints and guarantors | [Checkpoints](checkpoints.md) |
| Paxeer custody and exits | [Paxeer settlement](paxeer-settlement.md) |
| Hosted Paxeer JSON-RPC relay | [Paxeer boundary](paxeer-boundary.md) |

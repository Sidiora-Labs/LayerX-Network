# Paxeer settlement

Paxeer holds custody, checkpoint registration, guarantor bonds, challenges,
withdrawals, disputes, and emergency exits. An ordinary LayerX activity does
not require a Paxeer transaction (`spec/layerx-protocol/spec.kvx`,
requirement 1).

Paxeer Network is EVM chain ID `125`. Its node and contracts live under
`paxeer-network/`. Co-location in this monorepo does not grant either side new
authority over the other ([Monorepo](../overview/monorepo.md)).

## Division of responsibility

| LayerX | Paxeer |
| --- | --- |
| Identities, activity ordering, balances (`402LXP`) | Asset custody |
| Holds, escrow, budgets, streams, services, perps | Deposits and withdrawals |
| Programs and deterministic replay | Checkpoint registration |
| Receipts, DA, fees | Guarantor bonds and attestations |
| | Slashing, emergency exits, external settlement |

Requirement 1 of `spec/layerx-protocol/spec.kvx` assigns those columns
exclusively. A Paxeer contract interface that would interpret a perps order, a
service agreement, or an ordinary agent-to-agent transfer is non-conformant.

## Deposits and withdrawals

A deposit is credited inside LayerX only after it is proven against finalized
Paxeer custody state, as a `402LXP` transfer from `system:paxeer-reserve` to
the agent main account. A reused deposit identifier is rejected.

A withdrawal first transfers from the agent account to
`system:paxeer-withdrawals`. The funds are payable on Paxeer only after a
finalized checkpoint whose state root covers that transfer, and at most once
under a unique nullifier (requirement 1 `ac_7`–`ac_8`, requirement 24).

The beta specification (`spec/layerx-beta/spec.kvx`, requirement 10) additionally
requires withdrawal settlement to bind the asset recorded at request time. A
caller-supplied asset that does not match is refused with no transfer and no
state mutation.

## Emergency exit

If the sequencer refuses service or becomes unavailable, requirement 1 `ac_10`
and requirement 24 allow an agent to recover its custodied balance on Paxeer
against the most recently finalized checkpoint state root, without sequencer
cooperation. The platform specification ships that path in v1 as a guided
Settings flow (`spec/layerx-platform/spec.kvx`, decision `emergency_exit_v1`).
The web application has an exit surface at `/app/settings/exit`
([Human journeys](../human/journeys.md)).

## Hosted and contract surfaces

| Surface | Page |
| --- | --- |
| TLS JSON-RPC relay to the in-cluster Paxeer node | [Paxeer boundary](paxeer-boundary.md) |
| Authenticated custody credit (Bridge module 8, protocol 3) | [Custody](../human/custody.md) |
| Checkpoint witness publication | [Settlement evidence](../operators/settlement-evidence.md) |
| Guarantor producer | [Guarantor](../operators/guarantor.md) |
| Finality ladder | [Finality](../protocol/finality.md) |

## Status

There is no LayerX mainnet. Custody and settlement live on Paxeer. The public
testnet exposes a gateway API and a faucet. Hosted beta bring-up deploys
Paxeer contracts on chain id `125` inside the disposable cluster
([Beta cluster](../operators/beta-cluster.md)). Live production certification
of peers, DNS, TLS, and KMS is outside the beta bar
(`spec/layerx-beta/spec.kvx`).

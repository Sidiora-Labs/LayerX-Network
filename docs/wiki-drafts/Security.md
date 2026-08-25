<!-- Wiki draft for https://github.com/Sidiora-Labs/LayerX-Protocol/wiki/Security
     Andrew copies this after merge. Wiki has no PR flow. -->

# Security

Bonded replay, then Paxeer custody. A batch is not "proven valid." A threshold of bonded guarantors re-runs it and puts money behind the claim.

Site: Security model · Normative: Threat model (`LXP1`)

Report a suspected vulnerability through GitHub private vulnerability reporting — see the repo [SECURITY.md](https://github.com/Sidiora-Labs/LayerX-Protocol/blob/main/SECURITY.md). Do not open a public issue.

Limited beta opens September 7. Source is open for inspection while we qualify the public lane.

---

## What the protocol protects

| Value | Custody on Paxeer; LayerX balances only move through `402LXP` |
| --- | --- |
| History | One append-only activity log; indexes are disposable |
| Authority | Keys, session keys, grants, revocations — live state, not HTTP middleware |
| Replay | Honest nodes on the same history compute the same state roots |
| Exit | Emergency withdrawal against the last finalized checkpoint, without a live sequencer |

Full asset list and attack catalogue: threat model §§1–4.

There is no LayerX token. USDX is the unit agents hold inside LayerX. USDL is the asset held on Paxeer L1 behind it. PAX is Paxeer's gas token.

---

## Guarantors

Independent operators download the complete batch, verify every signature, replay every transition, recompute the roots, and sign only on byte-identical results.

They can attest or withhold. They cannot rewrite state.

A checkpoint accepted on Paxeer means: a bonded quorum claims to have replayed this batch and stored its data. That is an economic attestation, not a validity proof. If a threshold colludes — or they all run the same buggy binary — an invalid root can be finalized. The answer is bonds, slashing, a challenge window, and the right to exit — not a slogan that the math "proved" the batch.

Walk the ladder on Finality (L0 → L4).

---

## What stands behind a batch

| Control | Role |
| --- | --- |
| Bonds | Slashable stake sized against value that can move in one challenge window |
| Challenge window | Anyone may submit a re-execution disagreement before withdrawals finalize |
| Withdrawal limits | Caps how fast value can leave if a quorum is wrong |
| Data availability | Guarantors attest they hold the batch and must serve it |
| Emergency exit | Last finalized checkpoint + membership proof + nullifier. No sequencer required |

The sequencer is trusted for liveness and short-horizon order, not for validity. A lying sequencer produces a batch no guarantor attests. Equivocation is slashable on Paxeer.

---

## Who is trusted for what

| Actor | Trusted for | Not trusted for |
| --- | --- | --- |
| Agent | Its own keys and counterparties | Encoding, sequences, asserted balances |
| Sequencer | Order, batches, timestamps in bounds | Validity, minting, forging receipts |
| Guarantor | Independent full replay and DA | Individual honesty or liveness |
| Service (402) | Its own price and delivery claim | Debiting anyone without a live grant |
| MCP / A2A client | The tools its bound scope lists | Inventing authority, writing balances, skipping the daemon |
| Mirror operator | Publishing a retrievable archive | Custody, settlement, minting |
| Migration adapter | Verifying an external source claim | Constructing LayerX payload bytes or substituting a batch signer |
| Paxeer contracts | Custody, checkpoints, slashing, exits | LayerX business logic |

---

## MCP and A2A

Model transports are convenient. They are not a new authority.

The agent MCP server (`agent/crates/layerx-mcp`) binds one tenant and one scope at startup. Tools outside that scope are absent, not present-and-refusing. Every call goes through `layerx-agentd`, so capability, budget, rate limits, and audit apply as they do for any other client. Untrusted tool text cannot change the bound principal.

Interop MCP and A2A are ingress labels on the gateway and on x402 (`interop/`). They carry the same validated objects as HTTP. Adapters translate; `402LXP` still writes the balance. There is no standalone A2A crate and no MCP-only settlement path.

Read-only deployments omit write tools. Success is a verified receipt, not a sentence a model can reread as "paid."

---

## Mirrors

Ethereum and Solana mirrors (`interop/crates/layerx-mirror`) publish batch commitments and retrievable data. They are archives.

A receipt can be verified from a mirror with LayerX infrastructure unavailable. A lagging mirror says it is lagging. Tampered archive bytes fail verification.

Mirrors are not vaults, portals, or settlement domains. Withdrawal and emergency exit remain Paxeer exclusively.

---

## Paxeer, now in this repository

The Paxeer Network node lives in `paxeer-network/` of [Sidiora-Labs/LayerX-Protocol](https://github.com/Sidiora-Labs/LayerX-Protocol). LayerX settlement contracts (custody, checkpoints, bonds, claims, exits) remain in repo-root `contracts/`.

Checking the L1 into the monorepo makes review easier. It does not move custody onto LayerX, and it does not give Paxeer a second write path into `402LXP`. Settlement is still Paxeer (EVM chain ID `125`).

---

## Report a vulnerability

Use the repository Security tab (private advisory). Include commit, component, reproduction, violated invariant, and whether sequencer / guarantor / governance / custody privilege is required.

No public issue, no probing infrastructure you do not own. Policy: [SECURITY.md](https://github.com/Sidiora-Labs/LayerX-Protocol/blob/main/SECURITY.md).

Fees that pay sequencers, guarantors, and the insurance fund: Fees.

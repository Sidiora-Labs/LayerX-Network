# Unified Network Architecture

One repository, one network, two execution domains.

Paxeer is a Cosmos-SDK chain with a native EVM. LayerX is a deterministic execution and accounting network built in C and Rust. Both live in the same monorepo and share a single network identifier, but they execute in different runtimes. The bridge between them is not a message queue or a bridge contract — it is a set of native precompiles that run inside the Paxeer EVM and verify cryptographic evidence produced by LayerX.

## The two domains

**Paxeer EVM / Cosmos** is written in Go. The chain binary builds from `node/`, modules live in `modules/`, precompiles in `precompiles/`, and the build rules are in `chain.mk` with the Go module root at `go.mod`. Paxeer provides account management, bank transfers, staking, governance, and an EVM that can call into all of these through precompiled contracts.

**LayerX** has three language domains. The core node, sequencer, and consensus layer are in C under `src/` and `cmd/layerxd`. The agent runtime, platform services, and hosted infrastructure are in Rust under `agent/`, `platform/`, `human/`, and `programs/`. LayerX produces signed receipts, batch headers, state proofs, and checkpoint certificates — the evidence that the Paxeer precompiles verify.

## The proof-carrying connector

Seven precompiles at well-known EVM addresses connect the two domains. Each one is declared as a Solidity interface and implemented as a native Go function inside the EVM module. `LayerXVerify`, `Addr`, `LayerXCustody` and `LayerXAnchor` see only Paxeer state and calldata and verify submitted LayerX evidence using the `layerxproof/codec` and `layerxproof/verify` Go packages, both of which are pure — no clock, no network, no database. `LayerXExchange`, `LayerXBridge` and `Launchpad` (below) are newer additions with different jobs: the first two move value between chains or domains under signed evidence, and the third runs a self-contained AMM on Paxeer alone.

### LayerXVerify — `0x0000000000000000000000000000000000001012`

Stateless verification of LayerX evidence. Every trust anchor is calldata: the precompile reads no chain state, so the caller decides which sequencer key, sequencer identity, batch range, state root or program it trusts.

Declared in `precompiles/layerxverify/LayerXVerify.sol`:

| Method | Purpose |
|--------|---------|
| `verifyEd25519(publicKey, domain, message, signature) → bool` | Strict Ed25519 over SHA256(LayerX domain tag ‖ message) for domain 0..19, or over message itself for domain 255. |
| `verifyReceipt(receipt, sequencerPublicKey) → ReceiptFacts` | Canonical receipt decode and the sequencer signature over its digest. |
| `verifyReceiptInclusion(receipt, proof, batchHeader, headerSignature, sequencerId, sequencerPublicKey, firstBatchNumber, lastBatchNumber) → ReceiptFacts, BatchFacts` | Receipt plus a Merkle path to the receipt root of a signed batch header inside the authorised batch range. |
| `verifyStateProof(witness, stateRoot) → moduleId, key, value` | A version-2 native state witness folded to stateRoot. |
| `verifyDiscoveryProof(payload, proofMaterial, programId, stalenessMs, sequencerPublicKey) → DiscoveryFacts` | A sequencer program head attestation whose validity window is exactly stalenessMs. |

`verifyEd25519` answers with a boolean. Every other method reverts unless the evidence verifies, so a returned value is a verified fact.

### Addr — `0x0000000000000000000000000000000000001004`

Binding methods that link a Paxeer EVM address to a LayerX DID key. Declared in `precompiles/addr/Addr.sol`:

| Method | Purpose |
|--------|---------|
| `associate(v, r, s, customMessage) → paxAddr, evmAddr` | Associate a Pax address with an EVM address. |
| `associatePubKey(pubKeyHex) → paxAddr, evmAddr` | Associate using a compressed public key. |
| `bindLayerX(didPublicKey, signature)` | Bind `msg.sender` to `did:layerx:<hex of didPublicKey>`. The signature is the DID key's strict Ed25519 over `"LX:PAXEER-BIND:v1" ‖ chain id ‖ msg.sender ‖ layerXBindNonce(msg.sender)`. |
| `unbindLayerX()` | Remove the caller's LayerX binding and consume a nonce. |
| `getPaxAddr(addr) → response` | Look up the Pax address for an EVM address. |
| `getEvmAddr(addr) → response` | Look up the EVM address for a Pax address. |
| `getLayerXDid(addr) → didPublicKey, did` | Get the LayerX DID for an EVM address. Reverts when unbound. |
| `getEvmAddrByLayerX(didPublicKey) → evmAddr` | Get the EVM address for a DID public key. |
| `layerXBindNonce(addr) → nonce` | The nonce the next `bindLayerX` signature must cover. |
| `getUnifiedAccount(addr) → evm, paxAddr, didPublicKey, layerxMainAccountId` | Returns all four identifiers for an address. Never reverts for a missing identity. |

Events: `LayerXBound(evm, didPublicKey, nonce)` and `LayerXUnbound(evm, didPublicKey, nonce)`.

### LayerXCustody — `0x0000000000000000000000000000000000001013`

Native LayerX custody. Funds are held by the `layerxcustody` module account and leave it only against verified LayerX evidence: a sequencer-signed withdrawal receipt included in a finalized batch, or a native state proof of a balance under the latest finalized state root plus the account authority's recipient signature. Declared in `precompiles/layerxcustody/LayerXCustody.sol`:

| Method | Purpose |
|--------|---------|
| `deposit(beneficiary) → depositId` | Custody `msg.value` of the native coin for a LayerX account. |
| `depositToken(pointer, amount, beneficiary) → depositId` | Custody a bank denom addressed by its registered ERC20 pointer. |
| `requestWithdrawal(receipt, proof, header, headerSignature) → claimId, availableAt` | Verify a withdrawal and queue its claim; payable after the withdrawal delay. |
| `finaliseWithdrawal(receipt, proof, header, headerSignature) → claimId` | Re-verify a withdrawal and pay the recipient the receipt names. Queues the claim first when it was never requested. |
| `requestForcedExit(witness, batchNumber, account, assetId, recipient, recipientSignature) → claimId, availableAt` | Prove a whole balance under the latest finalized state root and queue its exit. |
| `executeForcedExit(witness, batchNumber, account, assetId, recipient, recipientSignature) → claimId` | Pay a forced exit, queueing it first when it was never requested. |

View methods: `depositCount`, `depositNonce`, `getDeposit`, `getDepositByIndex`, `getClaim`, `nullifierStatus`, `getAsset`, `assetByPointer`, `nativeAssetId`, `exitEligible`.

### LayerXAnchor — `0x0000000000000000000000000000000000001014`

Checkpoint registry, finality authority, availability record and guarantor bonds. Certificates are self-verifying, so anyone may submit them. A checkpoint is final once the required threshold of bonded, active guarantors attested it, it continues the finalized chain, no challenge is open and the challenge window elapsed. Declared in `precompiles/layerxanchor/LayerXAnchor.sol`:

| Method | Purpose |
|--------|---------|
| `submitCheckpoint(header, headerSignature, certificate) → checkpointId, status` | Submit a 354-byte batch header with its sequencer signature and guarantor certificate. Status: 1 submitted, 2 final. |
| `submitAvailabilityAttestation(attestation) → availabilityMask` | One 274-byte guarantor attestation over a known checkpoint. |
| `finalize(batchNumber) → bool` | Finalize a submitted checkpoint whose challenge or window has cleared. |
| `registerGuarantor(guarantorId, signer) → bool` | Register a new guarantor (payable, must meet min bond). |
| `increaseBond(guarantorId) → bool` | Add bond to an existing guarantor. |
| `beginUnbond(guarantorId, amount) → completionTime` | Start unbonding. The bond stays slashable until completion. |
| `completeUnbond(guarantorId) → amount` | Withdraw an expired unbonding. |
| `submitEquivocation(evidenceA, evidenceB) → slashed` | Two 274-byte attestations by one guarantor naming different checkpoints for one batch. |
| `openChallenge(batchNumber, kind, evidenceHash) → challengeId` | kind: 0 fraud, 1 data availability. The value must be the challenge bond. |
| `resolveChallenge(challengeId, upheld) → bool` | Authority only. |
| `activateGuarantor(guarantorId) → bool` | Authority only. |
| `setSequencerAuthorization(sequencerId, publicKey, firstBatchNumber, lastBatchNumber) → bool` | Authority only. |

View methods: `latestFinalized`, `checkpoint`, `finalizedStateRoot`, `finalizedReceiptRoot`, `guarantor`, `threshold`, `statusOf`.

The `statusOf` function returns the checkpoint status ladder: **0 unknown**, **1 submitted**, **2 final**.

### LayerXExchange — `0x0000000000000000000000000000000000001015`

Records pending trading intents; it never matches an order on Paxeer.
Every write emits an event carrying an `intentId`
(`sha256("LXP/Paxeer/exchange-intent/v1" || chain id || this || owner ||
kind || nonce)`) for the LayerX intent router. Margin only moves through
`layerxcustody`: a margin deposit is a `LayerXCustody` deposit tagged for
this account, and a margin withdrawal is paid out later by a proof-carrying
`LayerXCustody` withdrawal. Declared in
`precompiles/layerxexchange/LayerXExchange.sol`:

| Method | Purpose |
|--------|---------|
| `depositMargin(account) → intentId, depositId` | Custody `msg.value` as margin for a LayerX account. |
| `depositMarginToken(pointer, amount, account) → intentId, depositId` | Custody a bank-denom pointer as margin. |
| `withdrawMargin(account, assetId, amount) → intentId` | Ask LayerX to release margin to a withdrawable balance. |
| `placeOrder(market, side, price, qty, tif) → intentId` | Record a spot/perps order intent. |
| `cancelOrder(orderId) → intentId` | Record a cancel intent. |
| `requestSettlement(positionId) → intentId` | Record a settlement-request intent. |

View methods `getIntent`, `intentNonce`, `getMarket`, `getOrder`,
`getPosition` and `getMargin` prove LayerX state under a caller-supplied
witness and finalized batch number rather than reading over the network.
`human/crates/layerx-intents/src/precompile.rs` decodes and types these
events (`ExchangeOrder`, `ExchangeCancel`, `ExchangeSettle`,
`ExchangeMarginDeposit`, `ExchangeMarginWithdraw`), but nothing in the tree
calls that router outside its own tests yet — see
[Not yet built](#not-yet-built).

### LayerXBridge — `0x0000000000000000000000000000000000001016`

An attested bridge between Paxeer and registered external chains,
distinct from the LayerX kernel's own `bridge` module (module `8`, see
[Bridge](Bridge.md)), which is the LayerX↔Paxeer custody path. Governance
registers each external chain (vault address, finality depth), the
attestor set and threshold, and per-asset caps; genesis registers no
chain, so the bridge is dormant until governance brings one up. Declared
in `precompiles/layerxbridge/LayerXBridge.sol`:

| Method | Purpose |
|--------|---------|
| `bridgeIn(chain, vault, txHash, logIndex, recipient, asset, amount, signatures) → denom` | Mint the bridged denom against at least `threshold` attestor signatures over the vault deposit digest. |
| `bridgeOut(chain, asset, amount, recipient) → nonce` | Burn the caller's bridged denom and emit `BridgeOut` for attestors to countersign. |

View methods: `getChain`, `getAttestors`, `getCap`, `isPaused`,
`isNullified`. The `layerx-bridge-relayer` service journals observed
Ethereum events and signed transactions and gathers attestor signatures
before broadcast; `modules/layerxbridge/ATTESTATION.md` is the byte-exact
signing specification.

### Launchpad — `0x0000000000000000000000000000000000001017`

A native token-launch AMM local to Paxeer — it does not touch LayerX
evidence. Each market is a fixed-supply, 6-decimal `tokenfactory` denom
priced against a quote denom on a virtual-reserve constant-product curve;
`token` is the denom's ERC20 pointer. Declared in
`precompiles/launchpad/Launchpad.sol`:

| Method | Purpose |
|--------|---------|
| `createMarket(name, symbol, feeStrategy) → token, denom` | Launch a new fixed-supply market. |
| `buy(token, quoteIn, minOut, recipient, deadline) → amountOut` | Buy along the curve. |
| `sell(token, amountIn, minOut, recipient, deadline) → amountOut` | Sell along the curve. |
| `claimFees` / `executeBurn` / `executeAirdrop` / `claimAirdrop` / `executeLpRewards` | Distribute accumulated fees per the market's chosen fee strategy. |
| `pause(token)` / `unpause(token)` | Governance-only trading halt. |

View methods include `quoteBuy`, `quoteSell`, `getReserves`, `getPrice`,
`getMarket(s)` and `getConfig`. This constant-product curve is a Paxeer EVM
feature; it is not the LayerX kernel module described in
[Roadmap 8.5](Roadmap.md#85-constant-product-swap), which remains
unimplemented on the LayerX side.

## Account binding

An EVM address can be linked to a `did:layerx` identifier. The binding requires consent from both keys: the LayerX DID key signs a message, and the EVM address sends the transaction.

The message format is defined in `modules/evm/types/layerx_binding.go`:

```
"LX:PAXEER-BIND:v1" || chain_id (uint256 big-endian) || evm_address (20 bytes) || nonce (uint64 big-endian)
```

The domain string `LX:PAXEER-BIND:v1` is the constant `LayerXBindDomain`. The DID is rendered as `did:layerx:<64 lowercase hex characters>`. The LayerX main account name for a DID is `agent:did:layerx:<hex>:main`, and its native account ID is derived via `codec.DeriveAccountID`.

One EVM address has at most one DID, and one DID has at most one EVM address. Genesis can pre-populate the binding table; `ValidateLayerXGenesisEntry` checks that every entry under the binding prefixes is well formed and that every DID public key is canonical.

## Custody

The `layerxcustody` module (in `modules/layerxcustody/`) holds funds in a module account. Coins enter through `deposit` or `depositToken`. They leave only through proof-carrying claims:

- **Withdrawals** — a sequencer-signed receipt proving a LayerX withdrawal, included in a finalized batch via `requestWithdrawal`. After the withdrawal delay, `finaliseWithdrawal` pays the recipient.
- **Forced exits** — a state proof of the entire balance under the latest finalized state root, plus the recipient's signature, via `requestForcedExit`. After the delay, `executeForcedExit` pays out.
- **Emergency exits** — a special fast path when the custody module enters emergency mode.

Nullifiers prevent double-spending. Every claim is bound to a nullifier; a nullifier can be reserved, consumed, or cancelled but never reopened.

Genesis for the custody module is written by `platform/hosted/paxeer/custody-genesis.py`, which outputs a JSON section that `init-chain.sh` merges into the Paxeer genesis. It configures the network ID, sequencer authorization, withdrawal delays, forced-exit delays, liveness bound, and the asset map.

## Anchor

The `layerxanchor` module (in `modules/layerxanchor/`) is the finality authority for LayerX checkpoints. Its status ladder is **instant** (0 unknown), **sealed** (1 submitted), and **final** (2 final). The constants are defined in `modules/layerxanchor/types/state.go`.

A checkpoint starts as submitted when `submitCheckpoint` accepts a valid certificate. It becomes final when `finalize` runs and the challenge window has elapsed, no challenges are open, and the required threshold of bonded, active guarantors attested it.

Guarantors bond native tokens. They can register, increase their bond, begin unbonding (with a configurable delay during which the bond stays slashable), and complete unbonding. Equivocation — signing two different checkpoints for the same batch — results in a slash. Fraud and data-availability challenges can be opened against a checkpoint; the authority resolves them.

Genesis for the anchor module is written by `platform/hosted/paxeer/anchor-genesis.py`. It configures the authority account, Paxeer chain ID, network ID, certificate threshold, minimum bond, challenge parameters, unbonding delay, sequencer authorization, and optional pre-registered guarantors. The anchor point — the batch the first checkpoint must continue — is set at genesis.

## Not yet built

The following components are planned but not yet implemented:

- **Shared single endpoint** — a unified RPC endpoint that routes requests to the appropriate domain without requiring callers to know which chain to target.
- **Unified index / explorer account page** — a single view that merges activity from both domains for one account. `platform/hosted/indexer` (`layerx-indexer`) now ingests both chains into one SQLite store behind one `GET /v1/history/{account}` shape (`platform/hosted/indexer/src/store.rs`), but a query is still keyed by one domain's own identifier — a LayerX account id or a Paxeer address — since the indexer does not resolve the `Addr` binding between them. There is still no page that merges one person's activity across both domains automatically.
- **Intent routing, end to end** — `human/crates/layerx-intents` decodes and types `layerxexchange`, `layerxbridge` and `launchpad` precompile events into LayerX intents (`human/crates/layerx-intents/src/precompile.rs`), so the mapping itself exists. Nothing in the tree calls that router outside its own tests, so no running service yet resolves a cross-domain intent automatically end to end.
- **Light-client verification of Paxeer deposits on LayerX** — a light-client proof that a deposit transaction was included in a Paxeer block, verifiable on the LayerX side without a full Paxeer node.

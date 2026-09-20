# Unified Network Architecture

One repository, one network, two execution domains.

Paxeer is a Cosmos-SDK chain with a native EVM. LayerX is a deterministic execution and accounting network built in C and Rust. Both live in the same monorepo and share a single network identifier, but they execute in different runtimes. The bridge between them is not a message queue or a bridge contract — it is a set of native precompiles that run inside the Paxeer EVM and verify cryptographic evidence produced by LayerX.

## The two domains

**Paxeer EVM / Cosmos** is written in Go. The chain binary builds from `node/`, modules live in `modules/`, precompiles in `precompiles/`, and the build rules are in `chain.mk` with the Go module root at `go.mod`. Paxeer provides account management, bank transfers, staking, governance, and an EVM that can call into all of these through precompiled contracts.

**LayerX** has three language domains. The core node, sequencer, and consensus layer are in C under `src/` and `cmd/layerxd`. The agent runtime, platform services, and hosted infrastructure are in Rust under `agent/`, `platform/`, `human/`, and `programs/`. LayerX produces signed receipts, batch headers, state proofs, and checkpoint certificates — the evidence that the Paxeer precompiles verify.

## The proof-carrying connector

Four precompiles at well-known EVM addresses connect the two domains. Each one is declared as a Solidity interface and implemented as a native Go function inside the EVM module. A precompile sees only Paxeer state and calldata; it never makes an outbound network call. Its job is to verify submitted LayerX evidence using the `layerxproof/codec` and `layerxproof/verify` Go packages, both of which are pure — no clock, no network, no database.

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

## The single network endpoint

One endpoint serves the whole network: the hosted gateway at
`https://api.testnet.layerx.network/rpc`. A caller does not choose a chain — the
method name decides the domain.

- `eth_*`, `net_*` and `web3_*` are the Paxeer EVM JSON-RPC, relayed verbatim to
  the chain through the Paxeer boundary named by
  `LAYERX_GATEWAY_PAXEER_RPC_URL`. `eth_sendRawTransaction` is included: a
  signed transaction sent to the gateway lands on Paxeer. The node's own key
  never signs for a caller, so `eth_accounts`, `eth_coinbase`,
  `eth_sendTransaction`, `eth_sign`, `eth_signTransaction`, `eth_signTypedData`,
  `eth_signTypedData_v4` and `eth_mining` are refused, as are `eth_subscribe`
  and `eth_unsubscribe` — the gateway's WebSocket carries `lx_subscribe` only.
- `lx_*` are the LayerX methods, unchanged.
- `px_*` are unified cross-domain reads that answer from the precompiles above:
  `px_resolveAccount`, `px_getAccount`, `px_getBalances`, `px_listAssets` and
  `px_getNetwork`.

A JSON-RPC batch may mix all three namespaces; entries keep their ids and their
order. The `px_*` and `lx_*` reads need no API key; they are gated only by the
public read budget.

`px_resolveAccount` and `px_getAccount` take one identifier — an
`0x`-prefixed EVM address, a `did:layerx:<hex>` DID, or the bare 64-hex DID
public key — and answer with both halves of the identity (`evm_address`,
`pax_address`, `layerx_did`, `layerx_account`, `bound`) as
`getUnifiedAccount`, `getLayerXDid` and `getEvmAddrByLayerX` report them.
`px_listAssets` joins the LayerX asset list to the custody asset map read from
`getAsset`. `px_getBalances` resolves the account and then, per asset, reports
the custody record, the Paxeer bank balance of the custody denom for that EVM
address, and the LayerX account for the same asset. `px_getNetwork` reports the
chain id, the head block, the LayerX node info, and the latest finalized
checkpoint with its status from the anchor's ladder. Both joins are bounded to
the first sixteen assets and say so in `joined_limit`.

The exact parameter and result shapes are in the gateway's
[`openrpc.json`](https://github.com/Sidiora-Labs/Layerx-protocol/blob/main/platform/hosted/gateway/openrpc.json),
served live at `GET /rpc/schema`.

Clients that want to bind an account call `bindLayerX` with a signature the
`layerx-client` crate builds: `layerx_client::paxeer_binding::Binding` assembles
the exact consent message described below and signs it with the DID key.

## Not yet built

The following components are planned but not yet implemented:

- **Unified index / explorer account page** — a single view that merges activity from both domains for one account.
- **Intent routing** — a mechanism for expressing cross-domain intents that the network resolves automatically.
- **Light-client verification of Paxeer deposits on LayerX** — a light-client proof that a deposit transaction was included in a Paxeer block, verifiable on the LayerX side without a full Paxeer node.

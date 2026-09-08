# Settlement evidence publication

Publication version 1 uses the existing CheckpointRegistry and LayerXVault addresses. It does not change the guarantor certificate, Ed25519 checkpoint authority, custody finality, withdrawal debit or emergency-exit verification rules. Deployment finalization checks both contracts' `EVIDENCE_VERSION()` equals 1. Existing deployed bytecode must be redeployed through the normal suite deployment; editing source does not upgrade it.

## Authority and ABI

`registerCheckpoint` continues to require a valid guarantor certificate. It records its caller in `checkpointProposer(bytes32)`. Only that caller can publish witnesses for the registered canonical checkpoint, once:

```solidity
publishCheckpointWitnesses(bytes32 checkpointHash, bytes canonicalWithdrawalStateWitnesses, bytes canonicalBalanceWitnesses)
event CheckpointWitnessesPublished(bytes32 indexed checkpointHash, uint16 version, bytes32 witnessesDigest)
```

The selector is `0x69929738`. The digest is SHA256 of `abi.encode(checkpointHash, uint16(1), canonicalWithdrawalStateWitnesses, canonicalBalanceWitnesses)`. The full bytes remain in transaction input. Both byte strings must be nonempty and together at most 1,000,000 bytes. Empty vectors have their tag and zero count. `witnessesPublished` refuses repeated publication independently of digest value; `witnessesDigest` exposes the commitment.

```solidity
registerDepositRoot(bytes canonicalRegistration, bytes ed25519Signature, bytes32[] leafOrdering)
event DepositRootRegistered(bytes32 indexed checkpointId, bytes32 indexed depositRoot, bytes32 registrationDigest, uint16 version)
```

The selector is `0x8ff1fac9`. The vault resolves the existing registry through `guarantorBond.slashingAuthority().registry()`, checks its `guarantorEligibility()` equals the configured bond and requires the recorded checkpoint proposer. The same governance-controlled bond/challenge-manager wiring already used for settlement supplies this path; there is no new address or signing key configuration. The registration must match that registry's canonical checkpoint state root, network and protocol. It requires exactly 64 signature bytes and 1–4096 ordered leaf hashes. The contract pins the bytes; Ed25519 verification remains off-chain.

`registrationDigest = SHA256(abi.encode(uint16(1), canonicalRegistration, ed25519Signature, leafOrdering))`. `depositRegistrationDigest(checkpointId)` stores it; `depositRootRegistered(checkpointId)` refuses duplicates, including replacement signatures or orderings. Leaf ordering is Merkle positional order, not sorted hash order. The consumer rejects duplicate leaf hashes and reconstructs the root and index-aware path with `layerx_proof::merkle`.

## Canonical encodings

Integers below are unsigned big-endian. Tags are exact ASCII bytes, with a terminating zero only where explicitly shown. No trailing bytes are accepted. Publication version 1 is independent of the LayerX protocol version selected for proof validation.

Deposit `canonicalRegistration` is exactly the existing `deposit_root_registration_message` output:

```
"LX:PAXEER:DEPOSIT:ROOT:v1"
checkpoint_id[32] checkpoint_state_root[32] deposit_root[32]
custody_reference[32] network_id:u32 protocol_version:u16
```

The authority signs these exact bytes with Ed25519. Leaf hashes are the real deposit leaf encoding passed through the protocol Merkle leaf hash. The signature is separate ABI calldata, not appended to the signing message.

Both checkpoint witness byte strings use `tag || count:u32 || (item_length:u32 || item_bytes)*`, with at most 4096 entries and at most 1,048,576 bytes per decoded vector:

| Vector tag | Item bytes | Strict entry ordering |
| --- | --- | --- |
| `LXP/Paxeer/withdrawal-witnesses/v1\0` | withdrawal_id[32], withdrawal_leaf_hash[32], canonical checkpoint proof | withdrawal_id |
| `LXP/Paxeer/balance-witnesses/v1\0` | account[32], asset[32], balance:u128, recipient[20], canonical checkpoint proof | (account, asset, recipient) |

The checkpoint proof is the existing `wire::encode_checkpoint_proof_for_protocol` representation: `0x01 0x02`, checkpoint hash[32], state root[32], epoch:u64, batch:u64, data-availability root[32], leaf index:u64, sibling count:u16, siblings[32] each, attestation count:u32, then the existing canonical guarantor attestations. Its decoder applies all existing protocol, depth, index and attestation structural checks. Withdrawal leaves and balance leaves use the existing verifier hash functions, including ABI address padding. Every witness root must equal the registered resulting state root.

`CheckpointProof::encode_publication` and `ExitEvidence::encode_publication` produce these encodings from the workspace's real types and reject mismatched roots and unordered entries. The complete proof carries guarantor attestations; the existing claim verifiers check them against the certificate recorded on-chain.

## Consumer flow

1. Configure the normal `EndpointConfig` with the exact chain ID and TLS trust, or `LocalEmulator` for a loopback Anvil. Supply the existing vault and registry addresses, expected checkpoint identity, protocol version and confirmation depth. No custom RPC method or provider-specific indexer is used.
2. Fetch `CheckpointRegistered` and the appropriate publication event using `eth_getLogs`. Fetch each transaction input using `eth_getTransactionByHash` and its receipt using `eth_getTransactionReceipt`. Require successful execution, matching emitting contract, checkpoint topics, transaction hash/index and block identity. Require the publication caller to equal the checkpoint registration caller.
3. Use `eth_getBlockByNumber` for the head and containing blocks; require the requested confirmation depth, matching canonical block hashes and inclusion at the declared transaction index. `raw_call` also checks `eth_chainId` against endpoint configuration on each request. Refuse absent or ambiguous observations.
4. Strictly decode the ABI, reconstruct it to reject aliasing, padding and trailing calldata, and recompute the complete publication digest. Decode canonical witnesses and bind checkpoint hash, resulting root, epoch, batch and data-availability root to the registration event.
5. For deposits call `PublishedDepositProof::fetch_published` with the actual custody event facts, then pass its result and the separately tracked custody `FinalityReport` to `DepositProofVerifier::obtain`. That existing verifier checks the configured Ed25519 key, network, protocol, custody reference, quorum/finality and custody leaf inclusion.
6. For withdrawals call `CheckpointProof::fetch_published` with the debit expectation, then pass the returned proof and verified `CommittedWithdrawalDebit` to `WithdrawalBoundary::construct_claim`. For exits call `ExitEvidence::fetch_published`, then `EmergencyExit::construct_claim`. These existing boundaries remain mandatory: they verify certificate standing, asset and debit bindings, nullifiers, roots and exit eligibility before settlement. Publication itself is not a payout authorization.

The fetch methods return untrusted evidence and typed endpoint failures. They do not replace the existing quorum and admission APIs. Balance witnesses are available before `executeExit`; the exit event is never used as a pre-settlement proof source.

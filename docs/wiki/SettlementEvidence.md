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

## Native state witness version 2

The native witness codec is independent of the publication envelope and the checkpoint protocol version. All integers are big-endian:

```
version:u16=2 || module_id:u16 || key_len:u32 || key || value_len:u32 || value
|| leaf_index_a:u32 || leaf_count_a:u32 || depth_a:u8 || siblings_a[32]*
|| leaf_count_b:u32 || depth_b:u8 || siblings_b[32]*
```

The leaf hash is SHA256(`LXP/v1/state-leaf\0 || key_len:u32 || value_len:u32 || key || value`). Note that both lengths precede the key in the hash preimage, whereas each length precedes its bytes on the wire. Each path hashes SHA256(`LXP/v1/state-node\0 || left[32] || right[32]`) in native positional order. At odd widths the last node duplicates itself; the proof must carry that exact sibling. Counts, indices, depths and trailing bytes are checked strictly. The layer-B index is `module_id` and its leaf is SHA256(`LXP/v1/state-leaf\0 || u32(2) || u32(32) || module_id:u16 || module_subtree_root[32]`). The current native module count is 9 or 10, including empty module subtrees.

`lxp_state_proof_build` composes the existing native subtree and root constructors and verifies the result before returning it. `lxp_state_proof_encode`, `lxp_state_proof_decode` and `lxp_state_proof_verify` share that representation. Allocate `lxp_state_witness` on the heap: it owns up to one MiB of blob value material. `gp_runtime_state_proof` exposes the same constructor over the guarantor's independently replayed kernel. Rust `state_proof::StateWitness` and Solidity `NativeStateProof` verify the identical bytes. `build/tests/lxp_test_state_proof --vectors` emits the shared fixtures under `contracts/config/native-state-proofs.json` and the paxeer-client test vectors directory.

This generic proof does not yet enable version-2 settlement publication. Native account balances are inside a third account-registry tree under module-zero `account-tree`, with no EVM recipient field, and the withdrawal store is not committed as module KV. A proof of the account-tree root is not a proof of a particular account balance. The standalone asset balance root is not the composite checkpoint root. These gaps must be resolved without inventing settlement leaves or accepting an unsigned recipient binding.

The required rollout sequence remains: independently replay the checkpoint; decode committed settlement facts and build their proofs and deposit leaf ordering; register the checkpoint; publish the withdrawal and balance witness vectors; register the signed deposit root; fetch through `PublishedDepositProof::fetch_published`, `CheckpointProof::fetch_published` and `ExitEvidence::fetch_published`; then apply the existing custody, debit, certificate, nullifier and eligibility checks. The claim consumers and publication contracts still use version 1 until this entire sequence can carry real facts. A coordinated rollout must change both `EVIDENCE_VERSION` constants and the deployment finalization expectation to 2. The generic vectors are not settlement balance or withdrawal vectors.

For module zero and a 33-byte key beginning with `04`, version 2 carries the
canonical account leaf from `lx_account_state_leaf_material`. Immediately after
`value`, the encoding adds `account_index:u32 || account_count:u32 ||
account_depth:u8 || account_siblings[32]*`. Other keys have no account segment.
The account path hashes to the account registry root. That root is the value of
`account-tree` (12 bytes), hashed with the native state-leaf domain and both
lengths. Layer A proves this registry binding in module zero; the unchanged
module wrapper and layer B then prove inclusion under the composite root.
Balances, asset identity and account authority remain bound by the existing
canonical account record; this extension does not change native root semantics.

The native asset request stages an immutable withdrawal fact under
`withdrawal:` (11 ASCII bytes) followed by its existing 32-byte nullifier.
The exact 182-byte value is `version:u16=2 || network_id:u32 ||
withdrawal_id:32 || account_id:32 || asset_id:32 || amount:u128 ||
payout_recipient:32 || checkpoint_id:32`, all integers big-endian.
`lx_withdrawal_state_decode` validates the complete encoding and recomputes
its key from the real request type. Module-context commit persists the fact;
a failed transfer rolls the staged fact back. The runtime withdrawal store
retains its existing lookup and settlement behavior. The fact records request
creation, not subsequent settlement status.

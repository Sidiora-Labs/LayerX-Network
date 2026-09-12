# Settlement evidence publication

Both publication contracts expose `EVIDENCE_VERSION() == 2`; deployment finalization requires 2. Existing deployed contracts require the normal redeployment process. Guarantor checkpoint certificates retain their existing signature scheme and all certificate checks.

## Authority and publication

`registerCheckpoint` records its proposer. Only that proposer can call `publishCheckpointWitnesses(bytes32,bytes,bytes)` once for a canonical checkpoint. Version-2 publications validate native inclusion, signed recipient bindings, the exact recorded certificate, and request-anchor ancestry before recording `SHA256(abi.encode(checkpointHash,uint16(2),withdrawals,balances))`. The event carries version 2. Legacy version-1 publication and proof paths remain available for their existing consumers; envelopes cannot mix versions.

The vault's governance configures `setDepositRootAuthority(bytes32)` with the independently provisioned Ed25519 checkpoint authority public key. The deployment script grants this selector through the existing timelock. Run `authority-schedule <ed25519-public-key>` after permissions, then `authority-execute <ed25519-public-key>` after the recorded delay; the explicit nonzero key and operation nonce are recorded and must match on execution. Finalization refuses an absent or mismatched authority before broadcasting. There is no default authority. An absent key refuses registration. `registerDepositRoot(bytes,bytes,bytes32[])` requires the recorded proposer and canonical root/network/protocol, then verifies the Ed25519 signature over the exact registration bytes. Its digest is `SHA256(abi.encode(uint16(2),registration,signature,leafOrdering))`. Ordering is positional, contains 1–4096 leaves, and consumers reconstruct the deposit root with the existing Merkle codec.

`contracts/crypto/Ed25519.sol` implements SHA-512 and the cofactorless RFC 8032 verification equation with canonical point/scalar decoding and small-order refusals. Constructor-installed verifier helper contracts with no replacement setters keep this code outside the settlement contracts' runtime size. No EVM address is derived from an Ed25519 key.

## Version-2 publication encoding

All integers are unsigned big-endian. Vector framing is `tag || count:u32 || (item_length:u32 || item)*`.

| Tag | Item |
| --- | --- |
| `LXP/Paxeer/withdrawal-witnesses/v2\0` | withdrawal_id32, semantic withdrawal_leaf32, checkpoint proof |
| `LXP/Paxeer/balance-witnesses/v2\0` | account32, asset32, amount:u128, recipient20, checkpoint proof |

Checkpoint proof wire is `0x02 0x02 || inclusion_checkpoint32 || state_root32 || epoch:u64 || batch:u64 || data_availability_root32 || leaf_index:u64=0 || sibling_count:u16=0 || attestation_count:u32 || packed_attestations || native_evidence`. Attestations retain the existing 18 fields and canonical widths.

Native evidence is `request_anchor32 || inclusion_checkpoint32 || network_id:u32 || witness_length:u32 || witness || signature_length:u16 || signature`. Withdrawal signatures are empty because the committed native request already authenticates its recipient. Balance signatures are exactly 64 bytes. All native witnesses must reproduce the registered composite root. An empty balance publication is refused; an empty list cannot substitute for evidence of accounts in replayed state.

Recipient authorization signs `"LX:SETTLE:RECIPIENT:v1" || 0x00 || network_id:u32 || account32 || asset32 || recipient20 || request_anchor32` using the authority committed in the account leaf. The signature, native witness and both checkpoint identities persist through Human exit/withdrawal plans. Version-2 claim ABIs carry the inclusion checkpoint separately while retaining the request anchor in the existing nullifier domain. Both exit paths also consume the account/asset/inclusion balance once, so different valid signed anchors cannot spend that same proven balance twice.

Deposit registration signing bytes remain `"LX:PAXEER:DEPOSIT:ROOT:v1" || checkpoint_id32 || checkpoint_state_root32 || deposit_root32 || custody_reference32 || network_id:u32 || protocol_version:u16`, with no terminating zero in that domain.

## Consumer sequence

1. Independently replay native state and construct account/withdrawal witnesses and actual deposit ordering.
2. Obtain each account's recipient-binding signature and the configured checkpoint authority's deposit registration signature. A guarantor signing key cannot substitute for either authority.
3. Register the inclusion checkpoint with its full guarantor certificate. Require the request anchor to be a recorded canonical ancestor or equal.
4. Publish the version-2 vectors and register the signed deposit root.
5. Fetch through `PublishedDepositProof::fetch_published`, `CheckpointProof::fetch_published` and `ExitEvidence::fetch_published`. These verify transaction/receipt/block consistency, confirmation depth, proposer, canonical ABI, event version, digest and native proof. Endpoint TLS and chain identity checks remain required.
6. Apply the existing custody, committed-debit, certificate, nullifier, recipient and exit-eligibility boundaries before payout. Publication alone is not payout authorization.

The autonomous producer still needs the actual account-signed bindings and checkpoint-authority registration material; native replay cannot manufacture those signatures. Source support and focused contract tests are separate from a qualified end-to-end producer publication run.

## Native state witness version 2

The native witness codec is independent of the publication envelope and the checkpoint protocol version. All integers are big-endian:

```
version:u16=2 || module_id:u16 || key_len:u32 || key || value_len:u32 || value
|| [account_index:u32 || account_count:u32 || account_depth:u8 || account_siblings[32]*]
|| leaf_index_a:u32 || leaf_count_a:u32 || depth_a:u8 || siblings_a[32]*
|| leaf_count_b:u32 || depth_b:u8 || siblings_b[32]*
```

The leaf hash is SHA256(`LXP/v1/state-leaf\0 || key_len:u32 || value_len:u32 || key || value`). Note that both lengths precede the key in the hash preimage, whereas each length precedes its bytes on the wire. Each path hashes SHA256(`LXP/v1/state-node\0 || left[32] || right[32]`) in native positional order. At odd widths the last node duplicates itself; the proof must carry that exact sibling. Counts, indices, depths and trailing bytes are checked strictly. The layer-B index is `module_id` and its leaf is SHA256(`LXP/v1/state-leaf\0 || u32(2) || u32(32) || module_id:u16 || module_subtree_root[32]`). The current native module count is 9 or 10, including empty module subtrees.

`lxp_state_proof_build` composes the existing native subtree and root constructors and verifies the result before returning it. `lxp_state_proof_encode`, `lxp_state_proof_decode` and `lxp_state_proof_verify` share that representation. Allocate `lxp_state_witness` on the heap: it owns up to one MiB of blob value material. `gp_runtime_state_proof` exposes the same constructor over the guarantor's independently replayed kernel. Rust `state_proof::StateWitness` and Solidity `NativeStateProof` verify the identical bytes. `build/tests/lxp_test_state_proof --vectors` emits the shared fixtures under `contracts/config/native-state-proofs.json` and the paxeer-client test vectors directory.

The account segment is present exactly when `module_id == 0`, `key_len == 33`
and the first key byte is `04`. It proves the canonical account record into the
account registry; layer A then proves the `account-tree` binding, and layer B
proves the preserved module wrapper under the composite root. All other leaves
omit that segment. The shared C vectors include three real account balances.

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

The request checkpoint and inclusion checkpoint are separate settlement fields:
`request_anchor` is the record's existing `checkpoint_id` and remains in the
nullifier domain; `inclusion_checkpoint` selects the registered root proving
that record. `CheckpointRegistry::isRecordedAncestor(requestAnchor,
inclusionCheckpoint)` requires both checkpoints to remain canonical and checks
their recorded batch order. Registration already enforces consecutive batches,
sequence continuity and state-root continuity. Unknown, future and invalidated
anchors or inclusion checkpoints are refused. Version-2 consumers carry both fields and enforce this predicate together with `isFinalised` for the inclusion root.

The guarantor exports account witnesses only for asset-bearing agent-main accounts, the balance leaf kind accepted by the native settlement verifier. System reserves and withdrawal custody accounts are not owner exit balances. Withdrawal and custody-credit records are enumerated from their committed module KV keys and independently proved against the replayed checkpoint root. A checkpoint with no owner balance, withdrawal or custody-credit facts produces a `no_native_settlement_facts` evidence inventory; it does not submit an empty balance vector to the registry. Any nonempty settlement publication requires its complete owner balance bindings.

After registration, the producer writes `<checkpoint-id>.publication-request.json` in its state directory. The independently operated account owners and checkpoint authority deliver `<checkpoint-id>.json` into `LAYERX_GUARANTOR_PUBLICATION_INPUTS_DIR`. The file contains `version: 2`, `checkpoint_id`, `recipient_bindings` (account, asset, recipient, request_anchor and the account-owner Ed25519 signature), and `deposit_registration` (vault, custody_reference and the separately configured deposit-root-authority Ed25519 signature), or null exactly when replay contains no deposits. The producer never acquires these authorities' signing keys. Delivery is bounded to thirty seconds per registration attempt; absent or invalid inputs fail closed while leaving the registered checkpoint available for retry.

The producer derives deposit leaves from committed custody credits, sorts them by deposit ID, computes the ordered Merkle root, verifies the registration signature against the vault's configured authority, and verifies every owner signature against its proven account key. It checks recorded checkpoint ancestry before publishing. It submits the withdrawal/balance vectors and deposit registration using the checkpoint proposer, checks the resulting on-chain digests, and atomically records the complete vectors, ordering, signatures and transaction references in `<checkpoint-id>.evidence.json`. Retries compare existing publication digests and refuse conflicting content without resubmitting matching publications.

## Checkpoint ordering contract version 2

`CheckpointRegistry.CHECKPOINT_ORDER_VERSION()` is 2. A rollout must deploy the new registry and its bound settlement suite; finalization refuses a registry with the prior ordering policy and records the ordering version. The native witness encoding and `EVIDENCE_VERSION` remain 2 because their bytes have not changed.

Epoch identifies an authority lifecycle, not a checkpoint counter. Native header production uses `process->kernel.epoch`; `lxp_kernel_set_epoch` permits equality and refuses regression, and `lxp_kernel_epoch_transition` advances the kernel through the module epoch hooks inside one state journal, consuming one global sequence and recomputing the state root. A replica whose sealed header carries a higher epoch runs that transition before it checks the header's `first_sequence` and `previous_state_root` against its kernel. Multiple real batches can therefore share an epoch. Registration requires a nonzero epoch greater than or equal to `finalisedEpoch`, and the checkpoint sequence (`header.batchNumber`) must equal `finalisedBatchNumber + 1`. Activity ranges must also start at `finalisedLastSequence + 1`; state-root continuity, increasing timestamps, signatures, membership, freshness and invalidation checks remain mandatory.

The strict sequence prevents replay even when epochs are equal: a previously accepted batch can never occupy the next sequence, and a skipped batch is refused. Increasing the epoch cannot bypass either sequence check. Native checkpoint-log recovery already enforces non-decreasing epochs and consecutive batches and activity ranges. The former registry already required consecutive sequences. This rollout preserves those replay checks while aligning epoch progression with the native authority lifecycle.

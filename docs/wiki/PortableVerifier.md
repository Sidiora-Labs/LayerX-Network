# Portable receipt verifier

`layerx-portable` exports a self-contained `layerx-receipt-proof-v1`
JSON object and verifies it against an independently trusted
`AuthorizedBatch` with no node, gateway, daemon, database, clock, or
network (`interop/crates/layerx-portable/src/lib.rs:5-8`,
`interop/crates/layerx-portable/src/receipt.rs:112-114`). The wire
identifier is `PORTABLE_RECEIPT_FORMAT`
(`interop/crates/layerx-portable/src/receipt.rs:11`;
`interop/crates/layerx-portable/FORMAT.md:3-4`). The only admitted
`verificationLevel` is the literal `sequencer-signed`
(`interop/crates/layerx-portable/src/receipt.rs:12, 26, 171-172`;
`interop/crates/layerx-portable/FORMAT.md:9`).

This page is that receipt proof and its refusals. It does not document
program CALL terminals; those live in
[SDK terminal verification](SdkTerminalVerification.md).

---

## Inputs

`PortableReceipt::verify` takes the parsed object and a caller-supplied
`AuthorizedBatch` (`interop/crates/layerx-portable/src/receipt.rs:120-123`).
`PortableReceipt::verify_json` takes JSON bytes and the same batch
(`interop/crates/layerx-portable/src/receipt.rs:93-97`).
`from_json` alone establishes no trust
(`interop/crates/layerx-portable/src/receipt.rs:71-72`).

JSON bounds: empty or longer than `MAX_JSON_BYTES` (`1_500_000`) is
refused before parse (`interop/crates/layerx-portable/src/receipt.rs:14, 77-80`).
Canonical receipt bounds on export: empty or longer than
`MAX_RECEIPT_BYTES` (`1_048_576`)
(`interop/crates/layerx-portable/src/receipt.rs:13, 49-51`).

`AuthorizedBatch` is five 32-byte facts: `batch_id`, `asset`,
`previous_state_root`, `resulting_state_root`, `sequencer_public_key`
(`agent/crates/layerx-proof/src/receipt.rs:44-50, 52-69`). Those facts
must come from a snapshot, certificate, or vector manifest the verifier
already trusts (`interop/crates/layerx-portable/FORMAT.md:20-22`;
`interop/crates/layerx-portable/src/receipt.rs:18-21`). Repeating them
on the JSON object does not make them authoritative
(`interop/crates/layerx-portable/src/receipt.rs:18-21`).

The object members (`interop/crates/layerx-portable/src/receipt.rs:22-34`;
`interop/crates/layerx-portable/FORMAT.md:6-16`):

| Member | Role |
| --- | --- |
| `format` | Literal `layerx-receipt-proof-v1` |
| `verificationLevel` | Literal `sequencer-signed` |
| `canonicalReceipt` | Exact canonical receipt bytes, unpadded base64url, length in `1..=1_048_576` |
| `receiptDigest` | Claimed 32-byte receipt-signature digest, unpadded base64url |
| `batchId` | Claimed 32-byte batch id, unpadded base64url |
| `asset` | Claimed 32-byte batch asset, unpadded base64url |
| `previousStateRoot` | Claimed 32-byte predecessor root, unpadded base64url |
| `resultingStateRoot` | Claimed 32-byte successor root, unpadded base64url |
| `sequencerPublicKey` | Claimed 32-byte Ed25519 public key, unpadded base64url |

Unknown JSON members are refused (`serde(deny_unknown_fields)` at
`interop/crates/layerx-portable/src/receipt.rs:23`). Member order is
insignificant (`interop/crates/layerx-portable/src/receipt.rs:100-102`;
`interop/crates/layerx-portable/FORMAT.md:4-5`).

---

## What it verifies

After the wire object decodes, `verify` binds the five claimed batch
fields to `trusted_batch`, then calls `layerx_proof::receipt::verify_outcome`
on the decoded canonical bytes, then requires the recomputed digest to
equal `receiptDigest`
(`interop/crates/layerx-portable/src/receipt.rs:124-146`).

`verify_outcome` (`agent/crates/layerx-proof/src/receipt.rs:246-310`):

1. Decode the canonical receipt; re-encode; refuse a byte difference
   (`agent/crates/layerx-proof/src/receipt.rs:250-256`).
2. Require a full protocol receipt
   (`agent/crates/layerx-proof/src/receipt.rs:257-259`).
3. Require an occupancy protocol version (2 or 3)
   (`agent/crates/layerx-proof/src/receipt.rs:15-16, 260-261`;
   `agent/crates/layerx-wire/src/limits.rs:29-33`).
4. Require a non-zero operation and a non-zero activity id
   (`agent/crates/layerx-proof/src/receipt.rs:263-268`).
5. Require receipt `batch_id`, `asset` (and asset non-zero),
   `previous_state_root`, and `resulting_state_root` to equal the
   authorised batch (`agent/crates/layerx-proof/src/receipt.rs:269-280`).
6. When `result_code == 0`, require
   `debit_balance_before - amount == debit_balance_after` and
   `credit_balance_before + amount == credit_balance_after` with checked
   unsigned arithmetic (`agent/crates/layerx-proof/src/receipt.rs:281-296`).
7. Require a sequencer signature; encode the unsigned receipt; recompute
   `receipt_digest` (`agent/crates/layerx-proof/src/receipt.rs:297-303`).

The digest is SHA-256 of domain tag `LXP/v1/receipt\0` concatenated with
the unsigned canonical receipt
(`agent/crates/layerx-wire/src/hash.rs:69, 126, 282-287`;
`interop/crates/layerx-portable/FORMAT.md:32-35`). The signature scheme
is Ed25519 `verify_strict` over that 32-byte digest with
`sequencer_public_key` (`agent/crates/layerx-crypto/src/ed25519.rs:54-70`;
`agent/crates/layerx-proof/src/receipt.rs:304-305`;
`interop/crates/layerx-portable/FORMAT.md:35`).

`verify_outcome` always stores that digest on
`Evidence::sequencer` (`agent/crates/layerx-proof/src/receipt.rs:309`;
`agent/crates/layerx-proof/src/evidence.rs:19-22`). Portable `verify`
then compares it to the object's `receiptDigest`
(`interop/crates/layerx-portable/src/receipt.rs:135-141`).

Export runs the same `verify_outcome` path before encoding JSON, and
does not rewrite a rejected protocol result into success
(`interop/crates/layerx-portable/src/receipt.rs:38-40, 45-68`).

---

## What it deliberately does not

- Consult a node, gateway, daemon, database, clock, or network
  (`interop/crates/layerx-portable/src/lib.rs:5-8`;
  `interop/crates/layerx-portable/src/receipt.rs:112-114`).
- Treat JSON batch members as a trust root
  (`interop/crates/layerx-portable/src/receipt.rs:18-21`;
  `interop/crates/layerx-portable/FORMAT.md:20-22`).
- Claim batch inclusion, checkpoint finality, or external settlement
  (`interop/crates/layerx-portable/FORMAT.md:37-39`).
- Refuse a rejected protocol `result_code`. The portable path calls
  `verify_outcome`, not `verify`. `verify` additionally refuses
  `result_code != 0` as `ReceiptCheck::ResultCode`
  (`agent/crates/layerx-proof/src/receipt.rs:224-236, 246-248`;
  `interop/crates/layerx-portable/FORMAT.md:37`).
- Impose 402LXP transfer-root or program-terminal reconstruction. That
  is the SDK terminal decoder in
  [SDK terminal verification](SdkTerminalVerification.md).
- Run `verify_program_outcome` / `verify_program_state`
  (`agent/crates/layerx-proof/src/receipt.rs:356-488`). Those entry
  points are not the portable receipt verifier.

`verify_external_evidence` is a separate adapter-binding path for
upstream mandates and receipts (`interop/crates/layerx-portable/src/lib.rs:8-12`;
`interop/crates/layerx-portable/src/external.rs:140-184`). It is not
this verifier.

---

## Ordered steps and first refusal

`verify_json` is `from_json` then `verify`
(`interop/crates/layerx-portable/src/receipt.rs:93-97`).

`from_json` (`interop/crates/layerx-portable/src/receipt.rs:77-84`):

1. `JsonBounds` if `bytes` is empty or longer than `1_500_000`.
2. `JsonShape` if `serde_json::from_slice` fails (wrong type, missing
   members, unknown members).
3. `decode_fields`.

`decode_fields` (`interop/crates/layerx-portable/src/receipt.rs:167-188`):

1. `UnsupportedFormat` if `format != "layerx-receipt-proof-v1"`.
2. `UnsupportedVerificationLevel` if `verificationLevel != "sequencer-signed"`.
3. Decode `canonicalReceipt` (length `1..=1_048_576`), then the six
   32-byte fields in order: `receiptDigest`, `batchId`, `asset`,
   `previousStateRoot`, `resultingStateRoot`, `sequencerPublicKey`.

Each base64 field (`interop/crates/layerx-portable/src/receipt.rs:288-306`):

1. `InvalidBase64` if empty or contains `=`.
2. `InvalidBase64` if URL-safe unpadded decode fails.
3. `InvalidLength` if decoded length is outside the field bound.
4. `InvalidBase64` if re-encoding does not equal the input string.

`verify` after a successful decode
(`interop/crates/layerx-portable/src/receipt.rs:124-146`):

1. `BatchAuthorizationMismatch` if any of the five decoded batch facts
   differs from `trusted_batch`.
2. `Receipt(VerificationFailure { check })` from `verify_outcome`, with
   `ReceiptCheck` order Decode, CanonicalEncoding, ReceiptShape,
   ProtocolVersion, Operation, ActivityId, BatchId, Asset,
   PreviousStateRoot, ResultingStateRoot, DebitBalance, CreditBalance,
   MissingSignature, CanonicalEncoding (unsigned encode / digest),
   SequencerSignature (`agent/crates/layerx-proof/src/receipt.rs:250-305`).
3. `MissingReceiptDigest` if `evidence().receipt_digest()` is `None`.
4. `ReceiptDigestMismatch` if that digest differs from the object's
   `receiptDigest`.

Combined-fault order asserted by tests: a JSON object whose canonical
bytes also fail `Decode`, verified against a non-matching batch, returns
`BatchAuthorizationMismatch` rather than `Receipt(Decode)`
(`interop/crates/layerx-portable/tests/independent_verifier.rs:126-137`).
The same object against the matching batch returns `Receipt(Decode)`
(`interop/crates/layerx-portable/tests/independent_verifier.rs:99-111`).
Wire-shape faults (`from_json`) fire before `verify` and therefore
before batch mismatch.

`export` (`interop/crates/layerx-portable/src/receipt.rs:45-68`):

1. `ReceiptBounds` if the canonical slice is empty or longer than
   `1_048_576`.
2. `Receipt(...)` from `verify_outcome`.
3. `MissingReceiptDigest` if the verified evidence has no digest.

---

## Typed refusals

`PortableReceiptError`
(`interop/crates/layerx-portable/src/receipt.rs:232-244`):

| Variant | Condition |
| --- | --- |
| `JsonBounds` | JSON bytes empty or `len > 1_500_000` (`receipt.rs:14, 78-80`) |
| `JsonShape` | `serde_json` parse or serialize failure, including unknown members (`receipt.rs:23, 81-82, 109`) |
| `ReceiptBounds` | Canonical receipt empty or `len > 1_048_576` on `export` (`receipt.rs:13, 49-51`) |
| `UnsupportedFormat` | `format` is not `layerx-receipt-proof-v1` (`receipt.rs:11, 168-169`) |
| `UnsupportedVerificationLevel` | `verificationLevel` is not `sequencer-signed` (`receipt.rs:12, 171-172`) |
| `InvalidBase64(field)` | Empty, padded (`=`), undecodable, or non-canonical unpadded base64url (`receipt.rs:294-305`) |
| `InvalidLength(field)` | Decoded length outside the field bound (`receipt.rs:300-301, 282-285`) |
| `Receipt(VerificationFailure)` | `verify_outcome` refused; `check` names the stage (`receipt.rs:53, 133-134, 240`) |
| `BatchAuthorizationMismatch` | Object `batchId` / `asset` / `previousStateRoot` / `resultingStateRoot` / `sequencerPublicKey` differ from `trusted_batch` (`receipt.rs:125-131`) |
| `MissingReceiptDigest` | `evidence().receipt_digest()` is `None` after `verify_outcome` (`receipt.rs:54-57, 135-138`) |
| `ReceiptDigestMismatch` | Recomputed digest ≠ object `receiptDigest` (`receipt.rs:139-141`) |

`ReceiptCheck` values `verify_outcome` can return
(`agent/crates/layerx-proof/src/receipt.rs:99-132, 250-305`):

| `check` | Condition |
| --- | --- |
| `Decode` | Canonical decode failed |
| `CanonicalEncoding` | Re-encode ≠ supplied bytes, or unsigned encode / digest failed |
| `ReceiptShape` | Not a full protocol receipt |
| `ProtocolVersion` | Version is not occupancy 2 or 3 |
| `Operation` | Operation tag is 0 |
| `ActivityId` | Activity id is 32 zero bytes |
| `BatchId` | Receipt batch id ≠ authorised batch id |
| `Asset` | Receipt asset ≠ authorised asset, or asset is 32 zero bytes |
| `PreviousStateRoot` | Receipt predecessor ≠ authorised predecessor |
| `ResultingStateRoot` | Receipt successor ≠ authorised successor |
| `DebitBalance` | `result_code == 0` and debit did not decrease by `amount` |
| `CreditBalance` | `result_code == 0` and credit did not increase by `amount` |
| `MissingSignature` | Sequencer signature absent |
| `SequencerSignature` | Ed25519 `verify_strict` failed under the authorised key |

`ReceiptCheck::ResultCode` is raised only by `verify`, not by
`verify_outcome` (`agent/crates/layerx-proof/src/receipt.rs:224-236`).
`ReceiptCheck::Module` is raised only by `verify_program_state` /
`verify_program_outcome` (`agent/crates/layerx-proof/src/receipt.rs:380-388, 446-454`).

`Evidence::sequencer` always sets `receipt_digest: Some(...)`
(`agent/crates/layerx-proof/src/evidence.rs:19-22`). Portable code still
maps `None` to `MissingReceiptDigest`
(`interop/crates/layerx-portable/src/receipt.rs:54-57, 135-138`).

---

## Vectors

`interop/crates/layerx-portable/tests/receipt_vectors.rs` and
`interop/crates/layerx-portable/tests/independent_verifier.rs` do not
load files under `platform/sdk/conformance/fixtures/`. Their vectors are
inline JSON constants. SDK terminal tests do load those fixtures; see
[SDK terminal verification](SdkTerminalVerification.md).

Shared inline object `GOLDEN_RECEIPT_JSON` /
`GOLDEN_VECTOR_1` (`interop/crates/layerx-portable/tests/receipt_vectors.rs:11-21`;
`interop/crates/layerx-portable/tests/independent_verifier.rs:14-24`):
`format` `layerx-receipt-proof-v1`, `verificationLevel` `sequencer-signed`,
batch facts `[1;32]`, `[2;32]`, `[3;32]`, `[4;32]`, `[5;32]`, dummy
`canonicalReceipt` bytes. `GOLDEN_VECTOR_2`
(`interop/crates/layerx-portable/tests/independent_verifier.rs:26-36`)
uses a different dummy receipt and a `previousStateRoot` string that is
not `[8;32]`.

| Test | File | What it proves | Expected outcome |
| --- | --- | --- | --- |
| `format_constant_matches_specification` | `tests/receipt_vectors.rs:24-30` | Format and crate anchor strings | `PORTABLE_RECEIPT_FORMAT == "layerx-receipt-proof-v1"`; `interop_portable_verification() == "layerx-receipt-proof-v1-and-pinned-external-evidence-v1"` |
| `parse_golden_receipt_json` | `tests/receipt_vectors.rs:33-41` | Golden JSON parses | `from_json` `Ok`; `format()` equals `PORTABLE_RECEIPT_FORMAT` |
| `reject_malformed_json` | `tests/receipt_vectors.rs:44-58` | Empty, `{}`, `[]`, unknown field | `from_json` `Err` (variant not asserted) |
| `reject_padded_base64` | `tests/receipt_vectors.rs:61-68` | Insert `=` into a base64 field | `Err(InvalidBase64(_))` |
| `reject_non_canonical_base64` | `tests/receipt_vectors.rs:71-85` | Mutated last character of `receiptDigest` and `resultingStateRoot` (`Q` → `R`) | `Err(InvalidBase64(_))` |
| `reject_wrong_field_lengths` | `tests/receipt_vectors.rs:88-98` | `receiptDigest` shortened to `BAQE` | `Err(InvalidLength(_))` |
| `reject_unsupported_format` | `tests/receipt_vectors.rs:101-111` | `format` set to `layerx-receipt-proof-v2` | `Err(UnsupportedFormat)` |
| `reject_unsupported_verification_level` | `tests/receipt_vectors.rs:114-124` | `verificationLevel` set to `checkpoint-finalized` | `Err(UnsupportedVerificationLevel)` |
| `verify_requires_matching_batch_authorization` | `tests/receipt_vectors.rs:127-142` | Parsed golden vs `AuthorizedBatch` whose `batch_id` is `[99;32]` | `Err(BatchAuthorizationMismatch)` |
| `export_rejects_truncated_canonical_receipt` | `tests/receipt_vectors.rs:145-159` | 32-byte dummy export | `Err(Receipt(VerificationFailure { check: Decode }))` |
| `export_and_json_roundtrip` | `tests/receipt_vectors.rs:162-188` | `from_json` → `to_json` → `from_json` | `format`, `canonical_receipt`, `receipt_digest` equal |
| `reject_oversized_json` | `tests/receipt_vectors.rs:191-204` | JSON longer than `1_500_000` | `Err(JsonBounds)` |
| `reject_oversized_canonical_receipt` | `tests/receipt_vectors.rs:207-215` | Export `1_048_577` bytes | `Err(ReceiptBounds)` |
| `reject_empty_canonical_receipt` | `tests/receipt_vectors.rs:218-225` | Export empty slice | `Err(ReceiptBounds)` |
| `independent_verifier_rejects_invalid_receipt_in_golden_vector_1` | `tests/independent_verifier.rs:99-111` | Vector 1 vs matching `[1]..[5]` batch | `Err(Receipt(Decode))` |
| `independent_verifier_rejects_root_mismatch_in_golden_vector_2` | `tests/independent_verifier.rs:113-124` | Vector 2 vs `[6]..[10]` batch | `Err(BatchAuthorizationMismatch)` |
| `independent_verifier_rejects_batch_mismatch` | `tests/independent_verifier.rs:126-137` | Vector 1 vs `[99;32]`×5 (invalid receipt and mismatched batch) | `Err(BatchAuthorizationMismatch)` |
| `independent_verifier_processes_all_vectors` | `tests/independent_verifier.rs:139-154` | Both golden vectors against their listed batches | `[Err(Receipt(Decode)), Err(BatchAuthorizationMismatch)]` |
| `independent_verifier_no_layerx_infrastructure_required` | `tests/independent_verifier.rs:156-169` | Same as vector 1 matching batch | `Err(Receipt(Decode))` |
| `portable_format_constant_is_stable` | `tests/independent_verifier.rs:171-177` | Format string | `"layerx-receipt-proof-v1"` |
| `independent_implementation_can_enumerate_vectors` | `tests/independent_verifier.rs:179-195` | Two non-empty vectors contain the format id | length 2; each contains `layerx-receipt-proof-v1` |

No portable test returns `Ok(PortableVerifiedReceipt)`. No portable test
loads `platform/sdk/conformance/fixtures/`.

---

## Relationship to SDK terminal verification

[SDK terminal verification](SdkTerminalVerification.md) is the Programs
CALL terminal decoder that runs after a protocol receipt is already
bound. Shared SDK tests call `verify_receipt_outcome` on protocol 3
before decoding `terminal_payload`
(`docs/wiki/SdkTerminalVerification.md:17-19, 50-62`). Both surfaces
recompute the sequencer digest over domain `LXP/v1/receipt\0`
(`docs/wiki/SdkTerminalVerification.md:57-58`;
`agent/crates/layerx-wire/src/hash.rs:69`;
`interop/crates/layerx-portable/FORMAT.md:32-35`).

The portable verifier stops at sequencer-signed receipt authenticity
plus independently supplied batch facts. It does not unwrap
`terminal_payload`, reconstruct applied legs, or classify
`transfer_verification`. The SDK decoder does not implement
`layerx-receipt-proof-v1` JSON.

---

## Disagreements left intact

1. `FORMAT.md` step 2 requires protocol version 1
   (`interop/crates/layerx-portable/FORMAT.md:26-27`).
   `verify_outcome` refuses unless `protocol_version_uses_occupancy`
   (versions 2 and 3) (`agent/crates/layerx-proof/src/receipt.rs:15-16, 260-261`;
   `agent/crates/layerx-wire/src/limits.rs:29-33`).
2. `FORMAT.md` decodes and re-encodes the receipt, then compares batch
   fields (`interop/crates/layerx-portable/FORMAT.md:24-29`).
   `PortableReceipt::verify` compares object batch fields to
   `trusted_batch` before `verify_outcome`
   (`interop/crates/layerx-portable/src/receipt.rs:125-134`). Tests assert
   that combined invalid-receipt plus batch-mismatch yields
   `BatchAuthorizationMismatch`
   (`interop/crates/layerx-portable/tests/independent_verifier.rs:126-137`).
3. `FORMAT.md` names golden vectors in `tests/receipt_vectors.rs` and
   `tests/independent_verifier.rs` as the portability proof
   (`interop/crates/layerx-portable/FORMAT.md:72-74`). Those tests use
   inline JSON, not `platform/sdk/conformance/fixtures/`.
4. `FORMAT.md` step 6 requires `receiptDigest` equality, then Ed25519
   (`interop/crates/layerx-portable/FORMAT.md:34-35`). `verify_outcome`
   verifies Ed25519 first (`agent/crates/layerx-proof/src/receipt.rs:304-305`);
   portable `verify` compares `receiptDigest` only after that returns
   (`interop/crates/layerx-portable/src/receipt.rs:133-141`).

Sources:

- `interop/crates/layerx-portable/src/lib.rs:5-23`
- `interop/crates/layerx-portable/src/receipt.rs:11-14, 18-34, 38-40, 45-146, 167-188, 232-244, 288-306`
- `interop/crates/layerx-portable/FORMAT.md:3-39, 72-74`
- `interop/crates/layerx-portable/tests/receipt_vectors.rs:11-225`
- `interop/crates/layerx-portable/tests/independent_verifier.rs:14-195`
- `agent/crates/layerx-proof/src/receipt.rs:15-16, 44-69, 99-132, 224-236, 246-310`
- `agent/crates/layerx-proof/src/evidence.rs:19-22, 82-83`
- `agent/crates/layerx-wire/src/hash.rs:69, 126, 282-287`
- `agent/crates/layerx-wire/src/limits.rs:29-33`
- `agent/crates/layerx-crypto/src/ed25519.rs:54-70`
- `docs/wiki/SdkTerminalVerification.md`

[Home](Home.md)

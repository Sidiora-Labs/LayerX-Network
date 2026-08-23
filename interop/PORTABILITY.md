# LayerX Portable Verification Guide

This guide explains how to verify LayerX receipts and external protocol evidence without LayerX infrastructure.

## Overview

The LayerX portable verification system supports two directions of evidence flow:

1. **Export**: LayerX receipts exported for verification by external parties
2. **Import**: External protocol receipts and mandates verified by LayerX through adapters

Both directions maintain cryptographic rigor, exact provenance binding, and independence from live infrastructure.

## Portable Receipt Export

### Purpose

External parties can verify LayerX receipts without:
- A LayerX gateway or node
- The layerxd daemon
- A database connection
- Network connectivity
- Wall-clock time

### Format: `layerx-receipt-proof-v1`

The portable receipt is a UTF-8 JSON object with these exact camelCase members (in any JSON member order):

| Member | Type | Description |
|--------|------|-------------|
| `format` | string | Literal `"layerx-receipt-proof-v1"` |
| `verificationLevel` | string | Literal `"sequencer-signed"` |
| `canonicalReceipt` | string | Complete canonical LayerX receipt (unpadded base64url) |
| `receiptDigest` | string | 32-byte receipt-signature digest (unpadded base64url) |
| `batchId` | string | 32-byte authorized batch ID (unpadded base64url) |
| `asset` | string | 32-byte batch asset (unpadded base64url) |
| `previousStateRoot` | string | 32-byte predecessor state root (unpadded base64url) |
| `resultingStateRoot` | string | 32-byte successor state root (unpadded base64url) |
| `sequencerPublicKey` | string | 32-byte Ed25519 public key (unpadded base64url) |

### Verification Algorithm

An independent verifier must:

1. **Parse and validate the JSON object**
   - Reject unknown members
   - Reject padded or non-canonical base64url
   - Reject wrong fixed-field lengths
   - Reject canonical receipts above 1,048,576 bytes

2. **Obtain trusted batch authorization independently**
   - From a certificate
   - From a snapshot
   - From a test vector manifest
   - The JSON object itself is NOT the trust root

3. **Decode and re-encode the canonical receipt**
   - Must produce byte-identical output
   - Protocol version 1 required
   - Non-zero operation and activity IDs required
   - Sequencer signature required

4. **Verify batch field consistency**
   - All claimed batch fields must equal the independently trusted authorization

5. **Verify balance invariants** (for successful results)
   - Debit decreases by exactly the receipt amount (checked unsigned arithmetic)
   - Credit increases by exactly the receipt amount (checked unsigned arithmetic)

6. **Compute and verify the receipt digest**
   - Encode receipt without sequencer signature
   - Compute `SHA-256("LXP/v1/receipt\0" || unsigned_canonical_receipt)`
   - Require digest equals `receiptDigest`

7. **Verify the Ed25519 signature**
   - Signature over the 32-byte digest
   - Public key is `sequencerPublicKey`
   - Strict signature verification required

### What This Format Proves

A verified portable receipt proves:
- The sequencer signed this exact receipt
- State roots chain correctly
- Balance invariants hold
- The receipt is cryptographically valid

It does **not** claim:
- Batch inclusion in a checkpoint
- Checkpoint finality
- External settlement
- Network consensus

Those properties require additional proofs beyond this format.

### Example

```json
{
  "format": "layerx-receipt-proof-v1",
  "verificationLevel": "sequencer-signed",
  "canonicalReceipt": "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgIBAAAAAAAACgAAAAAAAAAFAAAAAAAAAMgAAAAAAAABAAAAAAAAAAEAAAAAAAAAAQAAAAAAAAABAAAAAAAAAGQAAAAAAAAAZAAAAAAAAAABZAAAAAAAAAFkAAAAAAAAAQAAAAAAAABlAAAAAAAAAGQAAAAAAAAAAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE",
  "receiptDigest": "BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ",
  "batchId": "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE",
  "asset": "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI",
  "previousStateRoot": "AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM",
  "resultingStateRoot": "BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ",
  "sequencerPublicKey": "BQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQU"
}
```

### Implementation Example

See `interop/crates/layerx-portable/tests/independent_verifier.rs` for a complete standalone verifier that processes golden vectors with no LayerX infrastructure.

## External Evidence Import

### Purpose

LayerX verifies external protocol evidence (x402 payments, AP2 mandates, UCP orders, etc.) through version-pinned adapters that:
- Perform protocol-specific cryptographic verification
- Enforce exact spec version and conformance suite bindings
- Return typed verified values bound to exact input bytes
- Preserve verification provenance for audit

### Architecture

```
External Presentation (untrusted)
    ↓
ExternalEvidenceVerifier (adapter)
    ↓
Protocol-Specific Verification
    ↓
VerifiedExternalEvidence (typed + bound)
```

### External Presentation

The `ExternalPresentation` struct carries:
- `adapter`: Adapter ID (e.g., `"x402-v2"`)
- `protocol`: Protocol ID (e.g., `"x402"`)
- `spec_version`: Exact pinned version (e.g., `"2.0.3"`)
- `kind`: `ExternalEvidenceKind::Mandate` or `ExternalEvidenceKind::Receipt`
- `media_type`: IANA media type (e.g., `"application/vnd.x402.payment+jose"`)
- `payload`: Raw untrusted bytes (borrowed, never retained)

Validation rules:
- Adapter and protocol IDs must follow LayerX identifier rules
- Spec version must be exactly pinned (no wildcards or ranges)
- Media type must not contain control characters
- Payload bounds: 1 byte to 2,097,152 bytes
- Empty payloads rejected
- Oversized payloads rejected

### Adapter Contract

An `ExternalEvidenceVerifier` implementation must:

1. **Declare exact descriptor**
   - Adapter ID
   - Protocol ID and spec version
   - Spec document digest (SHA-256 of canonical spec text)
   - Conformance suite (name, vector count, suite digest)

2. **Declare evidence kind**
   - `Mandate` or `Receipt`
   - A verifier accepts exactly one kind

3. **Declare media type**
   - The exact IANA media type accepted
   - Must match presentation media type exactly

4. **Perform protocol-specific verification**
   - Signature verification
   - Delegation chain validation
   - Time bounds and nonce checks
   - Audience and constraint enforcement
   - Return typed verified value only after all checks pass

### Verified External Evidence

After successful verification, `VerifiedExternalEvidence<T>` binds:
- The adapter's protocol-specific verified value (type `T`)
- Adapter ID, protocol ID, spec version
- Spec document digest
- Conformance suite name, vector count, suite digest
- Evidence kind and media type
- **Evidence digest**: domain-separated SHA-256 hash of all inputs

The evidence digest is computed as:
```
SHA-256(
    "LayerX/interop/external-evidence/v1\0" ||
    kind_tag ||
    len(adapter) || adapter ||
    len(protocol) || protocol ||
    len(spec_version) || spec_version ||
    spec_document_digest ||
    len(conformance_suite) || conformance_suite ||
    conformance_vector_count ||
    conformance_suite_digest ||
    len(media_type) || media_type ||
    len(payload) || payload
)
```

This digest:
- Changes with any input byte or metadata field
- Is safe to store and share (no secret exposure)
- Provides non-repudiation for audit
- Avoids retaining possibly secret-bearing raw mandates

### Example: x402 Payment Receipt

```rust
use layerx_portable::{verify_external_evidence, ExternalPresentation, ExternalEvidenceKind};

// 1. Receive untrusted x402 payment receipt
let x402_payload = receive_payment_receipt();

// 2. Create presentation with exact metadata
let presentation = ExternalPresentation::new(
    "x402-v2",
    "x402",
    "2.0.3",
    ExternalEvidenceKind::Receipt,
    "application/vnd.x402.payment-response+jose",
    &x402_payload,
)?;

// 3. Verify through pinned x402 adapter
let x402_verifier = get_x402_verifier();
let verified = verify_external_evidence(
    &x402_verifier,
    &presentation,
    &payment_context,
)?;

// 4. Extract typed x402 receipt value
let x402_receipt = verified.into_verified();

// 5. Store only the evidence digest (not raw payload)
store_evidence_digest(verified.evidence_digest());
```

### Supported Protocols

Each adapter implements full upstream conformance:

| Protocol | Adapter | Evidence Types | Conformance |
|----------|---------|----------------|-------------|
| x402 v2 | `x402-v2` | Mandate, Receipt | x402 reference vectors |
| AP2 | `ap2` | Mandate pair | AP2 SD-JWT suite |
| UCP | `ucp` | Order, Receipt | UCP profile vectors |
| Visa Tap | `visa-tap` | Mandate | Visa conformance suite |

See each adapter's `COMPATIBILITY.md` for transport matrix and vector coverage.

## Testing and Portability Proof

### Golden Vectors

Test vectors are located in:
- `interop/crates/layerx-portable/tests/receipt_vectors.rs` — LayerX receipt export/verification
- `interop/crates/layerx-portable/tests/external_verification.rs` — External evidence import
- `interop/crates/layerx-portable/tests/independent_verifier.rs` — Standalone verifier harness

### Running Tests

```bash
# Run all portable verification tests
make interop-test-mandates

# Run only receipt vectors
cargo test --manifest-path interop/Cargo.toml -p layerx-portable --test receipt_vectors

# Run only external evidence tests  
cargo test --manifest-path interop/Cargo.toml -p layerx-portable --test external_verification

# Run independent verifier proof
cargo test --manifest-path interop/Cargo.toml -p layerx-portable --test independent_verifier
```

### Portability Proof

The `independent_verifier.rs` test proves portability by implementing a verifier that:
1. Has no LayerX infrastructure dependencies
2. Verifies golden vectors using only the format specification
3. Requires only a trusted `AuthorizedBatch` from an independent source
4. Processes receipts through `PortableReceipt` API alone

This proves external parties can verify LayerX receipts without running LayerX software.

## Security Considerations

### Trust Boundaries

**LayerX Receipt Export:**
- The JSON object itself is NOT a trust root
- The verifier MUST obtain `AuthorizedBatch` from an independent trusted source
- A valid signature proves the sequencer signed this receipt
- It does NOT prove the batch was finalized or settled

**External Evidence Import:**
- Adapters trust only the pinned upstream specification
- Verification is cryptographic and deterministic
- The evidence digest is safe to store (no secret exposure)
- Raw mandate payloads may contain payment instruments — handle accordingly

### What Verification Does NOT Guarantee

Neither portable receipts nor external evidence verification prove:
- Liveness or availability of upstream systems
- Settlement finality on external networks
- Absence of equivocation (requires additional proofs)
- Freedom from rollback (requires checkpoint finality)

Always consult the specific verification level and proof scope.

## References

- **Format Specification**: `interop/crates/layerx-portable/FORMAT.md`
- **API Documentation**: `interop/crates/layerx-portable/src/lib.rs`
- **Golden Vectors**: `interop/crates/layerx-portable/tests/`
- **x402 Compatibility**: `interop/crates/layerx-x402/COMPATIBILITY.md`
- **Proof Library**: `agent/crates/layerx-proof/`

## Version History

| Version | Date | Changes |
|---------|------|---------|
| 1.0 | 2026 | Initial `layerx-receipt-proof-v1` format and external evidence binding |

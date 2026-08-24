# LayerX SDK Conformance Suite

This conformance suite validates production-grade hardening across the generated LayerX SDKs,
including Go, Java/Kotlin, Swift, and .NET plus the Agent SDK implementations.

## Test Coverage

### Secret Hygiene (`secret-hygiene.test.*`)

Proves that SDKs enforce secret hygiene by construction:

- **SecretBytes**: Key material and session tokens are never logged, never serialized into errors or JSON output, and zeroized where the language permits
- **IdempotencyKey**: Construction validation with no key material leakage through error serialization
- **ProtocolAmount**: Integer-only money representation with floating-point amounts structurally impossible
- **Error Hygiene**: Error messages contain only safe machine codes, never request details or session tokens

Required by: req.24.8 (secret hygiene)

### Streaming Resumability (`streaming-resumability.test.*`)

Proves that SDKs implement resumable streaming with stable cursors and no-gap-no-duplicate reconnection semantics:

- **StreamCursor**: Bounded opaque cursor validation
- **ResumableStream**: No-gap-no-duplicate event chain validation
- **Cursor Chain Integrity**: Refuses gaps, duplicates, and mismatched cursors
- **Reconnection**: Supports resumption after disconnection with stable cursor advancement

Required by: req.24.9 (resumable streaming)

### Operations Coverage (`operations.json`)

Schema-driven validation that every agent-api and human-api operation is covered by all SDKs:

- Complete operation enumeration from both schemas
- Idempotency enforcement on mutations
- Typed error taxonomy with stable machine codes
- Retriability classification (never, safe, after, unknown-outcome)

Required by: req.24.1, req.24.3 (operation coverage, error taxonomy)

## Running the Tests

The conformance suite is executed as part of the platform test target:

```bash
make platform-test-sdks
```

Individual SDK test suites:

```bash
# TypeScript
cd agent/sdk/typescript && npm test

# Python
cd agent/sdk/python && pytest

# Rust
cd agent/crates/layerx-sdk && cargo test

# JVM (JUnit, schema goldens, typed errors, streams, secrets, and local verification)
sh platform/sdk/conformance/run-jvm.sh
```

## Conformance Requirements

Every published SDK must:

1. **Secret Hygiene**: Pass all `secret-hygiene.test.*` tests proving no key material or session tokens in logs, errors, or serialized output
2. **Resumable Streaming**: Pass all `streaming-resumability.test.*` tests proving stable cursors and no-gap-no-duplicate semantics
3. **Integer-Only Money**: Enforce `ProtocolAmount` validation rejecting floating-point representation
4. **Required Idempotency Keys**: Enforce idempotency key presence on mutations
5. **Local Verification**: Ship receipt, batch-inclusion, and checkpoint verification paths requiring no trust in hosted surfaces

## Adding New Conformance Tests

When adding a new conformance requirement:

1. Write the test in all three languages (`.test.ts`, `.test.py`, `.test.rs`)
2. Ensure identical semantics across languages
3. Update this README with the new test coverage
4. Reference the requirement ID from `spec/layerx-platform/spec.kvx`

## Language-Specific Notes

### TypeScript

- Secret zeroization uses `Uint8Array.fill(0)`
- Branded types for compile-time safety (`SecretBytes`, `IdempotencyKey`, `ProtocolAmount`)
- All verification functions are async

### Python

- Secret zeroization on `__del__` with explicit `bytearray` zeroing
- `SecretBytes.__reduce__` raises `TypeError` to prevent pickle serialization
- Dataclasses for structured types

### Rust

- Secret zeroization via `zeroize` crate on `Drop`
- Newtype wrappers for type safety
- `#[must_use]` attributes on verification functions
- No `Clone` on `SecretBytes` to prevent accidental copying

### JVM

- Java-first schema operation/request/response/event types with Kotlin overloads in one Maven coordinate
- `BigInteger` protocol amounts encoded only as canonical decimal strings
- Virtual-thread streaming that fetches only under downstream demand and advances cursors atomically
- Local receipt, batch-inclusion, Merkle, and checkpoint verification with built-in signature verification

# SDK generation

One contract, three SDKs, no dialects. Reference for `[req.18]` and task group 22.

---

## 1. Why generation rather than three implementations

Three hand-written SDKs become three subtly different products. The differences
are never in the obvious places; they are in the ones that matter here:

- one SDK exposes `verified: boolean` where the others expose the level lattice;
- one maps `Unknown` onto an exception, so callers write `catch` and move on;
- one represents an amount as a JavaScript `number` and silently loses precision
  above 2^53;
- one retries internally with a fresh idempotency key.

Each of those is a correctness failure that looks like a style difference in
review. Generation from a single contract schema removes the opportunity.

---

## 2. The pipeline

```text
agent/schema/agent-api/v1.kvx        (the contract; source of truth)
            │
            ├──► layerx-agent-api::generated   (Rust contract types)
            │            │
            │            └──► layerx-sdk        (authored Rust SDK over it)
            │
            └──► agent/tools/sdk-gen
                        ├──► agent/sdk/typescript/src/generated/
                        └──► agent/sdk/python/layerx_sdk/generated/
```

The Rust SDK is **authored** over generated contract types, because the Rust
surface is where ergonomics and the type-level guarantees (`Verified<T>`,
`Projection<T>`, distinct signed/unsigned envelopes) do real work. The TypeScript
and Python SDKs are **generated**, because their job is faithful exposure of the
same contract, not independent design.

---

## 3. The drift gate

CI regenerates and diffs. A difference between committed generated output and a
fresh generation fails the build, and so does a hand-edit to a generated file.

```text
make agent-test-sdk-generate
  ├── regenerate into a temporary tree
  ├── diff against committed output
  └── fail on any difference, naming the file and the hunk
```

Generation is deterministic: stable ordering, no timestamps, no host paths, no
locale-dependent formatting. A generator that emits a different byte sequence on
two machines cannot support a drift gate at all.

---

## 4. What every SDK must preserve

| Property | Requirement in every language |
|---|---|
| Verification levels | The full lattice, ordered, on every read result. Never a boolean. |
| `Unknown` | A first-class submission state, not an exception and not a failure. |
| Idempotency | Caller-supplied keys on every mutating call; repeat returns the original result; differing body returns a conflict. |
| Result codes | Exact numeric protocol codes preserved, unknown codes carried verbatim, terminal vs retriable from the protocol taxonomy. |
| Exact integers | Consensus-critical values use an exact integer representation; the SDK fails rather than losing precision. |
| Projections | Structurally distinct from verified values; never returned in a verified field. |
| No key export | Signing goes through an external-signer interface; no SDK path exports key material. |
| Freshness | Head, latest batch and latest checkpoint accompany freshness-sensitive reads. |

### Exact integers, concretely

| Language | Representation | Not permitted |
|---|---|---|
| Rust | `u128` / fixed-width types | `f64`, lossy `as` casts |
| TypeScript | `bigint`, serialised as a decimal string on the wire | `number` for any amount, sequence or fee |
| Python | `int` (arbitrary precision), validated against the protocol bound | `float`, `Decimal` for consensus values |

A boundary case that silently rounds is a payment that silently differs.

---

## 5. Parity suite

Identical scenarios run through all three SDKs against the same daemon and the
same node, asserting identical observable behaviour:

| Scenario | Asserts |
|---|---|
| Payment with receipt verification | same receipt, same verification level |
| Retry after transport loss | one economic effect, `Unknown` then `Executed` |
| Terminal protocol rejection | same result code, no retry, same error class |
| Proven balance read | same level achieved, same refusal when unachievable |
| Availability failure | same failure classification and evidence |
| Subscription gap | same gap notification and cursor behaviour |
| Policy refusal | same error class and explanation shape |

Divergence is reported with the scenario, the language and both observed
behaviours, and fails the build.

---

## 6. Compatibility

Published as a matrix, verified in CI against a real node over the supported
range:

```text
SDK version  ×  contract version  ×  node interface version  ×  daemon version
```

Rules:

- Additive change only within a contract major version; a breaking change without
  a major increment fails CI.
- An SDK refuses to operate against a contract major version it was not generated
  for, naming both versions.
- The matrix, the examples and the guarantee documentation regenerate with the
  SDKs; a release check fails if they are stale.

---

## 7. Examples

Every SDK ships the same runnable example set, so behaviour can be compared
directly rather than described:

1. **Payment with verification** — pay an agent, wait for a verified receipt,
   check the level reached.
2. **HTTP 402 style settlement** — receive a payment requirement, settle it, hand
   the receipt back to the service.
3. **Budget-constrained delegated spending** — provision a protocol budget and a
   narrower daemon capability, spend under both, hit a ceiling and read the
   refusal.
4. **Service lifecycle** — a task commitment, a delivery and an acceptance, with
   the value movement executing as a separate 402LXP transfer.
5. **Offline verification** — a counterparty verifies a receipt from bytes alone
   with `layerx-proof`, with no daemon, no node and no network.

---

## 8. Documentation honesty

Generated documentation states, per restriction, whether it is protocol-enforced
or daemon-enforced. A documentation check fails the build when generated text
describes a daemon-enforced restriction as a protocol guarantee.

This is not pedantry about wording. An operator who believes a counterparty
allowlist is protocol-enforced will size their exposure to a compromised daemon
incorrectly, and the SDK documentation is where that belief usually forms.

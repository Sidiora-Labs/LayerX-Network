# Receipt authority and the human agent contract

The binary dispatches all eight `/v1/agent/*` routes with a dedicated scoped
credential and protected policy. Registry, authorized-batch and balance-context have successful
responses. Core-clock succeeds with two distinct retained verified headers.
Identity, capability-scope, budget-state and key-policy refuse
when the sources described below cannot establish the requested facts. These
refusals do not constitute complete successful-route coverage.

Sources: `agent/crates/layerx-agentd/src/human_runtime.rs`, especially
`RemoteHumanAuthority`, its `HumanAuthorityBoundary` implementation,
`CoreLeaseAttestation::map`, and the decoding helpers; registry constraints come
from `agent/crates/layerx-types/src/payload.rs` and `limits.rs`.

## Transport, encoding, and errors

Every request is GET over HTTPS with `Authorization: Bearer <token>`. Every route
has required `tenant` and `principal` query parameters. Strings are UTF-8 bytes
percent-encoded except ASCII alphanumeric and `-_.~`; spaces are `%20`, not `+`.
The client sends no body. Route-specific parameters are listed below.

The connection constructor requires an `https://` endpoint, a token of at least
32 bytes, a nonzero deadline, and a response limit in `1..=1_048_576` bytes.
It removes trailing slashes from the endpoint. The limit applies to the entire
JSON response, including all field names and hex text. The authority's existing
HTTP parser separately limits incoming headers to 16 KiB, requires HTTP/1.1 and
Host, and refuses bodies, duplicate headers, and transfer encoding.

In the tables, `H32` means a JSON string of exactly 64 ASCII hexadecimal
characters encoding 32 bytes, with no `0x` prefix; both cases are accepted.
`HEX` means a nonempty, even-length ASCII hex string, with a decoder limit of
2,097,152 characters, additionally constrained by the whole-response limit.
`U64`, `U16`, `U8`, and `U32` mean unsigned JSON integers of those widths, not
strings. `DEC128` means a JSON string parsed by Rust as `u128`; servers should
emit unsigned base-10 digits. Boolean fields are JSON booleans.

The client accepts HTTP success statuses and parses a JSON value. Missing fields,
wrong types, invalid hex, invalid integer ranges, invalid JSON, an oversized
Content-Length, or a successfully read body exceeding its limit produce
`HumanOperationError::Refused`. Transport failure or failure reading the body
produces `HumanOperationError::Unavailable`.

**Every non-success HTTP status, including 503, currently produces `Refused`.**
The error JSON and Retry-After header are not parsed. Registry converts Refused
to `CoreStateError::Unverified` and Unavailable to `CoreStateError::Unavailable`.
Identity converts them to `IdentityError::Unverified` and
`IdentityError::BoundaryUnavailable`, respectively. Other routes return the human
operation error directly. A service returning 503 for durable-state failure
cannot cause the current client to return Unavailable without a client change.

All fields below are required. The client does not reject unknown JSON fields.
No route parses a separate signature field: public keys and evidence digests
are not signatures. Canonical hex fields are opaque bytes to the route decoder;
the server must establish their authority before returning them.

## Routes

All paths below have the prefix `/v1/agent/`.

| Route | Additional query parameters | Response fields |
| --- | --- | --- |
| `registry` | None | `modules`: array of objects containing `module_id`: U16 and `activity_types`: array of U32 |
| `authorized-batch` | `activity_id`: H32 | `batch_id`, `asset`, `previous_state_root`, `resulting_state_root`, `sequencer_public_key`: H32 |
| `balance-context` | None | `account_id`, `asset_id`, `sequencer_id`, `sequencer_public_key`: H32; `currency`, `observed_at`: strings; `age_seconds`, `maximum_age_seconds`, `first_batch_number`, `last_batch_number`: U64 |
| `identity` | `did`: percent-encoded DID text | `authorities`: array of `{kind: string, id: H32}`; `canonical_core_bytes`: HEX; `head_sequence`, `revocation_sequence`: U64; `verification_level`: string; `frozen`: boolean |
| `core-clock` | None | `lower_unix_ms`, `lower_sequence`, `upper_unix_ms`, `upper_sequence`, `observed_head_sequence`: U64; `canonical_attestation`: HEX |
| `capability-scope` | `did`: percent-encoded DID; `authority`, `action_key`, `capability_id`: H32 | `activity_types`: array of U16; `counterparties`, `assets`: arrays of H32; `amount_ceiling`: DEC128; `expiry_sequence`, `observed_sequence`: U64; `enforceable_dimensions`: array of strings; `verification`: U8; `evidence_digest`: H32 |
| `budget-state` | `budget_id`: H32 | `revocation_sequence`, `observed_head_sequence`, `age_sequences`, `maximum_age_sequences`: U64; `verification`: U8; `evidence_digest`, `receipt_digest`, `checkpoint_digest`, `asset`: H32; `remaining`: DEC128 |
| `key-policy` | `did`: percent-encoded DID; `recovery`: literal `true` or `false` | `policy_revision`, `required_delay_seconds`, `maximum_delay_seconds`, `effective_sequence`, `observed_head_sequence`, `age_sequences`, `maximum_age_sequences`: U64; `verification`: U8; `evidence_digest`, `checkpoint_digest`: H32 |

### Registry

The outer array must contain 1 to 32 modules, with no duplicate module IDs.
Closed module IDs are 1 through 9 (Asset, Escrow, Budget, Stream, Service, Perps,
Governance, Bridge, Programs). Consequently only nine distinct entries can
currently be accepted. Each activity array has 1 to 64 entries, strictly
increasing with no duplicates. Each packed activity is
`(module_id << 16) | ordinal`, with ordinal in `1..=65535`, and its module must
match the containing registration. Array order between modules is not checked.

### Authorized batch and balance context

Authorized-batch constructs `AuthorizedBatch` from the five fields; the route
decoder does not itself verify a signature or correlate the answer to the
requested activity. The existing authority verifier can produce these fields
by verifying the signed header, receipt inclusion, execution batch identity,
and receipt outcome. Access still needs a durable principal-to-activity binding;
knowing an activity ID must not authorize access to another principal's facts.

Currency must have 1 to 32 UTF-8 bytes and observed_at 1 to 64 bytes. No timestamp
format or currency alphabet is validated by this decoder. Sequencer authorization
is constructed from its ID, public key, and inclusive batch-number endpoints.
The tuple contains account and asset identity and freshness metadata, not an
account balance. The decoder adds no nonzero or freshness comparisons here;
that does not authorize a server to invent freshness or sequencer authority.

### Identity

Authorities must be nonempty. Allowed `kind` values are `primary_key`,
`session_key`, and `capability_grant`. There is no per-array limit beyond the
response bound, and the decoder does not deduplicate the array.
`verification_level` must be one of `sequencer_signed`, `batch_included`,
`state_proven`, `checkpoint_finalised`, or `settlement_anchored`.
Canonical bytes must be nonempty. Later identity validation rejects frozen
identities and a zero revocation sequence; owner installation additionally
requires checkpoint-finalised evidence. An operator-provisioned policy is not,
by itself, proof of checkpoint finality.

### Core clock

The decoder requires integer fields and nonempty canonical bytes. Lease mapping
then requires increasing wall-clock and sequence anchors, a nonzero lower
sequence, and observed head in the inclusive anchor sequence interval. Requested
wall bounds must lie within the anchor wall interval and be increasing.
It maps the lower bound with floor and upper bound with ceiling using checked
integer arithmetic. The resulting upper sequence must be strictly greater than
both the observed head and the resulting lower sequence. Overflow refuses.

A single signed historical header timestamp is insufficient. In particular,
setting the upper sequence to the observed head cannot authorize a lease: the
mapped upper sequence cannot exceed that anchor, while the client requires it
to exceed the observed head. Extrapolating a future sequence from a local wall
clock would introduce an authority assertion absent from the verified evidence.

### Capability scope

Allowed dimensions are `activity_type`, `counterparty`, `asset`, `amount`,
`rate`, `purpose`, and `expiry`. Arrays are collected into sets, so duplicates
are collapsed. The route decoder accepts any U8 verification rank, but capability
installation requires rank 4 or 5, nonzero observed sequence and evidence digest,
and a requested expiry after the observed sequence. It also applies
`assert_narrowing` against the returned scope.

Installation checks the evidence digest against SHA-256 of six length-prefixed
parts. Each prefix is the part length as four big-endian bytes. Parts, in order:
ASCII `layerx-human/agent-create/agent-evidence/v1`; byte `05`; action_key (32
bytes); capability_id (32 bytes); observed_sequence (8 big-endian bytes);
verification (one byte). This request-binding digest is not a signature or a
substitute for verified policy evidence.

### Budget state

The decoder requires nonzero revocation_sequence; observed_head_sequence at least
revocation_sequence; verification 4 or 5; nonzero evidence_digest, receipt_digest,
checkpoint_digest and asset; positive maximum_age_sequences; and age_sequences
no greater than maximum_age_sequences. Remaining may be zero. It does not check
that age equals the head/observation difference or recompute the digests.

### Key policy

The decoder requires positive policy_revision, required_delay_seconds,
effective_sequence and maximum_age_sequences; maximum_delay_seconds at least
required_delay_seconds; observed_head_sequence at least effective_sequence;
verification 4 or 5; nonzero evidence_digest and checkpoint_digest; and
age_sequences no greater than maximum_age_sequences. Recovery and ordinary
rotation are distinct requests and must select explicitly provisioned policy.

## Configuration and durable evidence

| Environment | Required configuration |
| --- | --- |
| `LAYERX_AUTHORITY_HUMAN_AGENT_TOKEN_FILE` | Absolute canonical path to `human-agent.token`; 32..4096 ASCII bytes in `0x21..0x7e`, no trailing newline. Must differ from every legacy service token. |
| `LAYERX_AUTHORITY_HUMAN_AGENT_TENANT` | One nonempty tenant bound to the token. |
| `LAYERX_AUTHORITY_HUMAN_AGENT_PRINCIPAL` | One nonempty principal bound to the token. |
| `LAYERX_AUTHORITY_PRINCIPAL_POLICY_FILE` | Absolute canonical path to Secret key `principal-policy.json`, schema below. |
| `LAYERX_AUTHORITY_MODULE_REGISTRY_FILE` | Same protected module registry file consumed by the gateway. |
| `LAYERX_AUTHORITY_CORE_CLOCK_HORIZON` | Required positive u64 sequence horizon. |
| `LAYERX_AUTHORITY_STATE_ROOT` | Existing absolute canonical persistent directory, owned by the authority UID, mode 0700. Dedicated to retained receipt records. |

These variables form one configuration group. If all are absent, legacy receipt
service operation remains available and human routes return 503. Partial
configuration prevents startup. The human token is never accepted through the
legacy service-token list. Authentication compares token bytes in constant time;
a correctly authenticated request for any other tenant/principal receives 403.
An absent principal receives 404. Query keys are unique; unexpected keys refuse.

Protected provisioning and evidence files use the trust-history rules: absolute
canonical path, no symlink components, regular file owned by the process UID,
no group or other permissions, bounded reads, and identical inode, ownership,
mode, links, size, mtime and ctime before/after opening and reading. Mount real
files at these paths; projected Secret symlinks do not satisfy these rules.
Policy and registry files are bounded to 16 MiB. The policy is parsed and pinned
by SHA-256 at startup. Every authenticated request re-reads it with protected
checks and compares the digest. Missing, invalid or changed policy returns 503;
installing or changing policy requires a restart. Faults do not authorize stale
in-memory policy.

Verified activity lookups retain canonical receipts and replica documents under
`STATE_ROOT/<activity-id>.json`. Writes use an exclusive mode-0600 pending file,
file fsync, rename and directory fsync before answering success. Existing records
must match exactly. An interrupted pending write, malformed record, signature
failure, changed network, duplicate sequence or unprotected file refuses further
human evidence answers. Each request replays the receipt, header signature,
inclusion and execution-identity verifiers. Cached authorized-batch responses
use the same verified records. Upstream relay documents alone are not retained
because they do not carry a verified receipt.

The clock uses the two highest distinct signed header end sequences. Both time
and sequence must increase. The upper sequence is head plus configured horizon;
the upper timestamp adds floor(horizon * measured milliseconds / measured
sequences), using checked integer arithmetic. A zero-duration extension or any
overflow returns 503. No SystemTime rate is used. Canonical attestation bytes are
the JSON encoding of domain `layerx-human/core-clock/v1`, both original headers
and signatures, horizon and derived upper coordinates. This encoding is an
operator-authorized extrapolation, not a new sequencer signature.

## Principal policy schema

All object fields are required; unknown fields and duplicate principal pairs
or DID/capability bindings refuse the policy. H32 fields are nonzero 64-digit
hexadecimal identifiers. Provision lowercase hexadecimal consistently; bindings
compare exact text. The file is an object with `principals`, an array of:

```text
{
  tenant: string,
  principal: string,
  account_id: H32,
  asset_id: H32,
  activities: [H32],
  budgets: [H32],
  maximum_age_seconds: positive U64,
  maximum_age_sequences: positive U64,
  identities: [{
    did: string,
    authorities: [{kind: primary_key|session_key|capability_grant, id: H32}],
    revocation_sequence: positive U64,
    frozen: boolean,
    evidence: {activity_id: H32, receipt_digest: H32},
    capabilities: [{
      authority: H32,
      action_key: H32,
      capability_id: H32,
      activity_types: [U16],
      counterparties: [H32],
      assets: [H32],
      amount_ceiling: unsigned decimal string,
      expiry_sequence: positive U64,
      enforceable_dimensions: [activity_type|counterparty|asset|amount|rate|purpose|expiry],
      evidence: {activity_id: H32, receipt_digest: H32}
    }],
    rotation: KeyPolicy,
    recovery: KeyPolicy
  }]
}
KeyPolicy = {
  policy_revision: positive U64,
  required_delay_seconds: positive U64,
  maximum_delay_seconds: U64 >= required_delay_seconds,
  effective_sequence: positive U64,
  evidence: {activity_id: H32, receipt_digest: H32}
}
```

Every evidence activity must be in that principal's activity list. Capability
authority must be explicitly listed in its DID authorities. Receipt digests are
the protocol digest of canonical unsigned receipts, checked against retained
verified evidence. Empty capability lists deny every capability. Rotation and
recovery select separate explicit policy entries. Refusal codes distinguish
missing policy, missing binding, missing receipt evidence, stale evidence and
missing state/checkpoint proof.

## Remaining evidence refusals

The canonical registry now requires `schema_version: 2`, `modules` and `assets`.
Each asset contains `asset` (nonzero lowercase H32), `currency` (1..32 bytes),
`decimals` (0..38) and `symbol` (1..32 bytes). Currency and symbol reject control
characters. Assets must be unique, with 1..256 entries; unknown fields refuse.
Both readers reject the old unversioned file and version 1. The authority keeps
its existing module bounds and exact registrations; gateway retains its existing
eight-module bound and Programs ordinal insertion. No gateway writer exists in
`src/main.rs`; provisioning writes the file outside these owned paths.

Balance-context selects currency metadata by the policy asset, requires a held
verified header, and reports that header's timestamp as Unix milliseconds in a
decimal string. Age is computed from the system wall clock, rejecting future
headers and age exceeding the provisioned maximum before returning success.
The sequencer ID, key and batch range are the same startup pins used by the
verifier. Missing metadata or stale/missing evidence returns 503. The response
also includes decimals, symbol, the exact registry file digest and the existing
account evidence inventory. This endpoint returns context, not a state proof.

Budget receipt fields do not encode budget revocation state. Replica documents
provide batch inclusion, not checkpoint certificates. Budget-state returns 503
`budget_revocation_and_checkpoint_evidence_unavailable`. Balance and budget
refusals include an `evidence` object: bound account/asset, latest held account
balance as `remaining`, observed header head (null when no headers are held),
account observation sequence (null when absent), actual receipt digest array,
empty checkpoint digest array, and SHA-256 of the canonical held-evidence
inventory. An account with no receipt evidence reports `remaining: "0"` without
inventing a receipt, checkpoint, revocation sequence or head. An empty inventory
hash identifies the empty inventory; it is not a receipt or checkpoint digest.

Identity and capability routes resolve the provisioned binding and verify its
receipt reference, but a transfer/deploy receipt cannot prove identity or
capability state. They return 503 `identity_state_proof_unavailable` or
`capability_state_proof_unavailable`. Key-policy likewise refuses with
`key_policy_checkpoint_evidence_unavailable`; it does not label a provisioned
policy as checkpoint-finalised. Supporting successful answers requires the
corresponding verified state and checkpoint evidence interface. The real client
collapses these 503 statuses to Refused, as described above.

The real-client TLS regression starts the real authority binary and invokes
`RemoteHumanAuthority` with the server's explicit private CA DER. It preserves
refusal coverage and adds a successful balance-context read against the unchanged
real-node receipt fixture. That historical-fixture test explicitly provisions a
ten-year freshness window; the original short-window test still requires 503.
The root-only real-node tests retain their prerequisites.

The requested derived-header certificate conflicts with the existing finality
contract. `ReplicaDocument` supplies one sequencer signature over the batch-header
digest. `layerx_wire::receipt::CheckpointCertificate` contains guarantor
signatures and a threshold; encoding a sequencer signature into those fields
would not establish checkpoint finality. `layerx-proof` establishes batch
inclusion separately from checkpoint finality. Budget and key-policy clients
require verification rank 4 or 5; identity owner installation and capability
installation also require checkpoint-finalised evidence. These checks are intact.
No state-proof or checkpoint encoding is claimed for the remaining refusals.

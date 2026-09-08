# layerx-human-identity-provider

Linux LXIP identity directory for the Unix client in
`layerx-human-service/src/server/identity_dispatch.rs`. This is independent of
the hosted HTTP identity service. It depends on the client's actual
`PrincipalId`, `Device`, `AccountIdentity`, `Did`, and recovery policy types.

## Run

Build from the repository root:

```
cargo build --locked --manifest-path human/Cargo.toml -p layerx-human-identity-provider
```

Binary: `human/target/debug/layerx-human-identity-provider` (release builds use
`human/target/release/layerx-human-identity-provider`). No arguments, or `serve`,
starts the listener. Required environment:

| Variable | Meaning |
| --- | --- |
| `LAYERX_HUMAN_IDENTITY_PROVIDER_STATE_ROOT` | Absolute canonical dedicated state directory; created as 0700 if absent, existing directory must be owner-only. Parent must already exist. |
| `LAYERX_HUMAN_IDENTITY_PROVIDER_SOCKET` | Absolute socket path, e.g. `/run/layerx/human/identity.sock`; parent must already exist, be owned by the effective UID, and not be group/world writable. |
| `LAYERX_HUMAN_IDENTITY_PROVIDER_ALLOWED_UID` | Decimal effective UID of the human-service process permitted by Linux SO_PEERCRED. Required; no wildcard. |
| `LAYERX_HUMAN_IDENTITY_PROVIDER_RECOVERY_POLICY_FILE` | Absolute path to a 0600, single-link, regular JSON file owned by the provider UID. Required even on restart. Its parent must be protected. |
| `LAYERX_HUMAN_IDENTITY_PROVIDER_DEADLINE_SECONDS` | Optional total connection I/O budget, 1–60 seconds; default 5. |

The recovery file has `root` (array of 32 byte integers), `threshold` (nonzero
u16), and `delay_seconds` (nonzero u64). The operator supplies an established,
exercisable recovery authority's commitment and policy. This server does not
invent recovery commitments or claim to implement that authority. Existing
accounts retain their original policy across configuration changes.

The socket mode is 0660, owned by the provider's effective UID and inherited
socket GID. Configure the client's `LAYERX_HUMAN_IDENTITY_SOCKET` to this socket,
`LAYERX_HUMAN_IDENTITY_PEER_UID` and `LAYERX_HUMAN_IDENTITY_PEER_GID` to its owner
and group, and grant the human service group access to the socket and traversal
of its parent. The client's `LAYERX_HUMAN_IDENTITY_MAX_FRAME_BYTES` should be
1048576 and `LAYERX_HUMAN_IDENTITY_DEADLINE_SECONDS` should accommodate queueing
and the server's I/O budget. The client independently checks socket ownership,
world permissions and server SO_PEERCRED UID/GID before sending requests.

The listener handles one connection at a time with a listen backlog of 16,
allocating at most one 1 MiB request and at most four decoded fields. Each
connection carries one request and one response, then closes. The deadline is
absolute across reads and writes, including partial reads, so a slow sender
cannot extend it by sending occasional bytes. SIGTERM/SIGINT stops acceptance,
finishes the active bounded transaction, and removes only this listener's
socket inode. Filesystem fsync duration is not forcibly bounded. A crashed
listener's socket is removed on startup only if it is owned, protected, and
connection-refused; a live or untrusted endpoint is refused.

## LXIP v1 wire contract

All integers are unsigned, big endian. Both directions start with a u32 body
length, excluding that prefix. The body is:

```
"LXIP" | version:u8=1 | operation-or-status:u8 | field-count:u32
       | (field-length:u32 | field-bytes)*
```

No padding, trailing body bytes, or alternate versions are accepted. Server
body bounds are 10–1048576 bytes, checked before allocation; field counts are
checked against the operation before allocating fields. The client permits a
configured maximum in 64–1048576 and checks both request and response against
that maximum. A too-small client maximum can reject an otherwise valid reply.
Text responses must be nonempty UTF-8 and at most 4096 bytes, then satisfy the
real typed constructors. Request text uses the same 4096-byte bound, refuses
control characters, and imposes the narrower account rules below.

| Operation | Request fields, in order | Success response fields, in order |
| --- | --- | --- |
| 0 `probe` | none | none |
| 1 `provision` | email UTF-8; display name UTF-8; idempotency key UTF-8; timestamp u64 (8 bytes) | principal UTF-8; DID bytes; recovery root (32 bytes); approval threshold u16 (2 bytes); challenge delay u64 (8 bytes) |
| 2 `resolve_email` | email UTF-8 | principal UTF-8 |
| 3 `device_for_assertion` | principal UTF-8; assertion ID UTF-8 | device ID UTF-8; label UTF-8; platform UTF-8 |

Status 0 means success; status 1 is refusal with zero fields. The client treats
**every** nonzero status as `ProviderRefused`; it defines no finer error code
contract. Bad framing/inputs, missing bindings, idempotency conflicts, unknown
operations and capacity limits receive status 1 if the connection remains
writable within its deadline. Wrong-UID peers are disconnected before reading
a frame. I/O failure or timeout appears to the client as
`ProviderUnavailable`; malformed success evidence appears as
`ProviderEvidence`; peer mismatch appears as `ProviderAuthentication`.
Persistence/entropy failures stop the server, withdrawing readiness; no success
is sent for an uncommitted mutation.

Provisioning requires canonical lowercase ASCII email, maximum 256 bytes,
nonempty local/domain parts and a dotted domain, with no whitespace or controls.
Display names are bounded to 256 bytes and validated by `AccountIdentity`.
A new principal uses 256 bits from the operating system RNG, rendered as
`act_` plus lowercase hex, and validated by `PrincipalId`. The DID is
`did:layerx:` plus that principal, validated by `Did`. Email and idempotency
bindings are unique. An exact retry returns the same account and original
policy; timestamp may change on retry, but email/display name may not. A new
key cannot take over an existing email. The client constructs its onboarding
idempotency digest with SHA-256 of the original key.

## Trusted device enrollment

LXIP has no device enrollment operation or device metadata fields. Operation 3
is strictly a persisted principal/assertion lookup, never an attestation of
arbitrary user text. Unknown assertions and cross-principal lookups are refused.

Stop the server and run the binary as its state owner with `bind-device`, using
the same state/policy environment. Send at most 16384 bytes of JSON on stdin:
`principal`, `assertion_id`, and `device` containing `id`, `label`, `platform`.
These values must come from the trusted assertion enrollment authority. The
operator must verify that binding before import. Device fields are validated
with the real `Device` type; an existing assertion cannot change principal or
device. Exact repeated enrollment is idempotent. The library exposes the same
operation as `State::bind_device`. Exclusive state locking prevents concurrent
imports while serving. No enrollment endpoint is exposed to LXIP callers.

The production client currently has no enrollment integration; connecting the
verified assertion authority is required for automatic session flows. The client module and error type are private to the service crate. Integration
tests compile the unchanged original `identity_dispatch.rs` through a path
module, with all its imports bound to the real service types. They execute
`RemoteIdentityProvider` itself against the real listener; no protocol client
copy, fake provider, or service-source modification is used. Auxiliary symbols
in that source remain linked and type-checked. Separate wire and binary tests
exercise malformed frames, protected storage, crash restart and shutdown.

## Durability and limits

`writer.lock` holds an exclusive OS file lock for the full state lifetime.
A durable 0600 `initialized` marker prevents a missing committed snapshot from
being mistaken for a new directory after identities have been acknowledged.
`state.json` is a versioned, SHA-256 checked snapshot containing account/email/key
bindings, original policies and assertion/device bindings. State files are
0600, regular, owned by the effective UID, single-link, and opened with
O_NOFOLLOW/O_NONBLOCK. Symlink directory components and unsafe ancestor owners
or writable non-sticky ancestors are refused. The dedicated root is 0700;
state is access-protected, not encrypted at rest.

Each mutation writes a new 0600 `state.pending` with create-new, fsyncs it,
atomically renames it to `state.json`, and fsyncs the directory before updating
the in-memory snapshot or replying. Startup validates the committed checksum,
version, all typed values, uniqueness, and assertion references before serving.
It then discards an owner-protected uncommitted pending file. A crash before
rename replays the previous snapshot; a crash after rename replays the complete
new snapshot. An interrupted response is safely retried by idempotency key.
A malformed committed file is never reset to empty. Readiness is published only
after this replay and any initial empty-state commit succeed. Each request
rechecks the committed file digest and held directory/lock identities; detected
corruption or replacement withdraws the listener. Failed writes poison that
state instance until it is reopened and replayed.

The snapshot is capped at 16 MiB, 10000 accounts and 20000 assertion bindings.
Capacity exhaustion refuses further mutations without evicting identities.
Device bindings do not expire automatically; their lifecycle is the enrollment
authority's responsibility. Backups must preserve ownership and modes.

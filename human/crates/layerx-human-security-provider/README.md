# LXSP security provider contract

The library and `layerx-human-security-provider` binary serve the existing client
in `layerx-human-service/src/server/production_auth.rs`, unchanged. Discovery and
capability ownership remains local to the human service.

## Framing and encodings

Each call opens a new Unix stream and sends one request. Both directions begin
with a four-byte unsigned big-endian payload length, excluding that prefix.
The request payload is `LXSP`, version byte `1`, operation byte, a four-byte
big-endian field count, then fields. Each field is a four-byte big-endian byte
length followed by exactly that many bytes. Integers carried as fields still
have the field-length prefix: u32 has length 4, u64 has length 8. Strings are
UTF-8, without terminators. Optional u64 is either an empty field or eight bytes.

The response has the same structure, replacing operation with result code.
Code 0 means success; code 1 maps to `SecurityBoundaryError::Refused` immediately
(the client does not decode the remaining refusal payload). Other result codes,
wrong magic/version, malformed successful fields, trailing bytes in a successful
payload, and invalid response lengths map to `InvalidEvidence`. Connection,
timeout configuration, read and write failures map to `Unavailable`.

Client configuration requires an absolute socket path, nonzero deadline and
`maximum_frame_bytes` in 64..=1,048,576. The payload, excluding its outer prefix,
must fit this maximum. Outgoing oversize requests are refused locally. Incoming
zero-length or oversized responses are invalid evidence. Decoded text fields
must contain 1..=4096 UTF-8 bytes. Request strings are not similarly validated
by `call`; the server must validate them. There is no request ID or multiplexing.

## Operations

Here `p` is the UTF-8 `PrincipalId::as_str()`, `now` is a u64 field, and all
unspecified scalar fields are UTF-8. Response lists below exclude framing.

| Opcode | Request fields | Successful response fields |
| --- | --- | --- |
| 0 probe | none | none |
| 1 status | p | status projection |
| 2 begin_setup | p, label, now | setup_id, secret, secret_remask_at:u64, otpauth_uri, uri_remask_at:u64, expires_at:u64 |
| 3 finish_setup | p, setup_id, code, now | method_id, label, enabled_at:u64, last_used_at:optional-u64, backup_remask_at:u64, one or more backup codes |
| 4 disable | p, method_id, now | status projection |
| 5 rotate_backup_codes | p, now | backup_remask_at:u64, one or more backup codes |
| 6 reveal_verified_receipt | p, evidence_id, now | canonical_receipt, remask_at:u64, single byte 0x01 |

A status projection is `backup_codes_remaining:u32, method_count:u32`, followed
by exactly `method_count` groups of four fields: method_id, label, enabled_at:u64,
last_used_at:optional-u64. No other LXSP operations exist in this client.

`TimedSecret` and `BackupCodeSet` require remask_at strictly greater than the
request's now. Empty secrets or backup code lists are invalid. Setup secret and
receipt are copyable; otpauth URI is not. The client decodes setup expires_at but
does not enforce its relation to now. Setup lifetime, TOTP algorithm and period,
clock-skew window, code attempt limits, backup-code consumption, and recovery
record ingestion are not defined by the LXSP client. `RecoveryEvidenceProvider`
requires local verification of stored receipts before returning canonical bytes;
a stored boolean alone does not meet that requirement.

## Authenticator policy

TOTP uses a random 160-bit secret, HMAC-SHA1, six digits, a 30-second period,
and a ±1-period window. Setup expires after 300 seconds; equality is accepted.
A principal has one pending setup; beginning another replaces it. A setup admits
five code attempts, durably counted even when refused, and successful completion
consumes it. Requests cannot move an account's mutation clock backwards.
Method secrets and the accepted TOTP counter are retained. The current wire has
no login-code or backup-code consumption operation, so last_used_at remains
absent. Each completion or rotation generates ten independent 160-bit backup
codes, returned once; only SHA-256 digests are retained. Removing the last method
clears backups. Maximums: 1024 accounts, 16 methods per account, 256-byte labels.
All disclosures remask after 60 seconds; timestamp overflow is refused.

## Configuration

| Environment variable | Requirement |
| --- | --- |
| `LAYERX_HUMAN_SECURITY_PROVIDER_STATE_ROOT` | Required absolute provider-owned state directory; created mode 0700 if absent, parent must exist. |
| `LAYERX_HUMAN_SECURITY_PROVIDER_TRUST_HISTORY` | Required absolute protected canonical registry sequencer trust-history file, mode 0600, owned by provider UID. |
| `LAYERX_HUMAN_SECURITY_PROVIDER_SOCKET` | Required absolute UDS path; cluster value `/run/layerx/human/security.sock`. Existing paths are refused. Parent must be owned by provider UID and not group/other writable. |
| `LAYERX_HUMAN_SECURITY_PROVIDER_ALLOWED_UID` | Required decimal UID of the human service; no default. Checked using Linux SO_PEERCRED. |
| `LAYERX_HUMAN_SECURITY_PROVIDER_DEADLINE_SECONDS` | Optional integer 1–60, default 5; total request/response I/O deadline. |

The socket is 0660. The human service also needs filesystem access via the
socket's owner/group and parent directories. The listener handles one active
connection at a time, with an OS-bounded backlog, frames at most 1 MiB, at most
four request fields and at most 4096 bytes per field. One request per connection.
SIGTERM and SIGINT stop accepting and interrupt pending I/O within 100 ms;
shutdown removes only the socket inode this process created. A stale socket
following SIGKILL requires operator removal after checking no provider owns it.

## Durable file layout

All state files are regular, single-link, provider-owned 0600 files. Paths with
symlink components are refused. `writer.lock` is held with an exclusive flock
for the process lifetime, including administration. `snapshot.json` is a
canonical initial empty snapshot. `initialized` commits its SHA-256 digest.
`00000000000000000001.json` and subsequent numbered files are canonical state
records with sequence and previous-record digest. `head` commits the latest
sequence and digest, detecting a missing journal tail. `transaction.tmp` is the
exclusive temporary file used by atomic rename. Files are fsynced before rename;
the state directory is fsynced afterward. Initial root creation fsyncs its parent.

Startup replays every record, validates bounds and account clocks, verifies all
stored recovery proofs, and checks snapshot, marker, chain and head consistency.
Each operation repeats consistency checks; failure permanently withdraws probe
success for that process. Incomplete writes, unexpected files and corruption
fail closed, including a crash between a journal rename and head publication.
Operator restoration of a consistent backup is required; the server does not
silently discard an ambiguous transaction. Limits are 4096 transactions and
16 MiB per record, after which mutation is refused. No automatic compaction is
provided. Permissions protect secret material; hashes detect corruption, not
malicious rewriting of the whole state by its privileged owner. Rollback of an
entire consistent backup cannot be detected without an external monotonic anchor.

## Recovery administration

Stop the provider (administration requires the same exclusive writer lock), then:

```sh
layerx-human-security-provider ingest-recovery-receipt /protected/recovery.json
```

This command needs only STATE_ROOT and TRUST_HISTORY. It runs as the state
owner and requires an owner-only, single-link 0600 receipt file. The file is
compact canonical JSON, with keys in exactly this order, no whitespace or final
newline, no unknown fields:

```json
{"version":1,"principal":"alice","evidence_id":"recovery-1","canonical_receipt":"BASE64","receipt_proof":"BASE64","header":"BASE64","header_signature":"BASE64"}
```

All binary strings use canonical padded standard base64. `canonical_receipt` is
the real `layerx_wire::receipt::encode` output, `receipt_proof` is
`layerx_proof::merkle::encode_proof`, `header` is the encoded batch header and
`header_signature` is its 64-byte Ed25519 signature. The receipt string must fit
the client's 4096-byte text bound; the entire admin file is bounded to 32 KiB.
The trusted administrator supplies the principal/evidence association; this
association is not a claim made by the protocol receipt itself. It is durably
bound at ingestion, and conflicting reassignment of the same pair is refused.
Identical re-ingestion is idempotent. Maximum: 1024 principals with 64 receipts
each. Receipt bytes are returned exactly as the ingested canonical base64 string.

`ProtocolDeploymentVerifier::from_protected_history` and
`verify_historical_protocol_head` verify the real receipt signature, successful
protocol result, Merkle inclusion, signed header, batch identity and selected
historical trust anchor. Stored material is reverified on replay and reveal;
there is no stored boolean standing in for verification. Historical receipts
have no current-head staleness limit. Trust history retains the registry's exact
canonical binary format and revoked-anchor semantics. Unverifiable material is
refused and never stored. The runtime serves LXSP operations 0–6 only; ingestion
is not a new wire operation.

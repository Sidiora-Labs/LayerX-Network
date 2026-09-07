# Hosted authority

`layerx-receipt-authority` is the hosted receipt-authority binary from
crate `layerx-platform-authority`
(`platform/hosted/authority/Cargo.toml:1-2, 11-13`). It serves verified
authorised-batch facts over TLS
(`platform/hosted/authority/src/main.rs:41`). Every fact is derived from
three inputs only: the canonical receipt bytes named by an activity, the
independent replica's batch evidence for that receipt, and the pinned
sequencer authorisation. The verification library does not read the
sequencer daemon's own store
(`platform/hosted/authority/src/lib.rs:1-6`). Receipt bytes are taken
from the LNI `ReceiptLookup` socket and never from an HTTP receipt URL
(`platform/hosted/authority/src/main.rs:60-61`). Replica evidence is
fetched over loopback HTTP
(`platform/hosted/authority/src/main.rs:57, 218-224`).

The image entrypoint is `/usr/local/bin/layerx-receipt-authority`
(`platform/hosted/authority/Dockerfile:9-11`). Make target
`platform-test-authority` runs the crate tests; `platform-test-trusted-boundary`
depends on it (`platform/Makefile.inc:147-148, 159`).

This page covers that service, its configuration, its output, and its
refusals.

---

## Deployment

The node StatefulSet runs the independent replica as container
`layerxd-authority`: `supervisor.sh --role replica` on the shared node
data and run volumes
(`platform/hosted/node/deployment.yaml:95-105`). The sequencer container
sets `--replica-port` `9402`
(`platform/hosted/node/deployment.yaml:57-58`). Container
`receipt-authority` waits for `/run/layerx/node/core.env`, exports
`LAYERX_AUTHORITY_SEQUENCER_ID` from `LAYERX_CORE_SEQUENCER_ID` and
`LAYERX_AUTHORITY_SEQUENCER_PUBLIC_KEY` from
`/run/layerx/gateway-authority/sequencer-public-key`, then execs the
binary (`platform/hosted/node/deployment.yaml:152-166`). It talks to the
replica at `http://127.0.0.1:9402` and to LNI at
`/run/layerx/node/layerxd.lni.sock`
(`platform/hosted/node/deployment.yaml:175, 178`).

Service `layerx-receipt-authority` is ClusterIP port `9443` targeting
container port `authority-tls` (`9445`)
(`platform/hosted/node/deployment.yaml:181, 255-256`).

The Dockerfile sets `USER 4020:4020`
(`platform/hosted/authority/Dockerfile:8-10`). The pod security context
is `runAsUser` / `runAsGroup` `4020`
(`platform/hosted/node/deployment.yaml:26`). The `receipt-authority`
container overrides that to `runAsUser: 4021`, `runAsGroup: 4020`
(`platform/hosted/node/deployment.yaml:155`). Those two user ids differ.

---

## Configuration

| Key | Role |
| --- | --- |
| `LAYERX_AUTHORITY_LISTEN` | Listen address; default `0.0.0.0:9445` (`platform/hosted/authority/src/main.rs:52, 204-207`). Deployment sets `0.0.0.0:9445` (`platform/hosted/node/deployment.yaml:168`). |
| `LAYERX_AUTHORITY_TLS_CERT_DER` | Server certificate, DER, required, bounded regular file (`platform/hosted/authority/src/main.rs:53, 136-142, 177`). Deployment path `/run/layerx/authority-tls/server.crt.der` (`platform/hosted/node/deployment.yaml:171`). |
| `LAYERX_AUTHORITY_TLS_KEY_DER` | Server private key, PKCS#8 DER, required (`platform/hosted/authority/src/main.rs:54, 178-179`). Deployment path `/run/layerx/authority-tls/server.key.der` (`platform/hosted/node/deployment.yaml:172`). |
| `LAYERX_AUTHORITY_CLIENT_CA_DER` | Optional. When set, a presented client certificate must chain to it (`platform/hosted/authority/src/main.rs:55, 181-193`). Deployment always sets `/run/layerx/trust/ca.crt.der` (`platform/hosted/node/deployment.yaml:173`). The verifier is built with `allow_unauthenticated` (`platform/hosted/authority/src/main.rs:187-189`). |
| `LAYERX_AUTHORITY_TOKEN_FILES` | Colon-separated files, one bearer token each (`platform/hosted/authority/src/main.rs:56, 209-217`). Deployment lists gateway, registry, and webhooks token files (`platform/hosted/node/deployment.yaml:174`). |
| `LAYERX_AUTHORITY_REPLICA_URL` | Loopback `http://host:port` of the independent replica (`platform/hosted/authority/src/main.rs:57, 218-224`). The usage text names `http://127.0.0.1:PORT` (`platform/hosted/authority/src/main.rs:57`). `loopback_http` also admits `localhost` and `[::1]` (`platform/hosted/authority/src/main.rs:157-170`). Deployment sets `http://127.0.0.1:9402` (`platform/hosted/node/deployment.yaml:175`). |
| `LAYERX_AUTHORITY_REPLICA_BEARER_TOKEN_FILE` | Bearer token the replica requires (`platform/hosted/authority/src/main.rs:58, 225`). Deployment path `/run/layerx/tokens/replica-token` (`platform/hosted/node/deployment.yaml:176`). |
| `LAYERX_AUTHORITY_REPLICA_ID` | 64-hex replica identity every evidence document must carry; refused if zero (`platform/hosted/authority/src/main.rs:59, 226-229`). Deployment reads ConfigMap key `replica-id` (`platform/hosted/node/deployment.yaml:177`). |
| `LAYERX_AUTHORITY_LNI_SOCKET` | Absolute LNI unix socket used as the receipt source (`platform/hosted/authority/src/main.rs:60-61, 230-235`). Deployment path `/run/layerx/node/layerxd.lni.sock` (`platform/hosted/node/deployment.yaml:178`). |
| `LAYERX_AUTHORITY_PROTOCOL_NETWORK_ID` | Non-zero 32-bit protocol network id expected in the LNI handshake (`platform/hosted/authority/src/main.rs:62, 236-242`). |
| `LAYERX_AUTHORITY_NETWORK_ID` | Deployment network identifier echoed in every answer (`platform/hosted/authority/src/main.rs:63, 243-244, 665`). |
| `LAYERX_AUTHORITY_WIRE_VERSION` | Wire version echoed in every answer; default is the built protocol version; must equal that version (`platform/hosted/authority/src/main.rs:64, 245-254`). The node manifest does not set this variable. |
| `LAYERX_AUTHORITY_SEQUENCER_ID` | 64-hex sequencer identity pinned for header verification; refused if zero (`platform/hosted/authority/src/main.rs:65, 255-258`). |
| `LAYERX_AUTHORITY_SEQUENCER_PUBLIC_KEY` | 64-hex sequencer public key pinned for header and receipt signatures; refused if zero (`platform/hosted/authority/src/main.rs:66, 256-258`). |
| `LAYERX_AUTHORITY_FIRST_BATCH` | First authorised batch number; must be non-zero (`platform/hosted/authority/src/main.rs:67, 260-266`). Deployment sets `1` (`platform/hosted/node/deployment.yaml:179`). |
| `LAYERX_AUTHORITY_LAST_BATCH` | Last authorised batch number; must be at least first (`platform/hosted/authority/src/main.rs:68, 261-266`). Deployment sets `18446744073709551615` (`platform/hosted/node/deployment.yaml:180`). |

TLS files are refused unless they are regular files of length `1..=65536`
(`platform/hosted/authority/src/main.rs:28, 136-142`). Token files must
contain a bounded printable secret (`platform/hosted/authority/src/main.rs:114-128`).
Bearer comparison is length-checked and constant-time
(`platform/hosted/authority/src/main.rs:430-432`). Routes other than
`/livez` and `/readyz` require `Authorization: Bearer`
(`platform/hosted/authority/src/main.rs:46-49, 419-437, 732-735, 747-749`).

---

## Output fields

`GET /v1/authorized-batches/by-activity/{activity_id}` and
`GET /internal/v1/activities/{activity_id}/authority` return the same
JSON object on success (`platform/hosted/authority/src/main.rs:46-47,
740-749, 656-667`). That object has eight members:

| Member | Source |
| --- | --- |
| `activity_id` | The requested 64-hex activity id (`platform/hosted/authority/src/main.rs:659`) |
| `batch_id` | Hex of the re-derived execution batch id (`platform/hosted/authority/src/main.rs:660`; `platform/hosted/authority/src/lib.rs:305`) |
| `asset` | Hex of the receipt asset (`platform/hosted/authority/src/main.rs:661`; `platform/hosted/authority/src/lib.rs:306`) |
| `previous_state_root` | Hex of the signed header predecessor root (`platform/hosted/authority/src/main.rs:662`; `platform/hosted/authority/src/lib.rs:307`) |
| `resulting_state_root` | Hex of the signed header successor root (`platform/hosted/authority/src/main.rs:663`; `platform/hosted/authority/src/lib.rs:308`) |
| `sequencer_public_key` | Hex of the pinned sequencer public key (`platform/hosted/authority/src/main.rs:664`; `platform/hosted/authority/src/lib.rs:309`) |
| `network_id` | Config `LAYERX_AUTHORITY_NETWORK_ID` (`platform/hosted/authority/src/main.rs:665`) |
| `wire_version` | Config `LAYERX_AUTHORITY_WIRE_VERSION` (`platform/hosted/authority/src/main.rs:666`) |

The ramp client parses those same eight members with
`deny_unknown_fields` (`platform/ramps/toolkit/src/clients.rs:964-975,
1024-1041`). The real-node test asserts exactly those eight keys
(`platform/hosted/authority/tests/real_node.rs:1368-1390`).

`AuthorityFacts` is also eight fields, before hexadecimal encoding:
`activity_id`, `batch_id`, `asset`, `previous_state_root`,
`resulting_state_root`, `sequencer_public_key`, `global_sequence`,
`batch_number` (`platform/hosted/authority/src/lib.rs:132-151`). That
set is not the HTTP object: the library struct carries
`global_sequence` and `batch_number` and does not carry `network_id` or
`wire_version`. The HTTP object does the reverse
(`platform/hosted/authority/src/main.rs:656-667`).

`GET /v1/batches/{batch_id}/receipt-authority?receipt_digest={digest}`
relays the replica document unchanged on HTTP `200` or `404`
(`platform/hosted/authority/src/main.rs:48-49, 673-686`). That body is
not the eight-member authorised-batch object.

---

## Receipt digest

`receipt_locator` decodes the canonical receipt, requires a protocol
receipt, re-encodes the unsigned form, and calls `receipt_digest` on
those unsigned bytes (`platform/hosted/authority/src/lib.rs:11,
186-196`). The service uses that digest, with the receipt's batch id,
as the replica query
(`platform/hosted/authority/src/main.rs:627-638`):

`/v1/batches/{batch_id}/receipt-authority?receipt_digest={digest}`

---

## Replica document

`parse_replica_evidence` decodes JSON with `deny_unknown_fields`
(`platform/hosted/authority/src/lib.rs:164-178, 205-211`). Required
members:

| Member | Check |
| --- | --- |
| `authority_replica_id` | 64-hex; must equal the pinned replica id (`platform/hosted/authority/src/lib.rs:212-216`) |
| `sequencer_public_key` | 64-hex; must equal the pinned sequencer key (`platform/hosted/authority/src/lib.rs:217-221`) |
| `batch_evidence.header_hex` | Non-empty hex header bytes (`platform/hosted/authority/src/lib.rs:174-177, 222-223, 232-234`) |
| `batch_evidence.header_signature` | Hex that decodes to exactly 64 bytes (`platform/hosted/authority/src/lib.rs:224-229`) |
| `batch_evidence.receipt_proof_hex` | Non-empty hex Merkle proof that decodes as a canonical proof (`platform/hosted/authority/src/lib.rs:230-246`) |

Unknown or missing JSON fields, a replica id that is not 64-hex, or a
sequencer key that is not 64-hex are `ReplicaDocument`
(`platform/hosted/authority/src/lib.rs:210-218`). A well-formed replica
id that is not the pinned id is `ReplicaIdentity`
(`platform/hosted/authority/src/lib.rs:214-216`). A well-formed
sequencer key that is not the pinned key is `SequencerKey`
(`platform/hosted/authority/src/lib.rs:219-221`).

---

## Header signature, inclusion, and batch id

`authorized_batch_by_activity` decodes the receipt, requires a protocol
receipt, and refuses an activity id other than the one requested
(`platform/hosted/authority/src/lib.rs:260-270`). It then calls
`verify_receipt` with the replica proof, header, header signature, and
pinned `SequencerAuthorization`
(`platform/hosted/authority/src/lib.rs:271-280`). That call is header
signature verification and Merkle inclusion under the header receipt
root. Failure is `EvidenceRefusal::Inclusion`
(`platform/hosted/authority/src/lib.rs:121, 280`).

After inclusion it requires the receipt protocol version to equal the
header protocol version (`platform/hosted/authority/src/lib.rs:282-284`)
and the receipt global sequence to lie in the header range
(`platform/hosted/authority/src/lib.rs:285-289`). It re-derives the
execution batch id with `receipt_execution_batch_id(protocol, header)`
and refuses unless that value equals `protocol.batch_id()`
(`platform/hosted/authority/src/lib.rs:11, 290-294`). The returned
`batch_id` is that re-derived value
(`platform/hosted/authority/src/lib.rs:305`).

Outcome verification then runs under an `AuthorizedBatch` built from
the derived facts (`platform/hosted/authority/src/lib.rs:295-302,
315-331`). Module `9` operation `0` uses `verify_program_state` and
refuses a present program outcome; every other receipt uses
`verify_outcome` (`platform/hosted/authority/src/lib.rs:321-330`).

---

## Refusals

The `refusal` helper emits `{ "error": { "code": ..., "retry": ... } }`
(`platform/hosted/authority/src/main.rs:380-386`).

| HTTP | Code | Condition |
| --- | --- | --- |
| 401 | `identity_required` | Missing, empty, oversized, or non-matching `Authorization: Bearer` (`platform/hosted/authority/src/main.rs:419-437`) |
| 400 | `invalid_activity_id` | Activity id is not 64-hex or is 32 zero bytes (`platform/hosted/authority/src/main.rs:609-614`) |
| 404 | `unknown_activity` | LNI `ReceiptLookup` returns an empty payload (`platform/hosted/authority/src/main.rs:523-526, 587-591, 617`) |
| 502 | `sequencer_key_mismatch` | LNI handshake authorised sequencer key differs from the pinned key (`platform/hosted/authority/src/main.rs:554-556, 618-620`) |
| 503 | `receipt_source_unavailable` | LNI connect, handshake, capability, encode, send, receive, or decode fails (`platform/hosted/authority/src/main.rs:538-599, 622-624`). Retry-After 5. |
| 503 | `replica_evidence_unavailable` | Replica answers HTTP 404 for the receipt-authority document (`platform/hosted/authority/src/main.rs:641-642`). Retry-After 1. |
| 503 | `replica_unavailable` | Replica connect/read fails, or replica answers a status other than 200 or 404 (`platform/hosted/authority/src/main.rs:440-447, 644-648, 687-691`). Retry-After 5. |
| 502 | `evidence_refused` | Any `EvidenceRefusal` from locator, replica parse, or `authorized_batch_by_activity` (`platform/hosted/authority/src/main.rs:603-605, 627-629, 650-653, 669`) |
| 400 | `invalid_request` | Relay query is not `receipt_digest=` plus 64-hex, or `batch_id` is not 64-hex; also a malformed HTTP request (`platform/hosted/authority/src/main.rs:673-678, 763-765`) |
| 405 | `method_not_allowed` | Method is not `GET` (`platform/hosted/authority/src/main.rs:718-719`) |
| 404 | `not_found` | Unrecognised path, or a query string on a non-relay route (`platform/hosted/authority/src/main.rs:737-738, 751`) |

Library `EvidenceRefusal` values, all served as HTTP `evidence_refused`:

| Variant | Condition |
| --- | --- |
| `ReceiptDecode` | Receipt bytes are not canonically decodable (`platform/hosted/authority/src/lib.rs:106-107, 187`) |
| `ReceiptShape` | Receipt carries no protocol receipt (`platform/hosted/authority/src/lib.rs:108-109, 188`) |
| `ActivityMismatch` | Receipt names a different activity than requested (`platform/hosted/authority/src/lib.rs:110-111, 268-269`; `platform/hosted/authority/src/main.rs:631-632`) |
| `ReplicaDocument` | Replica JSON is the wrong shape (`platform/hosted/authority/src/lib.rs:112-113, 210-211`) |
| `ReplicaIdentity` | Replica document names a different replica identity (`platform/hosted/authority/src/lib.rs:114-115, 214-216`) |
| `SequencerKey` | Replica document names a sequencer key other than the pinned key (`platform/hosted/authority/src/lib.rs:116-117, 219-221`) |
| `EvidenceEncoding` | Replica evidence is not decodable (`platform/hosted/authority/src/lib.rs:118-119, 222-234`) |
| `Inclusion` | Header or Merkle inclusion verification failed (`platform/hosted/authority/src/lib.rs:120-121, 273-280`) |
| `ProtocolVersion` | Receipt and header disagree on protocol version (`platform/hosted/authority/src/lib.rs:122-123, 282-284`) |
| `SequenceRange` | Receipt global sequence lies outside the header range (`platform/hosted/authority/src/lib.rs:124-125, 285-289`) |
| `BatchIdentity` | Receipt batch identity is not the re-derived execution batch id (`platform/hosted/authority/src/lib.rs:126-127, 290-294`) |
| `Receipt` | Outcome verification failed under the derived facts (`platform/hosted/authority/src/lib.rs:128-129, 315-330`) |

A tampered header signature is `Inclusion(InclusionError::HeaderSignature)`
(`platform/hosted/authority/tests/real_node.rs:1514-1524`). A receipt
under another batch's evidence is `Inclusion(InclusionError::Merkle(_))`
(`platform/hosted/authority/tests/real_node.rs:1554-1562`). Those are
the header/signature/batch inclusion refusals. `BatchIdentity` is the
re-derived batch-id mismatch (`platform/hosted/authority/src/lib.rs:290-294`).

Unauthorized on the HTTP surface is `identity_required`, not a library
`EvidenceRefusal`. Unknown receipt on the by-activity surface is
`unknown_activity` when LNI returns empty bytes, and
`replica_evidence_unavailable` when the replica answers 404 for the
digest. Those two 404 paths differ
(`platform/hosted/authority/src/main.rs:617, 641-642`).

---

## Readiness

`GET /livez` returns `200` and `{ "live": true }` with no replica check
(`platform/hosted/authority/src/main.rs:44, 722-723`). `GET /readyz`
probes the replica at
`/v1/batches/{32-zero}/receipt-authority?receipt_digest={32-zero}`
(`platform/hosted/authority/src/main.rs:45, 695-714`). Ready is replica
HTTP `200` or `404` (`platform/hosted/authority/src/main.rs:697-700`).
The body has three members: `ready`, `network_id`, `wire_version`
(`platform/hosted/authority/src/main.rs:705-709`). Ready is HTTP `200`;
not ready is HTTP `503` with Retry-After 5
(`platform/hosted/authority/src/main.rs:710-713`). The node probes
HTTPS `/readyz` and `/livez` on `authority-tls`
(`platform/hosted/node/deployment.yaml:182-183`).

---

## Real-node tests

`platform/hosted/authority/tests/real_node.rs` starts a real `layerxd`
sequencer, a real `layerxd --authority-replica`, and the authority
binary (`platform/hosted/authority/tests/real_node.rs:1-3, 830-835,
1162`).

`real_node_authority_serves_verified_facts_and_reflects_replica_loss`
(`platform/hosted/authority/tests/real_node.rs:1276-1278`):

- `/livez` is `200` JSON (`platform/hosted/authority/tests/real_node.rs:1312-1314`).
- `/readyz` is `200` with `ready=true` and exactly three members
  (`platform/hosted/authority/tests/real_node.rs:1316-1336`).
- Missing bearer is `401` `identity_required`; a wrong token is `401`
  (`platform/hosted/authority/tests/real_node.rs:1338-1352`).
- Gateway bearer on by-activity is `200` with the eight output keys;
  `verify_outcome` accepts the receipt under those facts
  (`platform/hosted/authority/tests/real_node.rs:1354-1408`).
- The internal activity path returns the same JSON
  (`platform/hosted/authority/tests/real_node.rs:1410-1418`).
- The relay body equals the replica body byte-for-byte
  (`platform/hosted/authority/tests/real_node.rs:1427-1445`).
- Independent `verify_receipt` inclusion matches the served state roots
  and batch id (`platform/hosted/authority/tests/real_node.rs:1450-1479`).
- Tampered signature, tampered header, and cross-batch evidence are
  inclusion refusals (`platform/hosted/authority/tests/real_node.rs:1514-1562`).
- Unknown activity is `404` `unknown_activity`
  (`platform/hosted/authority/tests/real_node.rs:1586-1596`).
- After `replica.stop()`, `/readyz` is `503` with `ready=false`,
  by-activity is `503` `replica_unavailable`, and relay is `503`
  `replica_unavailable`
  (`platform/hosted/authority/tests/real_node.rs:1612-1638`).

`real_replica_readiness_relay_and_refusals_without_sequencer` starts
the replica and authority with no sequencer
(`platform/hosted/authority/tests/real_node.rs:1641-1644`):

- `/readyz` is `200` because the replica answers `404` on the zero
  probe (`platform/hosted/authority/tests/real_node.rs:1650-1668`;
  `platform/hosted/authority/src/main.rs:695-700`).
- By-activity without LNI is `503` `receipt_source_unavailable`
  (`platform/hosted/authority/tests/real_node.rs:1687-1694`).
- Relay of an unknown batch is `404` with the same body as the replica
  (`platform/hosted/authority/tests/real_node.rs:1704-1717`).
- After replica loss, `/readyz` is `503` and relay is `503`
  `replica_unavailable`
  (`platform/hosted/authority/tests/real_node.rs:1749-1765`).

[Home](Home.md)

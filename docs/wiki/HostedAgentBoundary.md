# Hosted agent boundary

`layerx-agent-boundary` is the TLS HTTP surface that submits
signed activities onto the node LNI and serves receipts to the
gateway, program registry, and webhooks
(`platform/hosted/agent-boundary/Cargo.toml:2`;
`platform/hosted/agent-boundary/Cargo.toml:8-10`;
`platform/hosted/agent-boundary/src/main.rs:39`). The crate is
`layerx-platform-agent-boundary`; the binary path is
`src/main.rs`. Service string `agent-boundary`
(`platform/hosted/agent-boundary/src/main.rs:39`).

The image is `ghcr.io/sidiora-labs/layerx-agent-boundary:0.1.0`,
user `4020:4020`, entrypoint
`/usr/local/bin/layerx-agent-boundary`
(`platform/hosted/agent-boundary/Dockerfile:5-11`). The node
StatefulSet runs that image as container `agent-boundary` with
`runAsUser: 4021` and `runAsGroup: 4020`
(`platform/hosted/node/deployment.yaml:193-196`). Those two
identities differ. Uid `4021` is the LNI allowed uid; see
[Hosted node](HostedNode.md).

It listens on `0.0.0.0:9446`. Service
`layerx-agent-boundary` is ClusterIP port `9443` targeting
container port `agent-tls` (`9446`)
(`platform/hosted/node/deployment.yaml:198`;
`platform/hosted/node/deployment.yaml:211`;
`platform/hosted/node/deployment.yaml:261-264`). There is no
Ingress object for this Service. The gateway component URL is
this Service, not the core Service
(`platform/hosted/gateway/deployment.yaml:78`;
[Hosted gateway](HostedGateway.md)). Qualification binds
`LAYERX_QUALIFICATION_AGENT_URL` as that Service
(`platform/hosted/tests/beta-cluster.sh:1136-1137`).

This page covers that binary, its journal, the tests in
`platform/hosted/agent-boundary/`, and the node-pod wiring.
It does not document [Agentd](Agentd.md) itself.

---

## TLS

Inbound TLS is rustls. The certificate is
`LAYERX_AGENT_BOUNDARY_TLS_CERT_DER`; the PKCS#8 key is
`LAYERX_AGENT_BOUNDARY_TLS_KEY_DER`
(`platform/hosted/agent-boundary/src/main.rs:334-341`). When
`LAYERX_AGENT_BOUNDARY_CLIENT_CA_DER` is set, a presented
client certificate must chain to it; the verifier is built
with `allow_unauthenticated`
(`platform/hosted/agent-boundary/src/main.rs:343-357`). When
unset, the listener uses no client authentication
(`platform/hosted/agent-boundary/src/main.rs:359-361`). The
node manifest always sets the CA to
`/run/layerx/trust/ca.crt.der`
(`platform/hosted/node/deployment.yaml:203`). A certificate
outside that CA does not complete `/livez`
(`platform/hosted/agent-boundary/tests/real_node.rs:2021-2032`).

At most 128 connections are live; further accepts are dropped
with no HTTP response
(`platform/hosted/agent-boundary/src/main.rs:46`;
`platform/hosted/agent-boundary/src/main.rs:1942-1967`).
Request bodies are bounded at `1_048_576 + 16 KiB`; activity
bodies at `1_048_576`
(`platform/hosted/agent-boundary/src/main.rs:40-41`;
`platform/hosted/agent-boundary/src/main.rs:1871`). Connect
timeout to the node is 3s; I/O timeout is 15s
(`platform/hosted/agent-boundary/src/main.rs:44-45`). Incoming
headers and bodies are zeroized on drop
(`platform/hosted/agent-boundary/src/main.rs:191-198`).
Responses are `Content-Type: application/json`,
`Cache-Control: no-store`, `Connection: close`
(`platform/hosted/agent-boundary/src/main.rs:1915-1921`).

---

## Tokens

Three inbound Bearer planes and one outbound node Bearer. The
three inbound secrets must be pairwise distinct
(`platform/hosted/agent-boundary/src/main.rs:430-446`).
Comparison is length-checked and constant-time
(`platform/hosted/agent-boundary/src/main.rs:1676-1699`).
`GET /livez` and `GET /readyz` run before authentication
(`platform/hosted/agent-boundary/src/main.rs:1710-1718`).

| Credential | File | Plane | Never |
| --- | --- | --- | --- |
| Gateway token | `LAYERX_AGENT_BOUNDARY_GATEWAY_TOKEN_FILE` | `POST /v1/activities`, program call/deploy/upgrade/wind-down/simulate, `GET /v1/receipts/{id}`, `GET /v1/programs/activities/{id}`, `GET /v1/programs/receipts/by-idempotency/{key}` (`platform/hosted/agent-boundary/src/main.rs:1741-1768`) | Relay paths or `/internal/v1/receipts/` |
| Registry token | `LAYERX_AGENT_BOUNDARY_REGISTRY_TOKEN_FILE` | `GET` relay paths and `GET /internal/v1/receipts/{id}` (`platform/hosted/agent-boundary/src/main.rs:1720-1736`) | Gateway `/v1` writes |
| Webhook token | `LAYERX_AGENT_BOUNDARY_WEBHOOK_TOKEN_FILE` | `GET /internal/v1/receipts/{id}` only (`platform/hosted/agent-boundary/src/main.rs:1729-1736`) | Gateway `/v1` writes and relay paths |
| Node token | `LAYERX_AGENT_BOUNDARY_NODE_BEARER_TOKEN_FILE` | `Authorization: Bearer` to loopback program HTTP (`platform/hosted/agent-boundary/src/main.rs:1612-1616`) | Presented on the TLS listener |

Missing, empty, oversized, or non-matching Bearer is `401`
`identity_required`
(`platform/hosted/agent-boundary/src/main.rs:1665-1700`).
Wrong plane is `403` `entitlement_denied`
(`platform/hosted/agent-boundary/src/main.rs:1721-1722`;
`platform/hosted/agent-boundary/src/main.rs:1730-1731`;
`platform/hosted/agent-boundary/src/main.rs:1760-1761`). Secret
files are stripped of trailing CR/LF and refused when empty or
longer than 4096 bytes
(`platform/hosted/agent-boundary/src/main.rs:309-319`).
Startup with a shared or missing webhook token exits `2`
(`platform/hosted/agent-boundary/tests/real_node.rs:2443-2478`;
`platform/hosted/agent-boundary/src/main.rs:1983-1987`).

The node manifest mounts Secret
`layerx-gateway-component-client` as the gateway token,
`layerx-webhooks-component-client` as the webhook token, and
`layerx-program-registry-node-client` as the registry token
(`platform/hosted/node/deployment.yaml:204-206`;
`platform/hosted/node/deployment.yaml:221-223`;
`platform/hosted/node/deployment.yaml:239-241`). The node
program token is `/run/layerx/tokens/program-token`
(`platform/hosted/node/deployment.yaml:209`).

---

## Routes

HTTP/1.1 only. `Transfer-Encoding` is refused. Duplicate
headers are refused. Host is required
(`platform/hosted/agent-boundary/src/main.rs:1839-1894`).
Query strings are refused on `/v1` and `/internal` except the
relay allow-list
(`platform/hosted/agent-boundary/src/main.rs:1733-1739`).
Malformed framing is `400` `invalid_request`
(`platform/hosted/agent-boundary/src/main.rs:1932-1934`).

| Method and path | Plane | Input | Success |
| --- | --- | --- | --- |
| `GET /livez` | none | none | `200` `{"status":"live","service":"agent-boundary"}` (`platform/hosted/agent-boundary/src/main.rs:1710-1711`) |
| `GET /readyz` | none | none | readiness document below (`platform/hosted/agent-boundary/src/main.rs:1713-1714`; `platform/hosted/agent-boundary/src/main.rs:1648-1662`) |
| `POST /v1/activities` | Gateway | `Content-Type: application/octet-stream`; `Idempotency-Key`; signed activity bytes | LNI submit then receipt (`platform/hosted/agent-boundary/src/main.rs:1764`; `platform/hosted/agent-boundary/src/main.rs:1303-1379`) |
| `POST /v1/programs/call` | Gateway | same; Programs ordinal `3` | same; success `state` `executed` (`platform/hosted/agent-boundary/src/main.rs:133-139`; `platform/hosted/agent-boundary/src/main.rs:1765`) |
| `POST /v1/programs/deploy` | Gateway | ordinal `1` | same (`platform/hosted/agent-boundary/src/main.rs:1766`) |
| `POST /v1/programs/upgrade` | Gateway | ordinal `2` | same |
| `POST /v1/programs/wind-down` | Gateway | ordinal `7` | same |
| `POST /v1/programs/simulate` | Gateway | octet-stream Programs ordinal `3`; no idempotency key | simulation document; `committed` is `false` (`platform/hosted/agent-boundary/src/main.rs:1189-1267`; `platform/hosted/agent-boundary/src/main.rs:1769`) |
| `GET /v1/receipts/{id}` | Gateway | 64 hex activity id | `{"result":{"activity_id","receipt"}}` (`platform/hosted/agent-boundary/src/main.rs:1382-1393`; `platform/hosted/agent-boundary/src/main.rs:1773-1774`) |
| `GET /internal/v1/receipts/{id}` | Registry or Webhook | 64 hex | `{"activity_id","receipt"}` without the `result` wrapper (`platform/hosted/agent-boundary/src/main.rs:1389-1392`; `platform/hosted/agent-boundary/src/main.rs:1729-1736`) |
| `GET /v1/programs/activities/{id}` | Gateway | 64 hex | completed program call plus `program_id` (`platform/hosted/agent-boundary/src/main.rs:1490-1565`; `platform/hosted/agent-boundary/src/main.rs:1775-1776`) |
| `GET /v1/programs/receipts/by-idempotency/{key}` | Gateway | identifier ≤ 128 | node lookup then sequencer-signature check (`platform/hosted/agent-boundary/src/main.rs:1460-1487`; `platform/hosted/agent-boundary/src/main.rs:1771-1772`) |

Gateway submit success is
`{"result":{"state","activity_id","receipt","terminal_payload","call_graph"}}`
(`platform/hosted/agent-boundary/src/main.rs:870-873`).
`state` is `completed` for `/v1/activities` with
`result_code == 0`, `executed` for program routes with
`result_code == 0`, otherwise `refused`
(`platform/hosted/agent-boundary/src/main.rs:123-131`;
`platform/hosted/agent-boundary/src/main.rs:865-868`).
Ordinary activities leave `terminal_payload` and `call_graph`
empty (`platform/hosted/agent-boundary/tests/real_node.rs:1607-1615`).

Indeterminate submit is `202`
`{"state":"unknown","activity_id":…,"retry":"after","retry_after_seconds":2}`
(`platform/hosted/agent-boundary/src/main.rs:880-887`;
`platform/hosted/agent-boundary/src/main.rs:1173-1184`). That
is not a verified success.

Simulation success
(`platform/hosted/agent-boundary/src/main.rs:1275-1300`):

```
{"result":{"committed":false,"execution":{state,activity_id,program_id,result_code,receipt,terminal_payload,call_graph},"simulation_evidence":{boundary_id,activity_id,previous_state_root,hypothetical_state_root,observed_sequence,observed_at,committed:false,public_key,signature}}}
```

`execution.state` is `simulated` when `result_code == 0`, else
`refused` (`platform/hosted/agent-boundary/src/main.rs:1261-1264`).

---

## Relay paths

Registry `GET` only. The path and query must match exactly
(`platform/hosted/agent-boundary/src/main.rs:1568-1596`;
`platform/hosted/agent-boundary/src/main.rs:1720-1727`). The
response status and body are the node document unchanged
(`platform/hosted/agent-boundary/src/main.rs:1602-1645`;
`platform/hosted/agent-boundary/tests/real_node.rs:1983-2003`).

| Path | Query |
| --- | --- |
| `/v1/protocol/account-state/head` | none |
| `/v1/receipts/{64 hex}/account-state` | none |
| `/v1/programs/account-state/changes` | `after_sequence=` decimal |
| `/v1/programs/{64 hex}/account-state` | `at=` decimal |
| `/v1/batches/{64 hex}/receipt-authority` | `receipt_digest=` 64 hex |

Any other query or identifier is `404` `not_found`
(`platform/hosted/agent-boundary/tests/real_node.rs:2007-2015`).
Gateway Bearer on a relay path is `403` `entitlement_denied`
(`platform/hosted/agent-boundary/src/main.rs:1721-1722`). Node
connect/I/O failure is `503` `node_unavailable`; a non-HTTP/1.1
or non-UTF-8 node reply is `503` `node_invalid`
(`platform/hosted/agent-boundary/src/main.rs:1604-1639`). Relay
body bound is 4 MiB
(`platform/hosted/agent-boundary/src/main.rs:42`).

These relay targets are the program listener on
[Hosted node](HostedNode.md), not [Hosted core](HostedCore.md)
and not [Hosted authority](HostedAuthority.md). The
receipt-authority TLS Service is a separate container in the
same pod. The node does not expose this listener through
[Paxeer boundary](PaxeerBoundary.md).
`LAYERX_AGENT_BOUNDARY_NODE_URL` must be
`http://127.0.0.1:<port>`
(`platform/hosted/agent-boundary/src/main.rs:410-422`). The
manifest sets `http://127.0.0.1:9401`
(`platform/hosted/node/deployment.yaml:208`).

---

## Submit, journal, and receipts

`decode_activity` requires a canonical signed envelope for the
configured module registry, protocol
`STATE_COMMITMENT_PROTOCOL_VERSION`, and
`LAYERX_AGENT_BOUNDARY_PROTOCOL_NETWORK_ID`
(`platform/hosted/agent-boundary/src/main.rs:765-776`).
Authority must be 32 bytes
(`platform/hosted/agent-boundary/src/main.rs:777-780`). A
program route that does not match module Programs and the
route ordinal is `400` `not_program_call` or
`wrong_program_operation`
(`platform/hosted/agent-boundary/src/main.rs:785-797`).
Deploy/upgrade require Wasm magic `\0asm\x01\0\0\0` and
SHA-256 of the Wasm equal to `new_hash`
(`platform/hosted/agent-boundary/src/main.rs:817-842`).

LNI connect uses interface `Version::V1_4`, the built protocol
version, and the configured network id, and requires
capabilities `NodeInfo`, `Submit`, and `ReceiptLookup`
(`platform/hosted/agent-boundary/src/main.rs:511-531`). Frame
limit is `1_212_416` bytes
(`platform/hosted/agent-boundary/src/main.rs:47`;
`platform/hosted/agent-boundary/src/main.rs:501-508`). Submit
maps `SubmitError` (`platform/hosted/agent-boundary/src/main.rs:669-701`):

| `SubmitError` | HTTP |
| --- | --- |
| `CoreRefusal` | `409` if retriable, `422` if terminal, code from `ResultCode` (`platform/hosted/agent-boundary/src/main.rs:626-642`; `platform/hosted/agent-boundary/src/main.rs:681-682`) |
| `Wire` / `Envelope` | `400` `malformed_activity` |
| `SignatureLength` / `Signature` | `422` `bad_signature` |
| `ProtocolVersion` | `422` `version_unsupported` |
| `Network` | `422` `wrong_network` |
| `UnavailableCapability` | `503` `node_unavailable` |
| `Disconnected` | `503` `node_transport_lost` on lookup; `202` unknown on submit (`platform/hosted/agent-boundary/src/main.rs:699-700`; `platform/hosted/agent-boundary/src/main.rs:1184`) |

Receipt lookup is LNI tag `5` / `6`; empty payload is Absent
(`platform/hosted/agent-boundary/src/main.rs:50-52`;
`platform/hosted/agent-boundary/src/main.rs:563-603`). Present
receipts are sequencer-signed protocol receipts whose activity
id matches (`platform/hosted/agent-boundary/src/main.rs:605-623`).
Poll interval is 50 ms until `LAYERX_AGENT_BOUNDARY_RECEIPT_WAIT_MS`
(`platform/hosted/agent-boundary/src/main.rs:49`;
`platform/hosted/agent-boundary/src/main.rs:1096-1108`).

Idempotency key is required, identifier ≤ 128
(`[A-Za-z0-9_-.:]`)
(`platform/hosted/agent-boundary/src/main.rs:286-292`;
`platform/hosted/agent-boundary/src/main.rs:1310-1314`). The
journal file is `STATE_DIR/journal/{sha256(key)}.json`; the
activity index is `STATE_DIR/activities/{activity_id}`
(`platform/hosted/agent-boundary/src/main.rs:705-714`;
`platform/hosted/agent-boundary/src/main.rs:475-476`). Same
digest and route replay the stored outcome; a different
digest or route is `409` `idempotency_conflict`
(`platform/hosted/agent-boundary/src/main.rs:1328-1337`).
Writes are temp file, `sync_all`, `rename`, directory
`sync_all` (`platform/hosted/agent-boundary/src/main.rs:716-734`).

Program CALL additionally fetches replica evidence and node
artifacts, then `artifacts::verify`
(`platform/hosted/agent-boundary/src/main.rs:890-955`;
`platform/hosted/agent-boundary/src/artifacts.rs:105-166`).
Lifecycle deploy/upgrade/wind-down bind protocol 3, module 9
version 4, operation 0, and absent program outcome
(`platform/hosted/agent-boundary/src/main.rs:958-993`).

---

## Config keys

| Key | Role |
| --- | --- |
| `LAYERX_AGENT_BOUNDARY_LISTEN` | Bind address; default `0.0.0.0:9446` (`platform/hosted/agent-boundary/src/main.rs:426-428`; `platform/hosted/node/deployment.yaml:198`) |
| `LAYERX_AGENT_BOUNDARY_TLS_CERT_DER` | Inbound server certificate DER; manifest `/run/layerx/agent-tls/server.crt.der` |
| `LAYERX_AGENT_BOUNDARY_TLS_KEY_DER` | Inbound PKCS#8 key DER; manifest `/run/layerx/agent-tls/server.key.der` |
| `LAYERX_AGENT_BOUNDARY_CLIENT_CA_DER` | Optional client CA DER; manifest `/run/layerx/trust/ca.crt.der` |
| `LAYERX_AGENT_BOUNDARY_GATEWAY_TOKEN_FILE` | Gateway Bearer |
| `LAYERX_AGENT_BOUNDARY_REGISTRY_TOKEN_FILE` | Registry Bearer |
| `LAYERX_AGENT_BOUNDARY_WEBHOOK_TOKEN_FILE` | Webhook Bearer |
| `LAYERX_AGENT_BOUNDARY_LNI_SOCKET` | Unix socket to `layerxd` LNI; manifest `/run/layerx/node/layerxd.lni.sock` (`platform/hosted/node/deployment.yaml:207`) |
| `LAYERX_AGENT_BOUNDARY_LNI_DEADLINE_MS` | LNI deadline `1..=60000`; default `10000` (`platform/hosted/agent-boundary/src/main.rs:451-454`) |
| `LAYERX_AGENT_BOUNDARY_RECEIPT_WAIT_MS` | Receipt poll `1..=60000`; default `5000` (`platform/hosted/agent-boundary/src/main.rs:455-458`) |
| `LAYERX_AGENT_BOUNDARY_PROTOCOL_NETWORK_ID` | Non-zero `u32` protocol network id (`platform/hosted/agent-boundary/src/main.rs:459-465`) |
| `LAYERX_AGENT_BOUNDARY_NETWORK_ID` | Canonical network name ≤ 64 (`platform/hosted/agent-boundary/src/main.rs:466-470`) |
| `LAYERX_AGENT_BOUNDARY_NODE_URL` | Loopback `http://127.0.0.1:<port>` |
| `LAYERX_AGENT_BOUNDARY_NODE_BEARER_TOKEN_FILE` | Bearer for node HTTP |
| `LAYERX_AGENT_BOUNDARY_STATE_DIR` | Creates `journal/` and `activities/`; manifest `/var/lib/layerx/agent-boundary` (`platform/hosted/agent-boundary/src/main.rs:471-476`; `platform/hosted/node/deployment.yaml:210`) |
| `LAYERX_AGENT_BOUNDARY_MODULE_REGISTRY_FILE` | Optional JSON `{modules:[{module,ordinals}]}`; default modules `1..=9` with ordinals `1..=16` (`platform/hosted/agent-boundary/src/main.rs:367-407`) |

The testnet pod pins
`LAYERX_AGENT_BOUNDARY_NETWORK_ID` from ConfigMap
`network-name` (`layerx-testnet`) and
`LAYERX_AGENT_BOUNDARY_PROTOCOL_NETWORK_ID` from `network-id`
(`402`) (`platform/hosted/node/deployment.yaml:199-200`).

---

## Callers the NetworkPolicy admits

Node ingress admits TCP `9446` from `app=layerx-gateway`,
`app=layerx-program-registry`, namespace `layerx-developer`
`app=layerx-webhooks`, and namespace `layerx-developer`
`app=layerx-dashboard-api`
(`platform/hosted/node/deployment.yaml:275-284`).
`app=layerx-testnet-control` is admitted on `9443`/`9444`/`9445`
and not on `9446`
(`platform/hosted/node/deployment.yaml:273-274`). Those two
port sets differ.

---

## Typed refusals

HTTP envelope `{ "error": { "code": …, "retry": "never"|"after", … } }`
(`platform/hosted/agent-boundary/src/main.rs:1793-1807`).

| HTTP | Code | Condition |
| --- | --- | --- |
| 400 | `invalid_request` | Framing, Host, or request line failed (`platform/hosted/agent-boundary/src/main.rs:1932-1934`) |
| 400 | `content_type_required` | Submit/simulate without `application/octet-stream` (`platform/hosted/agent-boundary/src/main.rs:1190-1191`; `platform/hosted/agent-boundary/src/main.rs:1304-1305`) |
| 400 | `invalid_activity_length` | Empty or > 1 MiB body (`platform/hosted/agent-boundary/src/main.rs:1193-1194`; `platform/hosted/agent-boundary/src/main.rs:1307-1308`) |
| 400 | `idempotency_key_required` | Missing `Idempotency-Key` on submit (`platform/hosted/agent-boundary/src/main.rs:1310-1311`) |
| 400 | `invalid_idempotency_key` | Key fails `valid_identifier` (`platform/hosted/agent-boundary/src/main.rs:1313-1314`; `platform/hosted/agent-boundary/src/main.rs:1461-1462`) |
| 400 | `malformed_activity` | Signed decode failed (`platform/hosted/agent-boundary/src/main.rs:766-767`) |
| 400 | `non_canonical_activity` | Re-encode ≠ body (`platform/hosted/agent-boundary/src/main.rs:768-769`) |
| 400 | `not_program_call` | Call route is not Programs ordinal 3 (`platform/hosted/agent-boundary/src/main.rs:789-796`) |
| 400 | `wrong_program_operation` | Lifecycle route ordinal mismatch (`platform/hosted/agent-boundary/src/main.rs:794`) |
| 400 | `malformed_program_call` | `NativeProgramCall::decode` failed (`platform/hosted/agent-boundary/src/main.rs:804-805`) |
| 400 | `malformed_program_deploy` / `malformed_program_upgrade` / `malformed_program_wind_down` | Lifecycle payload decode failed (`platform/hosted/agent-boundary/src/main.rs:820-832`) |
| 400 | `malformed_program_wasm` | Wasm magic mismatch (`platform/hosted/agent-boundary/src/main.rs:836-837`) |
| 400 | `program_payload_hash_mismatch` | SHA-256(Wasm) ≠ `new_hash` (`platform/hosted/agent-boundary/src/main.rs:839-841`) |
| 400 | `invalid_activity_id` | Path id is not 64 hex (`platform/hosted/agent-boundary/src/main.rs:1383-1384`; `platform/hosted/agent-boundary/src/main.rs:1491-1492`) |
| 401 | `identity_required` | Missing/wrong Bearer (`platform/hosted/agent-boundary/src/main.rs:1671`; `platform/hosted/agent-boundary/src/main.rs:1700`) |
| 403 | `entitlement_denied` | Token plane does not own the path (`platform/hosted/agent-boundary/src/main.rs:1721-1722`; `platform/hosted/agent-boundary/src/main.rs:1760-1761`) |
| 404 | `not_found` | Unknown path, query on a non-relay route, or non-GET on a GET route (`platform/hosted/agent-boundary/src/main.rs:1724-1725`; `platform/hosted/agent-boundary/src/main.rs:1738-1739`; `platform/hosted/agent-boundary/src/main.rs:1781`) |
| 404 | `receipt_not_found` | LNI Absent, node 404, or unknown non-hex32 idempotency key (`platform/hosted/agent-boundary/src/main.rs:1395`; `platform/hosted/agent-boundary/src/main.rs:1424-1425`; `platform/hosted/agent-boundary/src/main.rs:1473-1474`) |
| 404 | `activity_not_journaled` | No activity index (`platform/hosted/agent-boundary/src/main.rs:1497-1498`) |
| 409 | `idempotency_conflict` | Same key, different digest or route (`platform/hosted/agent-boundary/src/main.rs:1336-1337`) |
| 409 / 422 | `ResultCode` snake_case | LNI `CoreRefusal`; 409 with retry 5 if retriable, 422 if terminal (`platform/hosted/agent-boundary/src/main.rs:626-642`) |
| 422 | `version_unsupported` | Protocol version mismatch (`platform/hosted/agent-boundary/src/main.rs:771-772`; `platform/hosted/agent-boundary/src/main.rs:690-691`) |
| 422 | `wrong_network` | Network id mismatch (`platform/hosted/agent-boundary/src/main.rs:774-775`; `platform/hosted/agent-boundary/src/main.rs:693-694`) |
| 422 | `unsupported_authority` | Authority not 32 bytes (`platform/hosted/agent-boundary/src/main.rs:777-780`) |
| 422 | `bad_signature` | Submit signature error (`platform/hosted/agent-boundary/src/main.rs:687-688`) |
| 502 | `receipt_invalid` | Idempotency receipt failed sequencer/protocol bind (`platform/hosted/agent-boundary/src/main.rs:1484-1486`) |
| 503 | `node_unavailable` | LNI or node HTTP connect/I/O (`platform/hosted/agent-boundary/src/main.rs:214-216`; `platform/hosted/agent-boundary/src/main.rs:1605`) |
| 503 | `node_transport_lost` | LNI send/receive/correlation (`platform/hosted/agent-boundary/src/main.rs:218-220`) |
| 503 | `node_invalid` | Node HTTP status/body invalid (`platform/hosted/agent-boundary/src/main.rs:1622-1639`) |
| 503 | `capability_unavailable` | Simulate without `Capability::Simulate` or class 3 (`platform/hosted/agent-boundary/src/main.rs:1208-1209`; `platform/hosted/agent-boundary/src/main.rs:1227-1236`) |
| 503 | `persistence_unavailable` | Journal I/O (`platform/hosted/agent-boundary/src/main.rs:1147-1148`) |
| 503 | `persistence_invalid` | Journal binding mismatch (`platform/hosted/agent-boundary/src/main.rs:1342`; `platform/hosted/agent-boundary/src/main.rs:1401-1402`) |
| 503 | `program_artifacts_unavailable` | Replica or artifact HTTP not 200 (`platform/hosted/agent-boundary/src/main.rs:907-908`; `platform/hosted/agent-boundary/src/main.rs:930-931`) |
| 503 | `program_artifacts_invalid` | Artifact verify failed (`platform/hosted/agent-boundary/src/main.rs:897`; `platform/hosted/agent-boundary/src/main.rs:1044-1046`) |
| 503 | `lifecycle_receipt_invalid` | Lifecycle receipt bind failed (`platform/hosted/agent-boundary/src/main.rs:1000-1002`; `platform/hosted/agent-boundary/src/main.rs:1086-1087`) |
| 503 | `receipt_unavailable` | Idempotency node status not 200/404 (`platform/hosted/agent-boundary/src/main.rs:1476-1482`) |

---

## Readiness

`GET /livez` does not contact the node
(`platform/hosted/agent-boundary/src/main.rs:1710-1711`).
`GET /readyz` is ready only when LNI handshake succeeds and
TCP to `127.0.0.1:{node.port}` connects. Success is `200`

`{"ready":true,"network_id":"<LAYERX_AGENT_BOUNDARY_NETWORK_ID>","wire_version":"<handshake protocol>","synchronous_receipts":true,"state_snapshot":true}`

(`platform/hosted/agent-boundary/src/main.rs:1648-1662`).
Handshake or TCP failure is `503` `node_unavailable` retry 5.
HTTPS probes are `/readyz` period 5s and `/livez` period 15s
on `agent-tls` (`platform/hosted/node/deployment.yaml:212-213`).
After sequencer loss, `/readyz` is `503` and `/livez` is `200`
(`platform/hosted/agent-boundary/tests/real_node.rs:2151-2193`).

The gateway `/readyz` component probe labels this surface
`core_agent_boundary` and requires this JSON plus matching
`network_id` and `wire_version`
([Hosted gateway](HostedGateway.md);
`platform/hosted/gateway/src/main.rs:2805`).

---

## Tests

`make platform-test-agent-boundary` runs
`cargo test --offline --manifest-path platform/Cargo.toml --locked -p layerx-platform-agent-boundary`
(`platform/Makefile.inc:150-151`).
`platform-test-trusted-boundary` depends on that target
(`platform/Makefile.inc:159`).

Portable crate tests:

| Test | Proves |
| --- | --- |
| `native_c_lifecycle_vectors_are_admitted` | Deploy/upgrade/wind-down fixtures decode; a trailing byte is refused (`platform/hosted/agent-boundary/src/lifecycle_tests.rs:18-39`) |
| `lifecycle_code_hash_substitution_refuses_before_submit` | Flipped hash is `400` `program_payload_hash_mismatch`; reserved-field flip is `malformed_program_*` (`platform/hosted/agent-boundary/src/lifecycle_tests.rs:42-59`) |
| `idempotency_receipts_require_signed_activity_and_sequencer_bindings` | Idempotency receipt JSON binds activity id and sequencer key (`platform/hosted/agent-boundary/src/lifecycle_tests.rs:62-80`) |
| `historical_native_evidence_retains_identity_and_root_checks` | Historical batch evidence binds batch id and state roots; wrong key or network is refused (`platform/hosted/agent-boundary/src/artifacts.rs:267-335`) |
| `maintained_signed_evidence_authenticates_activity_identity_and_refuses_substitution` | Occupancy-maintenance evidence; historical kind, swapped proof, or flipped receipt is refused (`platform/hosted/agent-boundary/src/artifacts.rs:338-410`) |
| `artifact_documents_reject_identity_substitution_partial_pairs_and_noncanonical_hex` | Artifact JSON identity, partial payload/graph, and uppercase hex (`platform/hosted/agent-boundary/src/artifacts.rs:413-426`) |

`tests/real_node.rs` drives the real `layerx-agent-boundary`
binary over TLS against a real `layerxd` sequencer and
`layerxd --authority-replica` from `build/bin`
(`platform/hosted/agent-boundary/tests/real_node.rs:1-3`):

- `real_node_boundary_serves_the_component_contract`: `/livez`
  and `/readyz`; entitlements; SEND submit/replay/conflict;
  receipt routes; program-route mismatch; typed refusals
  (`unknown_did`, `bad_signature`, `malformed_activity`,
  `content_type_required`, `idempotency_key_required`,
  `invalid_idempotency_key`); registry relays equal the
  daemon document; client-certificate CA pin; journal replay
  after process restart; daemon loss keeps `/livez` 200
  (`platform/hosted/agent-boundary/tests/real_node.rs:2035-2101`)
- `persisted_submission_attempt_is_not_repeated_after_connectivity_returns`:
  LNI denied is `503` `node_unavailable` with `attempts=1`;
  restore yields `202` `unknown` without a second attempt
  (`platform/hosted/agent-boundary/tests/real_node.rs:2104-2148`)
- `real_program_simulation_executes_without_committing`:
  `committed: false`; simulated activity is not journaled;
  later commit then simulate is `422` `idempotent_replay`
  (`platform/hosted/agent-boundary/tests/real_node.rs:1813-1910`)
- `malformed_program_call_is_refused_before_a_following_send`:
  `400` `malformed_program_call` then a SEND still completes
  (`platform/hosted/agent-boundary/tests/real_node.rs:2276-2293`)
- `real_program_call_refusal_artifacts_are_bound_and_replay_after_restart`:
  refused CALL artifacts verify and replay after restart
  (`platform/hosted/agent-boundary/tests/real_node.rs:2296-2301`)
- `webhook_credential_configuration_refuses_shared_or_invalid_material`:
  shared or empty/oversize webhook token refuses boot
  (`platform/hosted/agent-boundary/tests/real_node.rs:2442-2503`)
- `real_escrow_deploy_call_upgrade_deprecate_exit_receipts_and_durable_dedup`:
  real deploy/call/upgrade/wind-down against a funded node,
  idempotent replay, registry denied on gateway paths
  (`platform/hosted/agent-boundary/tests/real_node/lifecycle.rs:593-628`)
- `maintained_program_journal_rejects_missing_corrupt_and_substituted_attachments`:
  journal artifact tamper is `503` `program_artifacts_invalid`
  (`platform/hosted/agent-boundary/tests/real_node/custody.rs:872-896`)

[Home](Home.md)

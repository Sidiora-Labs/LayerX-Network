# Hosted gateway

`layerx-gateway` is the receipt-verifying public ingress for hosted
LayerX Network (`platform/hosted/gateway/src/lib.rs:1`;
`platform/hosted/gateway/Cargo.toml:8-10`;
`platform/hosted/gateway/src/lib.rs:885-887`). The crate is
`layerx-platform-gateway`; the binary path is `src/main.rs`. The
platform string is `tls-receipt-verifying-multi-instance-hosted-gateway`
(`platform/hosted/gateway/src/lib.rs:885-887`).

It is the only hosted surface serving public `/v1` routes to humans and SDKs.
The in-cluster Human HTTPS API is Service `layerx-human` on the node pod
(`platform/hosted/human/deployment.yaml:1-12`). Qualification binds that
API as `LAYERX_QUALIFICATION_HUMAN_URL`
(`platform/hosted/tests/beta-cluster.sh:1516-1517`,
`platform/hosted/tests/beta-cluster.sh:1655`). The gateway URL is
`LAYERX_GATEWAY_URL`
(`platform/hosted/tests/beta-cluster.sh:1492`).

The image is `ghcr.io/sidiora-labs/layerx-gateway:0.1.0`, user
`4020:4020`, entrypoint `/usr/local/bin/layerx-gateway`
(`platform/hosted/gateway/Dockerfile:5-11`;
`platform/hosted/gateway/deployment.yaml:67-69`). The Deployment has
three replicas, listens on `0.0.0.0:9443`, exposes Service port `443`
to container `9443`, and is reached as Ingress host
`api.testnet.layerx.network`
(`platform/hosted/gateway/deployment.yaml:60`;
`platform/hosted/gateway/deployment.yaml:70-72`;
`platform/hosted/gateway/deployment.yaml:123-126`;
`platform/hosted/gateway/deployment.yaml:187-193`). PDB
`minAvailable` is `2`; HPA scales `3`–`30` on 65% CPU
(`platform/hosted/gateway/deployment.yaml:131`;
`platform/hosted/gateway/deployment.yaml:138-143`).

This page covers that binary, its Redis, and the tests in
`platform/hosted/gateway/`. It does not document the emulator
administration surface. `production_route` never accepts emulator
paths (`platform/hosted/gateway/src/lib.rs:804-808`;
`platform/hosted/gateway/src/lib.rs:880-881`).

---

## TLS

Inbound TLS is rustls with no client authentication. The certificate
is `LAYERX_GATEWAY_TLS_CERT_DER`; the PKCS#8 key is
`LAYERX_GATEWAY_TLS_KEY_DER`
(`platform/hosted/gateway/src/main.rs:459-478`). Each accepted TCP
connection becomes a rustls `ServerConnection`
(`platform/hosted/gateway/src/main.rs:1797-1808`). At most 256
connections are live; further accepts are shut down
(`platform/hosted/gateway/src/main.rs:40`;
`platform/hosted/gateway/src/main.rs:1816-1819`). Request bodies are
bounded at 8 MiB (`platform/hosted/gateway/src/main.rs:39`;
`platform/hosted/gateway/src/main.rs:1805-1806`).

Outbound HTTPS uses `native_tls` `TlsConnector` with
`LAYERX_GATEWAY_OUTBOUND_CA_DER`, a PKCS#12 client identity, and a
minimum protocol of TLS 1.2
(`platform/hosted/gateway/src/main.rs:484-501`;
`platform/hosted/gateway/src/http.rs:161-165`). Endpoints must be
`https://` with a DNS host, not a literal IP
(`platform/hosted/gateway/src/http.rs:23-26`;
`platform/hosted/gateway/src/http.rs:46-47`). The client writes
`Authorization`, optional `Idempotency-Key`, and optional `X-Trace-Id`
(`platform/hosted/gateway/src/http.rs:182-193`). Connect timeout is 3s;
I/O timeout is 8s (`platform/hosted/gateway/src/http.rs:8-9`).

Redis is `rediss://` only, TLS 1.2, the same outbound CA, then `AUTH`
username and password (`platform/hosted/gateway/src/store.rs:54-56`;
`platform/hosted/gateway/src/store.rs:741-767`). The Redis server
manifest sets `tls-auth-clients no`
(`platform/hosted/gateway/deployment.yaml:8-12`). Those two TLS client
auth settings differ: HTTPS upstreams present a PKCS#12 identity;
Redis does not require a client certificate.

Ingress nginx uses backend protocol HTTPS, body size `512k`, connect
timeout `3`, read timeout `10`
(`platform/hosted/gateway/deployment.yaml:181-184`).

---

## Tokens

The gateway accepts two inbound schemes and four outbound Bearer
tokens. It never forwards a caller `LayerX-Key` or a caller session
Bearer as the upstream `Authorization` value.

| Credential | Accepted from | Used as | Never |
| --- | --- | --- | --- |
| `LayerX-Key {id}:{secret}` | Production `/v1` routes and `GET /internal/v1/principal` (`platform/hosted/gateway/src/main.rs:1142-1153`; `platform/hosted/gateway/src/main.rs:1706-1718`; `platform/hosted/gateway/src/main.rs:1748`) | Local digest check against Redis (`platform/hosted/gateway/src/lib.rs:45-75`) | Sent to component, authority, identity, or registry. `Client::request` always prefixes its argument with `Bearer ` (`platform/hosted/gateway/src/http.rs:90-104`) |
| `Bearer` session | `/v1/keys` only (`platform/hosted/gateway/src/main.rs:952-961`; `platform/hosted/gateway/src/main.rs:1081-1088`) | JSON body `{"token": …}` to identity `POST /v1/sessions/introspect` (`platform/hosted/gateway/src/main.rs:962-973`) | Upstream `Authorization`. That header carries `LAYERX_GATEWAY_IDENTITY_TOKEN_FILE` |
| Component token | File `LAYERX_GATEWAY_COMPONENT_TOKEN_FILE` (`platform/hosted/gateway/src/main.rs:542`) | `Authorization: Bearer` to the agent-boundary URL | Presented by humans |
| Authority token | File `LAYERX_GATEWAY_AUTHORITY_TOKEN_FILE` (`platform/hosted/gateway/src/main.rs:547`) | `Authorization: Bearer` to the receipt-authority URL | Presented by humans |
| Identity token | File `LAYERX_GATEWAY_IDENTITY_TOKEN_FILE` (`platform/hosted/gateway/src/main.rs:552`) | `Authorization: Bearer` to the identity URL | Presented by humans |
| Registry token | File `LAYERX_GATEWAY_PROGRAM_REGISTRY_TOKEN_FILE` (`platform/hosted/gateway/src/main.rs:557`) | `Authorization: Bearer` to the program-registry URL | Presented by humans |
| Redis username and password | `LAYERX_GATEWAY_REDIS_USERNAME_FILE`, `LAYERX_GATEWAY_REDIS_PASSWORD_FILE` (`platform/hosted/gateway/src/main.rs:564-565`) | Redis `AUTH` (`platform/hosted/gateway/src/store.rs:761-767`) | HTTP |
| PKCS#12 client identity | `LAYERX_GATEWAY_CLIENT_IDENTITY_PKCS12` plus password file (`platform/hosted/gateway/src/main.rs:492-500`) | Outbound HTTPS mTLS | Inbound TLS (`with_no_client_auth` at `platform/hosted/gateway/src/main.rs:477`) |

`authenticate_gateway_key` requires prefix `LayerX-Key `, an id of at
most 64 `[A-Za-z0-9_-]` characters, and a secret `lxp_live_` plus 64
hex characters (`platform/hosted/gateway/src/lib.rs:25-26`;
`platform/hosted/gateway/src/lib.rs:49-59`;
`platform/hosted/gateway/src/lib.rs:111-117`). The store digest is
SHA-256 over length-prefixed `gateway-key-v1`, salt, and secret
(`platform/hosted/gateway/src/lib.rs:64`;
`platform/hosted/gateway/src/main.rs:1006-1014`). A disabled key or
digest mismatch is `AccessError::Unauthenticated`; a store error is
`AccessError::PersistenceUnavailable`
(`platform/hosted/gateway/src/lib.rs:33-36`;
`platform/hosted/gateway/src/lib.rs:60-73`). Persistence failure is
deliberately not an authentication refusal
(`platform/hosted/gateway/src/lib.rs:28-31`).

`Bearer` on a production route is `401 api_key_required`
(`platform/hosted/gateway/tests/hosted-boundary.sh:37`;
`platform/hosted/gateway/tests/tap_nonce.rs:290-291`).

Headers `x-layerx-principal` and `x-layerx-api-key` are refused as
`400 untrusted_identity_header` before authentication
(`platform/hosted/gateway/src/main.rs:1696-1704`;
`platform/hosted/gateway/tests/hosted-boundary.sh:44`). Incoming
headers and bodies are zeroized on drop
(`platform/hosted/gateway/src/http.rs:215-221`). Audit events hash
principal digest, action, subject, and outcome; payloads and
credentials never enter the audit stream
(`platform/hosted/gateway/src/lib.rs:90-92`;
`platform/hosted/gateway/src/main.rs:1070-1078`).

`Client::request_authorized` exists so another hosted ingress can
present an already validated `LayerX-Key` value to this gateway
(`platform/hosted/gateway/src/http.rs:107-120`). The gateway binary
itself calls `Client::request` with the four service tokens
(`platform/hosted/gateway/src/main.rs:927-949`).

---

## Scopes

Issued keys carry a sorted, non-empty list of at most six scopes
(`platform/hosted/gateway/src/main.rs:1025-1045`):

| Scope | Routes |
| --- | --- |
| `activity:write` | `POST /v1/activities` |
| `program:call` | `POST /v1/programs/call`, `/deploy`, `/upgrade`, `/wind-down` |
| `program:simulate` | `POST /v1/programs/simulate` |
| `program:read` | `GET /v1/programs/registry/{id}`, `/interface`, `/activities/{id}`, `/receipts/by-idempotency/{key}` |
| `receipt:read` | `GET /v1/receipts/{id}` |
| `state:read` | `GET /v1/state` |

`permits` is exact string match on the comma-joined record
(`platform/hosted/gateway/src/main.rs:1052-1068`). Missing scope is
`403 insufficient_scope` (`platform/hosted/gateway/src/main.rs:1758-1764`).

Quota is a finite fixed window: requests `1..=1_000_000`, window
`1..=2_592_000` seconds (`platform/hosted/gateway/src/lib.rs:164-171`).
Writes consume quota inside Redis `reserve`; reads consume it in
`consume_read` (`platform/hosted/gateway/src/store.rs:816-850`).

---

## Public routes

`production_route` is the production path set shared with the emulator
(`platform/hosted/gateway/src/lib.rs:809-881`). Program identifiers in
that parser are 64 lowercase hex characters; `GET /v1/receipts/{id}`
allows any ASCII hex digit, including `A-F`
(`platform/hosted/gateway/src/lib.rs:825-828`;
`platform/hosted/gateway/src/lib.rs:874`). Those two identifier
alphabets differ.

Unauthenticated and key-management routes are dispatched before
`production_route` (`platform/hosted/gateway/src/main.rs:1693-1739`).

| Method and path | Inputs | Upstream |
| --- | --- | --- |
| `GET /livez` | none | none. Body `status=live`, `service=layerx-gateway`, `package_semver` (`platform/hosted/gateway/src/main.rs:1721-1729`) |
| `GET /readyz` | none | Redis `PING`; component, authority `GET /readyz`; registry `GET /healthz` (`platform/hosted/gateway/src/main.rs:1640-1691`; `platform/hosted/gateway/src/main.rs:2779-2811`) |
| `GET /v1/status` | none | same Redis/component/authority probes, no registry (`platform/hosted/gateway/src/main.rs:2814-2841`) |
| `POST /v1/keys` | `Authorization: Bearer`, `Idempotency-Key`, JSON `signer_public_key`, `scopes`, `quota_requests`, `quota_window_seconds` (`platform/hosted/gateway/src/main.rs:76-82`; `platform/hosted/gateway/src/main.rs:2274-2300`) | identity `POST /v1/sessions/introspect`; Redis `issue_key`. Secret is derived, not stored (`platform/hosted/gateway/src/lib.rs:220-228`; `platform/hosted/gateway/src/main.rs:2301-2348`) |
| `GET /v1/keys` | Bearer session | identity introspect; Redis `list_keys` (`platform/hosted/gateway/src/main.rs:1090-1091`; `platform/hosted/gateway/src/main.rs:2351-2382`) |
| `DELETE /v1/keys/{id}` | Bearer session | identity introspect; Redis `revoke_key` (`platform/hosted/gateway/src/main.rs:1114-1126`) |
| `POST /v1/keys/{id}/rotate` | Bearer session, `Idempotency-Key` | identity introspect; Redis `rotate_key` (`platform/hosted/gateway/src/main.rs:1128-1137`; `platform/hosted/gateway/src/main.rs:2385-2454`) |
| `GET /internal/v1/principal` | `LayerX-Key` | Redis key lookup. Body `principal_digest` only (`platform/hosted/gateway/src/main.rs:1706-1718`) |
| `POST /v1/activities` | `LayerX-Key`, `Idempotency-Key`, `application/json` `{activity}` or `application/octet-stream` signed bytes (`platform/hosted/gateway/src/main.rs:3068-3117`) | agent-boundary `POST /v1/activities` as `application/octet-stream` with the component token and protocol idempotency key (`platform/hosted/gateway/src/main.rs:3193-3206`). Then authority `GET /v1/authorized-batches/by-activity/{id}` (`platform/hosted/gateway/src/main.rs:1156-1186`) |
| `GET /v1/state` | `LayerX-Key` and `state:read` | none. Always `503 principal_state_proof_unavailable` before quota (`platform/hosted/gateway/src/main.rs:1572-1573`; `platform/hosted/gateway/src/main.rs:1611`) |
| `GET /v1/receipts/{id}` | `LayerX-Key`, `receipt:read` | Redis `activity_owner`; agent-boundary `GET /v1/receipts/{id}`; authority by-activity (`platform/hosted/gateway/src/main.rs:2457-2513`) |
| `POST /v1/programs/call` | `LayerX-Key`, `program:call`, `Idempotency-Key` hex32, JSON or octet-stream Programs CALL (`platform/hosted/gateway/src/main.rs:303-390`; `platform/hosted/gateway/src/main.rs:1766-1771`) | registry `GET /v1/programs/registry/{program}`; agent-boundary `POST /v1/programs/call`; authority by-activity |
| `POST /v1/programs/simulate` | same call body, `program:simulate` | registry head; agent-boundary `POST /v1/programs/simulate` (`platform/hosted/gateway/src/main.rs:1250-1329`) |
| `POST /v1/programs/deploy` | `program:call`, octet-stream only, Programs ordinal 1 (`platform/hosted/gateway/src/program_lifecycle.rs:7-13`; `platform/hosted/gateway/src/main.rs:3084-3101`) | agent-boundary `POST /v1/programs/deploy`; authority by-activity |
| `POST /v1/programs/upgrade` | ordinal 2, octet-stream | agent-boundary `POST /v1/programs/upgrade`; authority |
| `POST /v1/programs/wind-down` | ordinal 7, octet-stream | agent-boundary `POST /v1/programs/wind-down`; authority |
| `GET /v1/programs/registry/{id}` | `program:read`, JSON selector `program_id` plus `requested_verification_level=sequencer-signed` (`platform/hosted/gateway/src/main.rs:847-861`) | registry `GET /v1/programs/registry/{id}` (`platform/hosted/gateway/src/main.rs:393-429`; `platform/hosted/gateway/src/main.rs:2516-2548`) |
| `GET /v1/programs/registry/{id}/interface` | same selector | registry head and `GET …/interface` (`platform/hosted/gateway/src/main.rs:2551-2581`) |
| `GET /v1/programs/activities/{id}` | JSON selector `activity_id` plus `sequencer-signed` (`platform/hosted/gateway/src/main.rs:886-901`) | Redis owner and operation; pending calls agent-boundary `GET /v1/programs/activities/{id}` (`platform/hosted/gateway/src/main.rs:2638-2688`; `platform/hosted/gateway/src/main.rs:3568-3572`) |
| `GET /v1/programs/receipts/by-idempotency/{key}` | JSON selector `idempotency_key`, `expected_activity_id`, `sequencer-signed` (`platform/hosted/gateway/src/main.rs:864-883`) | Redis operation; pending lifecycle uses agent-boundary `GET /v1/programs/receipts/by-idempotency/{key}` (`platform/hosted/gateway/src/main.rs:1460-1467`; `platform/hosted/gateway/src/main.rs:2584-2635`) |

The component URL in the hosted manifest is the agent boundary, not
the core Service (`platform/hosted/gateway/deployment.yaml:78`):

`https://layerx-agent-boundary.layerx-testnet.svc.cluster.local:9443`

Authority is
`https://layerx-receipt-authority.layerx-testnet.svc.cluster.local:9443`
(`platform/hosted/gateway/deployment.yaml:80`). Identity is
`https://layerx-identity.layerx-testnet.svc.cluster.local:9443`
(`platform/hosted/gateway/deployment.yaml:82`). Registry is
`https://layerx-program-registry.layerx-testnet.svc.cluster.local:9420`
(`platform/hosted/gateway/deployment.yaml:84`). Readiness labels the
component probe `core_agent_boundary`
(`platform/hosted/gateway/src/main.rs:2805`). The gateway has no core
URL of its own.

Program GET/simulate/error responses are wrapped in the agent envelope
unless the request is a successful `POST` call or lifecycle mutation
(`platform/hosted/gateway/src/main.rs:749-825`;
`platform/hosted/gateway/src/main.rs:1777-1785`).

Unknown `production_route` values are `404 not_found`
(`platform/hosted/gateway/src/main.rs:1740-1746`).
`GET /__emulator/reset` is 404
(`platform/hosted/gateway/tests/hosted-boundary.sh:36`).

---

## Config keys

| Key | Role |
| --- | --- |
| `LAYERX_GATEWAY_LISTEN` | Bind address; default `0.0.0.0:9443` (`platform/hosted/gateway/src/main.rs:532-535`; `platform/hosted/gateway/deployment.yaml:72`) |
| `LAYERX_GATEWAY_TLS_CERT_DER` | Inbound server certificate DER |
| `LAYERX_GATEWAY_TLS_KEY_DER` | Inbound PKCS#8 key DER |
| `LAYERX_GATEWAY_OUTBOUND_CA_DER` | Trust bundle for HTTPS and Redis |
| `LAYERX_GATEWAY_CLIENT_IDENTITY_PKCS12` | Outbound client identity |
| `LAYERX_GATEWAY_CLIENT_IDENTITY_PASSWORD_FILE` | PKCS#12 password |
| `LAYERX_GATEWAY_COMPONENT_URL` | Agent-boundary HTTPS origin |
| `LAYERX_GATEWAY_COMPONENT_TOKEN_FILE` | Bearer to the agent boundary |
| `LAYERX_GATEWAY_AUTHORITY_URL` | Receipt-authority HTTPS origin |
| `LAYERX_GATEWAY_AUTHORITY_TOKEN_FILE` | Bearer to authority |
| `LAYERX_GATEWAY_IDENTITY_URL` | Identity HTTPS origin |
| `LAYERX_GATEWAY_IDENTITY_TOKEN_FILE` | Bearer to identity |
| `LAYERX_GATEWAY_PROGRAM_REGISTRY_URL` | Program-registry HTTPS origin |
| `LAYERX_GATEWAY_PROGRAM_REGISTRY_TOKEN_FILE` | Bearer to registry |
| `LAYERX_GATEWAY_REDIS_URL` | `rediss://` origin |
| `LAYERX_GATEWAY_REDIS_USERNAME_FILE` | Redis ACL user |
| `LAYERX_GATEWAY_REDIS_PASSWORD_FILE` | Redis ACL password |
| `LAYERX_GATEWAY_SEQUENCER_PUBLIC_KEY_FILE` | Pinned 32-byte hex sequencer key (`platform/hosted/gateway/src/main.rs:502-503`) |
| `LAYERX_GATEWAY_KEY_PROVISIONING_KEY_FILE` | 32-byte hex used to derive issued secrets (`platform/hosted/gateway/src/main.rs:504-505`; `platform/hosted/gateway/src/lib.rs:220-228`) |
| `LAYERX_GATEWAY_NETWORK_ID` | Hosted network identifier; must match authority facts (`platform/hosted/gateway/src/main.rs:513-518`; `platform/hosted/gateway/src/main.rs:1171-1175`) |
| `LAYERX_GATEWAY_LXP_WIRE_VERSION` | Numeric wire version; must equal `STATE_COMMITMENT_PROTOCOL_VERSION` (`platform/hosted/gateway/src/main.rs:515-524`) |
| `LAYERX_GATEWAY_PROTOCOL_NETWORK_ID` | `u32` checked on signed activities (`platform/hosted/gateway/src/main.rs:526-529`) |
| `LAYERX_GATEWAY_MODULE_REGISTRY_FILE` | JSON module ordinals (`platform/hosted/gateway/src/main.rs:2231-2271`) |
| `LAYERX_GATEWAY_IDEMPOTENCY_SECONDS` | Retention `3600..=2592000`; default `604800` (`platform/hosted/gateway/src/main.rs:41`; `platform/hosted/gateway/src/main.rs:506-511`; `platform/hosted/gateway/deployment.yaml:95`) |

The testnet Deployment pins `LAYERX_GATEWAY_NETWORK_ID=layerx-testnet`,
`LAYERX_GATEWAY_LXP_WIRE_VERSION=3`,
`LAYERX_GATEWAY_PROTOCOL_NETWORK_ID=402`
(`platform/hosted/gateway/deployment.yaml:91-93`).

---

## Redis state and ACL

The gateway owns StatefulSet `layerx-gateway-redis`: Redis 8.2.1,
TLS port 6379, `appendonly yes`, `appendfsync always`,
`maxmemory-policy noeviction`, `protected-mode yes`, ACL file
`/run/layerx/auth/users.acl` (`platform/hosted/gateway/deployment.yaml:5-17`;
`platform/hosted/gateway/deployment.yaml:20-49`). NetworkPolicy admits
TCP 6379 only from pods `app=layerx-gateway` and allows no egress
(`platform/hosted/gateway/deployment.yaml:147-154`).

Cluster bring-up writes ACL `user default off` and one enabled user
`layerx-gateway` with `~* &* +@all`
(`platform/hosted/tests/beta-cluster.sh:473-475`;
`platform/hosted/tests/beta-cluster.sh:497-499`). The TAP portable
test uses the same `default off` plus one enabled user shape
(`platform/hosted/gateway/tests/tap_nonce.rs:76-80`).

| Key | Contents |
| --- | --- |
| `gateway:key:{id}` | Hash: `principal`, `salt`, `secret_digest`, `signer_public_key`, `scopes`, `quota_requests`, `quota_window_seconds`, `epoch`, `disabled` (`platform/hosted/gateway/src/store.rs:186`; `platform/hosted/gateway/src/store.rs:781-790`) |
| `gateway:principal:{digest}:keys` | Set of key ids, cap 128 (`platform/hosted/gateway/src/store.rs:41`; `platform/hosted/gateway/src/store.rs:251-268`; `platform/hosted/gateway/src/store.rs:785`) |
| `gateway:quota:{key_id}:{window}` | INCR usage with EXPIRE remaining window (`platform/hosted/gateway/src/store.rs:373-374`; `platform/hosted/gateway/src/store.rs:831`) |
| `gateway:idem:{scope}` | Operation hash: digest, state, response, receipt, principal, activity_id, idempotency_key, continuation chunks (`platform/hosted/gateway/src/store.rs:375`; `platform/hosted/gateway/src/store.rs:832-834`) |
| `gateway:pending` | Set of pending idempotency keys (`platform/hosted/gateway/src/store.rs:835`) |
| `gateway:activity:{activity_id}` | Owner principal digest (`platform/hosted/gateway/src/store.rs:376`; `platform/hosted/gateway/src/store.rs:566-574`) |
| `gateway:activity-operation:{activity_id}` | Pointer into `gateway:idem:…` (`platform/hosted/gateway/src/store.rs:377`; `platform/hosted/gateway/src/store.rs:579-610`) |
| `gateway:audit` / `gateway:audit:head` | Hash-chained `XADD` stream (`platform/hosted/gateway/src/store.rs:724-737`; `platform/hosted/gateway/src/store.rs:788`) |
| `gateway:tap:nonce:{scope}` | One-use TAP nonce (`platform/hosted/gateway/src/store.rs:635`; `platform/hosted/gateway/src/store.rs:868-890`) |
| `gateway:tap:binding:{digest}` | TAP credential binding (`platform/hosted/gateway/src/store.rs:636`; `platform/hosted/gateway/src/store.rs:686-721`) |

Continuation of a signed program activity is stored in 128 KiB chunks,
at most 16 chunks / 2 MiB (`platform/hosted/gateway/src/store.rs:37-39`;
`platform/hosted/gateway/src/store.rs:1143-1184`).

---

## Callers the NetworkPolicy admits

`topology-check.sh` includes `platform/hosted/gateway/deployment.yaml`
(`platform/hosted/tests/topology-check.sh:22`;
`platform/hosted/tests/topology-check.sh:86`). Gateway ingress admits
TCP 9443 from namespace `ingress-nginx` or from pods
`app=layerx-testnet-control`
(`platform/hosted/gateway/deployment.yaml:162-164`). Egress is Redis
6379, program-registry 9420, `layerx-plane: trusted-boundary` on 9443 /
9445 / 9446, and DNS 53 (`platform/hosted/gateway/deployment.yaml:165-173`).

---

## Typed refusals

HTTP refusals use `{"ok": false, "error": {"code": …}}`
(`platform/hosted/gateway/src/main.rs:578-585`). Library errors are
`GatewayError` and `AccessError`.

| HTTP | Code | Condition |
| --- | --- | --- |
| 400 | `untrusted_identity_header` | `x-layerx-principal` or `x-layerx-api-key` present (`platform/hosted/gateway/src/main.rs:1696-1699`) |
| 400 | `invalid_http_request` | Request framing failed (`platform/hosted/gateway/src/main.rs:1805-1806`) |
| 400 | `idempotency_key_required` | Missing or malformed `Idempotency-Key` on key issue, rotation, or activity (`platform/hosted/gateway/src/main.rs:2281-2284`; `platform/hosted/gateway/src/main.rs:3068-3078`) |
| 400 | `invalid_key_request` | `IssueRequest` JSON rejected (`platform/hosted/gateway/src/main.rs:2285-2286`) |
| 400 | `invalid_quota` | `Quota::new` rejected (`platform/hosted/gateway/src/main.rs:2295-2296`) |
| 400 | `invalid_scopes` | Empty, unsorted, unknown, or more than six scopes (`platform/hosted/gateway/src/main.rs:2298-2299`; `platform/hosted/gateway/src/main.rs:1025-1045`) |
| 400 | `invalid_activity` | JSON activity hex failed (`platform/hosted/gateway/src/main.rs:3111-3116`) |
| 400 | `invalid_program_call` | Program call body or signed CALL failed (`platform/hosted/gateway/src/main.rs:1256-1257`; `platform/hosted/gateway/src/main.rs:3103-3106`) |
| 400 | `invalid_program_lifecycle` | Lifecycle validate failed (`platform/hosted/gateway/src/main.rs:3099-3100`) |
| 400 | `invalid_program_selector` | Discovery GET body failed (`platform/hosted/gateway/src/main.rs:1577-1578`) |
| 400 | `invalid_program_receipt_selector` | Receipt GET body failed (`platform/hosted/gateway/src/main.rs:1584-1585`) |
| 400 | `invalid_program_activity_selector` | Activity GET body failed (`platform/hosted/gateway/src/main.rs:1589-1590`) |
| 400 | `invalid_program_id` | Path program id is not 32-byte hex (`platform/hosted/gateway/src/main.rs:2517-2518`) |
| 401 | `session_required` | Missing/inactive Bearer session or identity non-200 (`platform/hosted/gateway/src/main.rs:961`; `platform/hosted/gateway/src/main.rs:975-989`) |
| 401 | `api_key_required` | Missing `LayerX-Key`, format failure, digest mismatch, disabled key, or Redis `Reservation::Revoked` (`platform/hosted/gateway/src/main.rs:1148-1152`; `platform/hosted/gateway/src/main.rs:3276`) |
| 403 | `insufficient_scope` | Authenticated key lacks the route scope (`platform/hosted/gateway/src/main.rs:1758-1759`) |
| 403 | `signer_not_owned` | Issue/rotate signer is not in the session allow-list (`platform/hosted/gateway/src/main.rs:2288-2293`; `platform/hosted/gateway/src/main.rs:2394-2399`) |
| 403 | `activity_authorization_refused` | `verify_submission` failed (`platform/hosted/gateway/src/main.rs:1262-1269`; `platform/hosted/gateway/src/main.rs:3123-3130`) |
| 404 | `not_found` | Unknown path, unknown key id for this principal, or write route reached via `read_route` (`platform/hosted/gateway/src/main.rs:1094`; `platform/hosted/gateway/src/main.rs:1631-1636`; `platform/hosted/gateway/src/main.rs:1740-1741`) |
| 404 | `receipt_not_found` | No owner, owner mismatch, or component 404 (`platform/hosted/gateway/src/main.rs:2467-2493`) |
| 404 | `unknown_program` | Registry 404 (`platform/hosted/gateway/src/main.rs:412-413`) |
| 404 | `program_interface_absent` | Registry interface 404 (`platform/hosted/gateway/src/main.rs:2572-2573`) |
| 404 | `program_receipt_not_found` | No matching idempotency operation for this principal (`platform/hosted/gateway/src/main.rs:2604-2608`) |
| 404 | `program_activity_not_found` | No owner or operation for this principal (`platform/hosted/gateway/src/main.rs:2646-2661`) |
| 409 | `idempotency_conflict` | Same idempotency, different request digest, or key-issue collision (`platform/hosted/gateway/src/main.rs:2328-2330`; `platform/hosted/gateway/src/main.rs:3286-3292`) |
| 409 | `rotation_conflict` | Rotate neither wrote nor replayed (`platform/hosted/gateway/src/main.rs:2452-2453`) |
| 409 | `protocol_idempotency_mismatch` | Header key ≠ signed activity idempotency (`platform/hosted/gateway/src/main.rs:3132-3133`) |
| 409 | `program_not_active` | Registry lifecycle is not `active` (`platform/hosted/gateway/src/main.rs:1275-1276`; `platform/hosted/gateway/src/main.rs:3137-3138`) |
| 409 | `program_receipt_selector_mismatch` | Selector `expected_activity_id` ≠ stored activity (`platform/hosted/gateway/src/main.rs:2610-2611`) |
| 409 | `program_call_refused` | Stored operation state is not `pending` or `completed` (`platform/hosted/gateway/src/main.rs:2616-2617`; `platform/hosted/gateway/src/main.rs:2676-2677`) |
| 409 | `activity_refused` | Non-program activity verified with `result_code != 0` (`platform/hosted/gateway/src/main.rs:3485-3498`) |
| 415 | `activity_content_type_required` | Missing/unsupported type or empty body (`platform/hosted/gateway/src/main.rs:3095-3096`) |
| 429 | `quota_exceeded` | Redis `rate_limited` (`platform/hosted/gateway/src/main.rs:1294`; `platform/hosted/gateway/src/main.rs:1607`; `platform/hosted/gateway/src/main.rs:3277-3279`) |
| 502 | `lifecycle_binding_invalid` | Pending lifecycle continuation does not re-verify (`platform/hosted/gateway/src/main.rs:1458`; `platform/hosted/gateway/src/main.rs:1487-1488`; `platform/hosted/gateway/src/main.rs:1556-1557`) |
| 502 | `receipt_verification_failed` | Lifecycle receipt/authority mismatch (`platform/hosted/gateway/src/main.rs:1497-1501`; `platform/hosted/gateway/src/main.rs:3427-3441`) |
| 502 | `component_invalid` | Pending lifecycle component status/body invalid (`platform/hosted/gateway/src/main.rs:1479-1483`) |
| 503 | `persistence_unavailable` | Redis error (`platform/hosted/gateway/src/main.rs:1152`; `platform/hosted/gateway/src/main.rs:1608`) |
| 503 | `identity_unavailable` | Introspect encode/decode failure (`platform/hosted/gateway/src/main.rs:964`; `platform/hosted/gateway/src/main.rs:978`) |
| 503 | `component_unavailable` | Outbound request to the agent boundary failed (`platform/hosted/gateway/src/main.rs:949`; `platform/hosted/gateway/src/main.rs:1307-1308`) |
| 503 | `component_invalid` | Component status/body/activity mismatch (`platform/hosted/gateway/src/main.rs:1310-1314`; `platform/hosted/gateway/src/main.rs:3361-3376`) |
| 503 | `authority_unavailable` | Authority non-200 (`platform/hosted/gateway/src/main.rs:1166-1167`) |
| 503 | `authority_invalid` | Authority JSON/hex failed (`platform/hosted/gateway/src/main.rs:1169-1185`) |
| 503 | `authority_mismatch` | Activity id, network id, or wire version mismatch (`platform/hosted/gateway/src/main.rs:1171-1175`) |
| 503 | `receipt_verification_failed` | `verify_activity_operation` failed (`platform/hosted/gateway/src/main.rs:1199-1205`) |
| 503 | `program_receipt_verification_failed` | `verify_program_operation` failed (`platform/hosted/gateway/src/main.rs:1231-1242`) |
| 503 | `program_simulation_unverified` | Simulation receipt or evidence failed (`platform/hosted/gateway/src/main.rs:2907-2908`; `platform/hosted/gateway/src/main.rs:2927-3001`) |
| 503 | `program_registry_unavailable` | Registry transport failed (`platform/hosted/gateway/src/main.rs:411`; `platform/hosted/gateway/src/main.rs:2569-2570`) |
| 503 | `program_registry_invalid` | Registry JSON/head failed (`platform/hosted/gateway/src/main.rs:415-418`; `platform/hosted/gateway/src/main.rs:2149-2187`) |
| 503 | `program_registry_unverified` | Missing `receipt-verified` or interface digest mismatch (`platform/hosted/gateway/src/main.rs:421-427`; `platform/hosted/gateway/src/main.rs:2729-2753`) |
| 503 | `program_state_unverified` | Head clock/state fields missing or stale (`platform/hosted/gateway/src/main.rs:2206-2215`) |
| 503 | `program_simulation_head_unavailable` | Active program lacks state root/sequence (`platform/hosted/gateway/src/main.rs:1278-1281`) |
| 503 | `principal_state_proof_unavailable` | `GET /v1/state` (`platform/hosted/gateway/src/main.rs:1572-1573`) |
| 503 | `operation_state_unknown` | Existing reservation state is not pending/completed/refused_* (`platform/hosted/gateway/src/main.rs:3324-3325`) |
| 503 | `receipt_encoding_failed` | Verified JSON could not be stored (`platform/hosted/gateway/src/main.rs:3523-3533`) |

`502 receipt_verification_failed` (lifecycle) and
`503 receipt_verification_failed` (ordinary activity) are different
status codes for the same code string
(`platform/hosted/gateway/src/main.rs:1205`;
`platform/hosted/gateway/src/main.rs:3435`).

Library `GatewayError` (`platform/hosted/gateway/src/lib.rs:921-935`):

| Variant | Raised when |
| --- | --- |
| `Unauthenticated` | Display string only; HTTP uses `AccessError` instead (`platform/hosted/gateway/src/lib.rs:939-940`) |
| `Forbidden` | Submission Ed25519 verifier is not the key's signer (`platform/hosted/gateway/src/lib.rs:450`) |
| `InvalidRequest` | Canonical decode, protocol/network, encoding, or transfer disclosure failed (`platform/hosted/gateway/src/lib.rs:431-436`; `platform/hosted/gateway/src/lib.rs:388-414`) |
| `InvalidRoute` | `production_route` miss (`platform/hosted/gateway/src/lib.rs:808`; `platform/hosted/gateway/src/lib.rs:880-881`) |
| `IdempotencyConflict` | Display string; HTTP uses `409 idempotency_conflict` |
| `VerificationRequired` | Missing receipt digest or program verify failed (`platform/hosted/gateway/src/lib.rs:538`; `platform/hosted/gateway/src/lib.rs:604`) |
| `UntrustedSequencer` | Authority sequencer key ≠ pinned key (`platform/hosted/gateway/src/lib.rs:518-524`; `platform/hosted/gateway/src/lib.rs:585-591`) |
| `ActivityMismatch` | Receipt activity id ≠ expected (`platform/hosted/gateway/src/lib.rs:532-533`) |
| `Receipt(ReceiptCheck)` | `verify_outcome` failed (`platform/hosted/gateway/src/lib.rs:526-527`) |
| `Encoding` | JSON render failed (`platform/hosted/gateway/src/lib.rs:547`) |
| `Entropy` | `getrandom` failed during `IssuedKey::generate` (`platform/hosted/gateway/src/lib.rs:209-210`) |
| `Unavailable` | Display string for a missing dependency |

`verify_submission` returns `Forbidden` for a wrong signer and
`InvalidRequest` for wire/protocol failures
(`platform/hosted/gateway/src/lib.rs:424-456`). HTTP maps both to
`403 activity_authorization_refused`
(`platform/hosted/gateway/src/main.rs:3123-3130`).

---

## Receipt verification before success

A `VerifiedOperation` cannot be constructed from a status word and
bytes (`platform/hosted/gateway/src/lib.rs:292-294`). Public activity
JSON is rendered only from verified receipt fields
(`platform/hosted/gateway/src/lib.rs:506-508`;
`platform/hosted/gateway/src/lib.rs:539-547`).

Ordinary activities: decode receipt hex, load `AuthorityFacts` from
authority, require the authority sequencer key to match
`LAYERX_GATEWAY_SEQUENCER_PUBLIC_KEY_FILE`, run `verify_outcome` on
the authorized batch, require the receipt activity id, then complete
Redis as `receipt_verified` (`platform/hosted/gateway/src/main.rs:1189-1210`;
`platform/hosted/gateway/src/lib.rs:512-555`;
`platform/hosted/gateway/src/main.rs:3535-3554`). Non-zero
`result_code` on a non-program activity is stored as `refused_409`
with the verified receipt still present
(`platform/hosted/gateway/src/main.rs:3485-3520`).

Program CALL: `verify_program_operation` checks authorized program
execution against the registry head's program id and guest ABI, plus
terminal payload and call graph
(`platform/hosted/gateway/src/lib.rs:572-613`;
`platform/hosted/gateway/src/main.rs:1213-1247`).

Program simulation: `verify_program_simulation_operation` against the
pinned previous state root, then Ed25519 over
`LayerX/agent/program-simulation-evidence/v1` with
`committed: false` (`platform/hosted/gateway/src/lib.rs:623-657`;
`platform/hosted/gateway/src/main.rs:2896-3001`).

Lifecycle deploy/upgrade/wind-down: `program_lifecycle::verify_receipt`
checks sequencer signature, protocol 3, module 9, module version 4,
operation 0, absent program outcome, and on `result_code == 0` a
program state proof (`platform/hosted/gateway/src/program_lifecycle.rs:69-94`;
`platform/hosted/gateway/src/main.rs:3423-3435`).

Component HTTP 202 or transport failure returns `202` with
`state: unknown` and `retained_signed_activity`; that is not a
verified success (`platform/hosted/gateway/src/main.rs:1383-1401`;
`platform/hosted/gateway/src/main.rs:3207-3221`).

---

## Readiness

Gateway `GET /readyz` is ready only when Redis `PING` is `PONG`, the
agent-boundary `/readyz` is 200 JSON with `ready`, matching
`network_id` and `wire_version`, and `synchronous_receipts` plus
`state_snapshot`, the authority `/readyz` is 200 JSON with `ready` and
matching network/wire (route flags not required), and registry
`GET /healthz` is 200 JSON `status=ready`, `service=program-registry`
(`platform/hosted/gateway/src/main.rs:1640-1691`;
`platform/hosted/gateway/src/main.rs:2779-2811`). Identity is not a
`/readyz` component. `principal_state_boundary` is always the string
`unavailable` (`platform/hosted/gateway/src/main.rs:2808`).

Probes: Deployment `readinessProbe` HTTPS `/readyz` every 5s,
`livenessProbe` HTTPS `/livez` every 15s
(`platform/hosted/gateway/deployment.yaml:96-97`).

Testnet control probes the gateway with unauthenticated `GET /readyz`
and requires HTTP 200 (`platform/hosted/testnet/src/main.rs:1032-1037`;
`platform/hosted/testnet/src/main.rs:1104`). Payment, receipt
inspection, and programs journeys list `Dependency::Gateway`
(`platform/hosted/testnet/src/main.rs:251-267`). Funding does not.

`GET /v1/status` reports `hosted_gateway` as `degraded` when Redis is
ready and `unavailable` when it is not; it never reports
`available` for that field (`platform/hosted/gateway/src/main.rs:2814-2835`).
Testnet public status names the gateway as one of four components
`testnet`, `gateway`, `core`, `paxeer`
(`platform/hosted/testnet/src/main.rs:1647`). Those two status shapes
differ.

---

## Tests

Portable crate tests (no cluster):

| Test | Proves |
| --- | --- |
| `issued_key_debug_redacts_the_credential` | `IssuedKey` debug prints `[REDACTED]`, not the secret (`platform/hosted/gateway/src/lib.rs:962-969`) |
| `production_program_routes_are_exact_and_bounded` | Exact nine program routes; uppercase and `..` path ids are `InvalidRoute` (`platform/hosted/gateway/src/lib.rs:972-1033`) |
| `continuation_tests` | 2 MiB activity hex chunks and rejects one extra byte (`platform/hosted/gateway/src/store.rs:1211-1232`) |
| `agent_program_error_envelope_is_exact` | `409 idempotency_conflict` → class `IdempotencyConflict`, `retriability=Terminal` (`platform/hosted/gateway/src/main.rs:2037-2053`) |
| `program_error_classes_are_stable_for_provider_parity` | `quota_exceeded`→`RateLimit`; `activity_authorization_refused`→`PolicyRefusal`; `program_receipt_verification_failed`→`VerificationFailure`; `404 program_interface_absent`→`UnavailableCapability`; `LXP_ERR_*`→`CoreRejection`; 503→`TransportFailure` (`platform/hosted/gateway/src/main.rs:2056-2074`; `platform/hosted/gateway/src/main.rs:611-642`) |
| `program_get_selectors_bind_every_identity_field` | Selector identity fields are exact (`platform/hosted/gateway/src/main.rs:2106-2141`) |
| `exact_native_json_and_scope_bindings` | Native JSON must match the signed payload field-for-field (`platform/hosted/gateway/src/native_call.rs:153-187`) |
| `lifecycle_signed_activity_binds_exact_route_protocol_and_payload` | Wrong signer, network, ordinal, hash, or trailing byte is refused (`platform/hosted/gateway/src/program_lifecycle.rs:150-204`) |
| `exact_pending_retry_survives_reconstruction_but_altered_nonce_reuse_is_replay` | Real Redis TAP nonce: identical retry is `AlreadyConsumed`; altered operation/path is `Replay` (`platform/hosted/gateway/tests/tap_nonce.rs:159-247`) |
| `principal_binding_comes_only_from_the_authenticated_durable_key_record` | Empty, `Bearer`, wrong secret, and revoked key are `authenticate_gateway_key` errors; principal digest is the stored record (`platform/hosted/gateway/tests/tap_nonce.rs:250-305`) |

Hosted boundary script, against a real gateway URL
(`platform/hosted/gateway/tests/hosted-boundary.sh`):

- `POST /v1/activities` with `LayerX-Key` returns `ok` and a receipt
  that `LAYERX_RECEIPT_VERIFY_BIN` accepts
  (`platform/hosted/gateway/tests/hosted-boundary.sh:18-34`)
- `GET /__emulator/reset` is 404 (`:36`)
- `Authorization: Bearer {secret}` on `/v1/state` is 401 (`:37`)
- Conflicting activity under the same idempotency key is 409 (`:38`)
- `GET /internal/v1/principal` is 200 with a 64-hex digest; missing
  auth is 401; `X-LayerX-Principal` is 400 (`:40-44`)

Lifecycle boundary script
(`platform/hosted/gateway/tests/lifecycle-boundary.sh`):

- Unauthorized POST is 401 (`:25-26`)
- JSON content type is 415 (`:27-28`)
- Missing idempotency key is 400 (`:29-30`)
- Flipped signature byte is 403 (`:31-40`)
- First submit and replay compare equal JSON results; receipt verifies
  offline (`:41-51`)

`tests/local/lifecycle.rs` runs that script against real
`layerx-gateway`, identity, receipt-authority, TLS Redis, and core
boundary processes. Unrelated signer issue is 403; exact issuance
replay is 200 with byte-identical key JSON; an unrelated TLS root
cannot complete `/livez`
(`platform/hosted/gateway/tests/local/lifecycle.rs:463-496`;
`platform/hosted/gateway/tests/local/README.md:8-9`). The offline
verifier requires protocol 3, module 9 version 4, operation 0, success,
absent call outcome (`platform/hosted/gateway/tests/local/verifier.rs:24-34`;
`platform/hosted/gateway/tests/local/README.md:40-41`).

`tests/load.js` issues a key with a Bearer session and submits the
signed corpus to `POST /v1/activities`, requiring status 200 and a
receipt (`platform/hosted/gateway/tests/load.js:37-72`).

---

## Make targets

`platform/Makefile.inc` has no `platform-test-gateway` target. Gateway
inputs appear here:

| Target | Gateway use |
| --- | --- |
| `platform-test` | `cargo test` of the platform workspace, including `layerx-platform-gateway` (`platform/Makefile.inc:112-113`) |
| `platform-build` | workspace build (`platform/Makefile.inc:90-91`) |
| `platform-hosted-smoke` | requires `LAYERX_GATEWAY_URL` (`platform/Makefile.inc:161-173`) |
| `platform-hosted-topology-check` | `topology-check.sh`, default manifests include the gateway (`platform/Makefile.inc:177-178`; `platform/hosted/tests/topology-check.sh:22`) |
| `platform-test-agent-install` | requires `LAYERX_GATEWAY_URL` (`platform/Makefile.inc:190-201`) |

[Home](Home.md)

## Version-2 module registry

`beta-cluster.sh` runs `/usr/local/bin/layerx-module-registry generate` from the
node image with networking disabled and a read-only filesystem. It
reads `ASSET_SYMBOL`, `ASSET_CURRENCY`, and `ASSET_DECIMALS` from
`platform/hosted/node/bootstrap.sh`, supplies the node network and asset id,
and passes `--custody-profile` when configured. The producer links the daemon's
actual Asset and Programs interfaces and includes Bridge only for a validated
custody profile. It refuses malformed metadata and noncanonical declarations.
The generated schema-version-2 bytes are published as `registry.json` in
`layerx-core-module-registry`, shared by gateway and receipt authority. The authority init container installs
those same canonical bytes into its private `registry.json` file.

After node readiness, `layerx-module-registry read-node` requests an authenticated
LNI preparation snapshot through the node socket as UID 4021. Bring-up compares
its module ids and ordinals with the published ConfigMap and refuses disagreement.
Assets are not compared: preparation snapshots carry no asset metadata.
The node image must install `/usr/local/bin/layerx-module-registry` for this gate.

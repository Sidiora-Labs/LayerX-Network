# Hosted identity

`layerx-identity` is the hosted principal and session service
(`platform/hosted/identity/Cargo.toml:2`;
`platform/hosted/identity/Cargo.toml:8-10`). The crate is
`layerx-platform-identity`; the binary path is `src/main.rs`. It
stores principals and sessions on disk and answers TLS HTTP from
seven calling services: `gateway`, `webhooks`, `dashboard`, `faucet`,
`testnet`, `ramp`, and `provisioning`
(`platform/hosted/identity/src/main.rs:36-66`). Provisioning creates
principals, mints sessions, and revokes sessions. Introspection is
permitted for the other six service tokens and refused for
provisioning
(`platform/hosted/identity/src/main.rs:862-887`;
`platform/hosted/identity/src/main.rs:683`).

The image is `ghcr.io/sidiora-labs/layerx-identity:0.1.0`, user
`4020:4020`, entrypoint `/usr/local/bin/layerx-identity`
(`platform/hosted/identity/Dockerfile:5-11`;
`platform/hosted/identity/deployment.yaml:16-19`). The Deployment has
one replica, strategy `Recreate`, listens on `0.0.0.0:9443`, and
exposes Service port `9443`
(`platform/hosted/identity/deployment.yaml:10-11`;
`platform/hosted/identity/deployment.yaml:21-23`;
`platform/hosted/identity/deployment.yaml:46-48`). Pod label
`layerx-plane` is `trusted-boundary`
(`platform/hosted/identity/deployment.yaml:14`).

This page covers that binary, its on-disk store, and the tests in
`platform/hosted/identity/`.

---

## Callers

Identity authenticates callers by exact match of a Bearer token to one
file under `LAYERX_IDENTITY_SERVICE_TOKENS_DIR`. Every name in
`Service::ALL` must be present, at least 16 bytes, and distinct
(`platform/hosted/identity/src/main.rs:47-55`;
`platform/hosted/identity/src/main.rs:267-304`;
`platform/hosted/identity/src/main.rs:470-483`). A directory entry
whose name is not one of those seven services refuses boot, except
names that start with `..`, which are skipped
(`platform/hosted/identity/src/main.rs:274-281`).

| Caller | Token file name | Identity route | Response shape |
| --- | --- | --- | --- |
| Gateway | `gateway` | `POST /v1/sessions/introspect` (`platform/hosted/gateway/src/main.rs:966-973`) | `GatewayShape`: `active`, `sub`, `allowed_signer_public_keys` (`platform/hosted/identity/src/main.rs:117-121`; `platform/hosted/identity/src/main.rs:604-620`) |
| Webhooks | `webhooks` | `POST /v1/sessions/introspect` (`platform/hosted/webhooks/src/trusted.rs:393-399`) | `DeveloperShape`: `active`, `sub`, `csrf_token` (`platform/hosted/identity/src/main.rs:124-128`; `platform/hosted/identity/src/main.rs:621-636`) |
| Dashboard | `dashboard` | `POST /v1/sessions/introspect` via the same `DeveloperIdentity::authenticate` (`platform/hosted/webhooks/src/trusted.rs:337-363`; `platform/hosted/webhooks/src/trusted.rs:398`) | same `DeveloperShape` (`platform/hosted/identity/src/main.rs:621`) |
| Faucet | `faucet` | `POST` to the path in `LAYERX_IDENTITY_INTROSPECTION_URL` (`platform/hosted/faucet/src/main.rs:261-266`; `platform/hosted/faucet/src/main.rs:469-476`; `platform/hosted/testnet/deployment.yaml:145`) | `SubjectShape`: `active`, `sub` (`platform/hosted/identity/src/main.rs:131-134`; `platform/hosted/identity/src/main.rs:637-650`) |
| Testnet control | `testnet` | `GET /readyz` only (`platform/hosted/testnet/src/main.rs:823-826`; `platform/hosted/testnet/src/main.rs:1097`; `platform/hosted/testnet/src/main.rs:1032-1034`) | readiness JSON, not an introspection shape |
| Ramp | `ramp` | `POST /v1/introspect` with `audience` (`platform/hosted/identity/src/main.rs:580-582`; `platform/hosted/identity/src/main.rs:651-681`) | `RampShape`: `active`, `principal_id`, `account`, `audience`, `expires_at` (`platform/hosted/identity/src/main.rs:137-143`) |
| Provisioning | `provisioning` | `POST /v1/principals`, `POST /v1/sessions`, `DELETE /v1/sessions/{id}` (`platform/hosted/identity/src/main.rs:869-886`; `platform/hosted/tests/beta-cluster.sh:1000-1026`) | `PrincipalResponse`, `SessionResponse`, `RevocationResponse` (`platform/hosted/identity/src/main.rs:146-167`) |

`human/crates` maps `GET /internal/v1/principal` onto schema path
`/v1/sessions` inside the human HTTP server
(`human/crates/layerx-human-service/src/server/http.rs:141-146`). That
path is not a request to `layerx-identity`. `agent/crates` has no
`/v1/introspect`, `/v1/sessions`, or `/v1/principals` route to this
service.

Bring-up copies `gateway-identity.token` to `identity-tokens/gateway`,
`developer-identity.token` to `identity-tokens/webhooks`, and
`identity-client.token` to `identity-tokens/faucet`, then generates
distinct files for `dashboard`, `testnet`, `ramp`, and `provisioning`
(`platform/hosted/tests/beta-cluster.sh:524-532`). Webhooks and
dashboard both mount `identity-token` from
`layerx-developer-hosted-runtime`, which is `developer-identity.token`
(`platform/hosted/tests/beta-cluster.sh:639`;
`platform/hosted/webhooks/deployment.yaml:31`;
`platform/hosted/webhooks/deployment.yaml:49`;
`platform/hosted/webhooks/deployment.yaml:109`;
`platform/hosted/webhooks/deployment.yaml:164`). That presented token
is the `webhooks` file. The `dashboard` token file is still required
at identity boot (`platform/hosted/identity/src/main.rs:284-286`).
Those two token identities differ.

No `platform/hosted` binary other than `layerx-identity` and its
tests presents the `ramp` token. Bring-up still writes
`identity-tokens/ramp` (`platform/hosted/tests/beta-cluster.sh:530-531`).
Testnet control sets `LAYERX_TESTNET_IDENTITY_URL` and does not set an
identity service-token env (`platform/hosted/testnet/deployment.yaml:92`;
`platform/hosted/testnet/deployment.yaml:83-101`).

---

## Credential and session model

A principal record is `sub`, `allowed_signer_public_keys`, optional
`account`, and `audiences`
(`platform/hosted/identity/src/store.rs:16-21`). `sub` is 1..=128
bytes of lowercase ASCII, digits, `-`, `_`, `.`, `:`
(`platform/hosted/identity/src/main.rs:204-211`). Signer keys are at
most 128 unique 32-byte lowercase hex strings
(`platform/hosted/identity/src/main.rs:26`;
`platform/hosted/identity/src/main.rs:229-236`). `account` is at most
512 identifier bytes; each audience is at most 128 identifier bytes;
audiences are at most 32 and unique
(`platform/hosted/identity/src/main.rs:27`;
`platform/hosted/identity/src/main.rs:214-220`;
`platform/hosted/identity/src/main.rs:701-716`). Empty signer-key
lists are accepted (`platform/hosted/identity/src/main.rs:229-236`;
`platform/hosted/identity/src/main.rs:1084`).

A session token is `ses_{id}.{secret}` with `id` 16 bytes hex and
`secret` 32 bytes hex (`platform/hosted/identity/src/main.rs:29-30`;
`platform/hosted/identity/src/main.rs:485-490`;
`platform/hosted/identity/src/main.rs:791`). The store keeps
`sha256_hex(secret)` as `token_digest`, not the secret
(`platform/hosted/identity/src/main.rs:770`;
`platform/hosted/identity/src/store.rs:28`). CSRF is 32 random bytes
hex (`platform/hosted/identity/src/main.rs:31`;
`platform/hosted/identity/src/main.rs:760`). The store keeps
`sha256_hex(csrf)` and a sealed copy
(`platform/hosted/identity/src/main.rs:764-772`;
`platform/hosted/identity/src/store.rs:29-30`). Lookup constant-time
compares the presented digest, then opens the sealed CSRF and checks
its digest (`platform/hosted/identity/src/main.rs:500-551`).

Default TTL is `LAYERX_IDENTITY_SESSION_TTL_SECONDS` or 86400 seconds.
Per-session `ttl_seconds` may override. Zero and values above
`30 * 86400` are refused
(`platform/hosted/identity/src/main.rs:28`;
`platform/hosted/identity/src/main.rs:353-358`;
`platform/hosted/identity/src/main.rs:748-752`). At most 4096 live
unrevoked sessions exist per principal
(`platform/hosted/identity/src/store.rs:12`;
`platform/hosted/identity/src/store.rs:120-129`). A second revoke
returns the original `revoked_at`
(`platform/hosted/identity/src/store.rs:143-144`).

Unknown, digest-mismatched, revoked, or expired tokens introspect as
inactive for the caller's shape, not as an HTTP error
(`platform/hosted/identity/src/main.rs:526-532`;
`platform/hosted/identity/src/main.rs:596`). Ramp additionally
requires a stored `account` and an audience grant matching the
request; otherwise `active` is false with empty `principal_id` and
`account` (`platform/hosted/identity/src/main.rs:651-680`).

---

## Request and response contracts

Inbound TLS is rustls. `LAYERX_IDENTITY_TLS_CERT_DER` and
`LAYERX_IDENTITY_TLS_KEY_DER` are required. When
`LAYERX_IDENTITY_CLIENT_CA_DER` is set, a presented client certificate
must chain to it; when unset, the server uses no client authentication
(`platform/hosted/identity/src/main.rs:307-337`). The Deployment does
not set `LAYERX_IDENTITY_CLIENT_CA_DER`
(`platform/hosted/identity/deployment.yaml:22-29`). Gateway and
webhooks still present a PKCS#12 client identity on outbound HTTPS
(`platform/hosted/gateway/src/main.rs:492-500`;
`platform/hosted/webhooks/src/trusted.rs:315-326`;
`platform/hosted/webhooks/src/trusted.rs:346-357`). Those two TLS
client-auth settings differ.

HTTP/1.1 only, `Host` required, no query string, no
`Transfer-Encoding`, no duplicate headers, body bound 16 KiB, I/O
timeout 8s, at most 128 live connections
(`platform/hosted/identity/src/main.rs:23-25`;
`platform/hosted/identity/src/main.rs:370-451`;
`platform/hosted/identity/src/main.rs:959-969`). Further accepts are
dropped without a response
(`platform/hosted/identity/src/main.rs:990-993`). Parse failure is
`400 invalid_request` (`platform/hosted/identity/src/main.rs:948-950`).
JSON bodies use `#[serde(deny_unknown_fields)]`
(`platform/hosted/identity/src/main.rs:90-114`). Successful JSON is
HTTP 200 with `Content-Type: application/json`, `Cache-Control:
no-store`, and `Connection: close`
(`platform/hosted/identity/src/main.rs:559-567`;
`platform/hosted/identity/src/main.rs:917-937`).

Headers `forwarded`, `x-forwarded-for`, `x-real-ip`,
`x-layerx-client-ip`, and `x-layerx-principal` are refused as
`400 untrusted_identity_header` before service authentication
(`platform/hosted/identity/src/main.rs:839-846`). Request headers and
bodies are zeroized on drop
(`platform/hosted/identity/src/main.rs:176-183`).

| Method and path | Inputs | Output |
| --- | --- | --- |
| `GET /livez` | none | `{"status":"live","service":"identity"}` (`platform/hosted/identity/src/main.rs:833-834`) |
| `GET /readyz` | none | `{"status":"ready","service":"identity"}` after `probe_writable`, else `503 store_unavailable` (`platform/hosted/identity/src/main.rs:822-829`; `platform/hosted/identity/src/main.rs:836-837`) |
| `POST /v1/principals` | Bearer provisioning, `application/json` `PrincipalRequest` (`platform/hosted/identity/src/main.rs:99-106`; `platform/hosted/identity/src/main.rs:693-737`) | `PrincipalResponse` |
| `POST /v1/sessions` | Bearer provisioning, `application/json` `SessionRequest` (`platform/hosted/identity/src/main.rs:110-114`; `platform/hosted/identity/src/main.rs:740-798`) | `SessionResponse` with `token` and `csrf_token` |
| `DELETE /v1/sessions/{id}` | Bearer provisioning, `id` 16-byte hex (`platform/hosted/identity/src/main.rs:801-819`; `platform/hosted/identity/src/main.rs:881-886`) | `RevocationResponse` |
| `POST /v1/sessions/introspect` | Bearer of an introspecting service, `application/json` `{"token"}` and, for ramp only, `audience` (`platform/hosted/identity/src/main.rs:91-95`; `platform/hosted/identity/src/main.rs:570-596`; `platform/hosted/identity/src/main.rs:862-867`) | caller shape |
| `POST /v1/introspect` | same handler as `/v1/sessions/introspect` (`platform/hosted/identity/src/main.rs:847-852`; `platform/hosted/identity/src/main.rs:863`) | caller shape |

Faucet is configured with
`https://layerx-identity.layerx-testnet.svc.cluster.local:9443/v1/introspect`
(`platform/hosted/testnet/deployment.yaml:145`). Gateway is configured
with origin
`https://layerx-identity.layerx-testnet.svc.cluster.local:9443` and
posts `/v1/sessions/introspect`
(`platform/hosted/gateway/deployment.yaml:82`;
`platform/hosted/gateway/src/main.rs:971`). Webhooks and dashboard use
origin `https://identity.layerx-internal.svc` and post
`/v1/sessions/introspect`
(`platform/hosted/webhooks/deployment.yaml:9`;
`platform/hosted/webhooks/deployment.yaml:48`;
`platform/hosted/webhooks/src/trusted.rs:398`). Those paths are both
accepted (`platform/hosted/identity/src/main.rs:863`).

Bring-up `identity_request` treats HTTP 201 or 200 as success
(`platform/hosted/tests/beta-cluster.sh:1014-1015`;
`platform/hosted/tests/beta-cluster.sh:1018-1019`;
`platform/hosted/tests/beta-cluster.sh:1023-1024`). `serialize` and
`ok` emit 200 (`platform/hosted/identity/src/main.rs:559-567`;
`platform/hosted/identity/src/main.rs:892-897`). Those accepted status
sets differ.

---

## Storage and durability

State lives in `LAYERX_IDENTITY_STATE_DIR`: `snapshot.json`,
`journal.log`, and `ready.marker`
(`platform/hosted/identity/src/store.rs:8-10`;
`platform/hosted/identity/src/main.rs:345-348`). Open creates the
directory, loads the snapshot if present, replays the journal, writes
a new snapshot, and truncates the journal
(`platform/hosted/identity/src/store.rs:61-94`). A torn trailing
journal line without a newline is discarded
(`platform/hosted/identity/src/store.rs:245-247`). Records are JSON
lines under 64 KiB (`platform/hosted/identity/src/store.rs:11`;
`platform/hosted/identity/src/store.rs:191-193`). Append `write_all`
then `sync_all`; failure sets `failed` and later writes and readiness
refuse until process restart
(`platform/hosted/identity/src/store.rs:156-169`;
`platform/hosted/identity/src/store.rs:189-204`). Replacing the
directory or journal inode also refuses
(`platform/hosted/identity/src/store.rs:164-168`).

CSRF plaintext is sealed with `StoreKey` derived from
`LAYERX_IDENTITY_STORE_KEY_FILE`: SHA-256 labelled
`layerx-identity-seal-encryption` and
`layerx-identity-seal-authentication`, SHA-256 keystream, HMAC-SHA256
tag, hex encoding (`platform/hosted/identity/src/seal.rs:10-41`;
`platform/hosted/identity/src/main.rs:359-364`). Token secrets are not
written to snapshot or journal
(`platform/hosted/identity/tests/service.rs:836-844`).

The PVC is `layerx-identity-state`, `ReadWriteOnce`, 5Gi, mounted at
`/var/lib/layerx/identity`
(`platform/hosted/identity/deployment.yaml:1-4`;
`platform/hosted/identity/deployment.yaml:35`;
`platform/hosted/identity/deployment.yaml:40`). Secrets
`layerx-identity-server-tls`, `layerx-identity-service-tokens`, and
`layerx-identity-store-key` are applied by bring-up
(`platform/hosted/tests/beta-cluster.sh:624-626`;
`platform/hosted/identity/deployment.yaml:41-43`).

---

## Typed refusals

HTTP refusals are `{"error":{"code":…,"retry":"never"|"after"}}` with
optional `retry_after_seconds` and header `Retry-After`
(`platform/hosted/identity/src/main.rs:900-914`;
`platform/hosted/identity/src/main.rs:927-929`).

| HTTP | Code | Condition |
| --- | --- | --- |
| 400 | `invalid_request` | Request framing failed (`platform/hosted/identity/src/main.rs:948-950`) |
| 400 | `untrusted_identity_header` | `forwarded`, `x-forwarded-for`, `x-real-ip`, `x-layerx-client-ip`, or `x-layerx-principal` present (`platform/hosted/identity/src/main.rs:839-846`) |
| 400 | `content_type_required` | JSON route without `application/json` (`platform/hosted/identity/src/main.rs:571-572`; `platform/hosted/identity/src/main.rs:694-695`; `platform/hosted/identity/src/main.rs:741-742`) |
| 400 | `invalid_argument` | JSON decode failure, unknown field, invalid `sub`/keys/account/audiences/TTL/token bounds, or `audience` on a non-ramp introspect (`platform/hosted/identity/src/main.rs:576`; `platform/hosted/identity/src/main.rs:584`; `platform/hosted/identity/src/main.rs:587`; `platform/hosted/identity/src/main.rs:697-718`; `platform/hosted/identity/src/main.rs:751-752`) |
| 400 | `audience_required` | Ramp introspect missing a valid `audience` (`platform/hosted/identity/src/main.rs:580-582`) |
| 401 | `service_token_required` | Missing, empty, oversized, or unmatched Bearer (`platform/hosted/identity/src/main.rs:470-482`) |
| 403 | `service_not_permitted` | Provisioning on introspect, or any other service on principals/sessions/revoke (`platform/hosted/identity/src/main.rs:683`; `platform/hosted/identity/src/main.rs:864-883`) |
| 404 | `not_found` | Unknown method/path (`platform/hosted/identity/src/main.rs:855-856`; `platform/hosted/identity/src/main.rs:888`) |
| 404 | `principal_not_found` | Session mint for an unknown `sub` (`platform/hosted/identity/src/main.rs:780-781`) |
| 404 | `session_not_found` | Revoke of a missing or non-hex id (`platform/hosted/identity/src/main.rs:802-803`; `platform/hosted/identity/src/main.rs:817`) |
| 429 | `session_bound_reached` | Live sessions for the principal at 4096; `Retry-After: 60` (`platform/hosted/identity/src/main.rs:784-786`; `platform/hosted/identity/src/store.rs:127-129`) |
| 503 | `store_unavailable` | Mutex poison, journal/directory identity change, journal write failure, CSRF open/digest failure, or readiness probe failure; `Retry-After: 5` (`platform/hosted/identity/src/main.rs:502-507`; `platform/hosted/identity/src/main.rs:539-550`; `platform/hosted/identity/src/main.rs:726-730`; `platform/hosted/identity/src/main.rs:823-828`) |
| 503 | `clock_unavailable` | System clock precedes Unix epoch; `Retry-After: 5` (`platform/hosted/identity/src/main.rs:197-201`; `platform/hosted/identity/src/main.rs:589-590`; `platform/hosted/identity/src/main.rs:754-755`; `platform/hosted/identity/src/main.rs:805-806`) |
| 503 | `encoding_failed` | JSON serialize failure; `Retry-After: 5` (`platform/hosted/identity/src/main.rs:559-561`) |
| 503 | `entropy_unavailable` | `getrandom` or CSRF seal failure; `Retry-After: 5` (`platform/hosted/identity/src/main.rs:687-689`; `platform/hosted/identity/src/main.rs:757-765`) |

Unknown HTTP statuses other than 200/400/401/403/404/429 are written
as `Service Unavailable` (`platform/hosted/identity/src/main.rs:918-926`).

---

## Readiness

`GET /livez` does not touch the store
(`platform/hosted/identity/src/main.rs:833-834`). `GET /readyz` locks
the store and writes `ready.marker` through a temp file, `sync_all`,
rename, and directory sync
(`platform/hosted/identity/src/main.rs:822-829`;
`platform/hosted/identity/src/store.rs:172-187`). Process start also
opens the store and probes writable before listen
(`platform/hosted/identity/src/main.rs:978-987`).

Deployment `readinessProbe` is HTTPS `/readyz` every 5s,
`failureThreshold` 3; `livenessProbe` is HTTPS `/livez` every 15s
(`platform/hosted/identity/deployment.yaml:30-31`). Testnet control
probes identity with unauthenticated `GET /readyz` and requires HTTP
200 (`platform/hosted/testnet/src/main.rs:1032-1034`;
`platform/hosted/testnet/src/main.rs:1097`). Gateway `/readyz` probes
store, agent-boundary, authority, and registry; identity is not among
those components (`platform/hosted/gateway/src/main.rs:2779-2808`).

---

## Config keys

| Key | Role |
| --- | --- |
| `LAYERX_IDENTITY_LISTEN` | Bind address; default `0.0.0.0:9443` (`platform/hosted/identity/src/main.rs:341-344`; `platform/hosted/identity/deployment.yaml:23`) |
| `LAYERX_IDENTITY_TLS_CERT_DER` | Inbound server certificate DER (`platform/hosted/identity/src/main.rs:311-315`; `platform/hosted/identity/deployment.yaml:24`) |
| `LAYERX_IDENTITY_TLS_KEY_DER` | Inbound PKCS#8 key DER (`platform/hosted/identity/src/main.rs:313-318`; `platform/hosted/identity/deployment.yaml:25`) |
| `LAYERX_IDENTITY_CLIENT_CA_DER` | Optional client CA DER; unset means no client authentication (`platform/hosted/identity/src/main.rs:320-333`). The Deployment does not set it (`platform/hosted/identity/deployment.yaml:22-29`) |
| `LAYERX_IDENTITY_STATE_DIR` | Required store directory (`platform/hosted/identity/src/main.rs:345-348`; `platform/hosted/identity/deployment.yaml:26`) |
| `LAYERX_IDENTITY_SERVICE_TOKENS_DIR` | Required directory of seven service token files (`platform/hosted/identity/src/main.rs:349-352`; `platform/hosted/identity/deployment.yaml:27`) |
| `LAYERX_IDENTITY_STORE_KEY_FILE` | Required seal secret file (`platform/hosted/identity/src/main.rs:254-257`; `platform/hosted/identity/src/main.rs:359`; `platform/hosted/identity/deployment.yaml:28`) |
| `LAYERX_IDENTITY_SESSION_TTL_SECONDS` | Default session TTL; default `86400`; must be `1..=2592000` (`platform/hosted/identity/src/main.rs:353-358`; `platform/hosted/identity/deployment.yaml:29`) |

Secret files are refused if empty, longer than 4096 bytes, or
containing ASCII control (`platform/hosted/identity/src/main.rs:244-250`).

---

## Cluster apply and admitted edges

`IMAGE_NAMES` includes `layerx-identity`
(`platform/hosted/tests/beta-cluster.sh:89-90`). Image source is
`ghcr.io/sidiora-labs/layerx-identity:0.1.0` from
`platform/hosted/identity/Dockerfile`
(`platform/hosted/tests/beta-cluster.sh:125`). Render writes
`platform/hosted/identity/deployment.yaml` as `identity.yaml`
(`platform/hosted/tests/beta-cluster.sh:829`).
`trusted_boundary_apply` applies `paxeer.yaml`, then `identity.yaml`,
then `node.yaml`, and requires Service `layerx-identity`
(`platform/hosted/tests/beta-cluster.sh:858-865`;
`platform/hosted/tests/beta-cluster.sh:91`). Bring-up waits for
`app=layerx-identity`, port-forwards `19451:9443`, then
`identity_provision`
(`platform/hosted/tests/beta-cluster.sh:95`;
`platform/hosted/tests/beta-cluster.sh:1266`;
`platform/hosted/tests/beta-cluster.sh:1274-1276`). Provisioning posts
principals for the smoke source and destination DIDs and, when
`LAYERX_BETA_TEST_AUTH_TOKEN_FILE` is unset, mints a source session
whose token must match `^ses_[0-9a-f]{32}\.[0-9a-f]{64}$`
(`platform/hosted/tests/beta-cluster.sh:29-31`;
`platform/hosted/tests/beta-cluster.sh:1008-1026`). Those principal
bodies omit `account` and `audiences`
(`platform/hosted/tests/beta-cluster.sh:1012-1013`).

Server certificate SANs include
`layerx-identity.layerx-testnet.svc.cluster.local`,
`identity.layerx-internal.svc.cluster.local`, `localhost`, and
`127.0.0.1` (`platform/hosted/tests/beta-cluster.sh:397-398`).

NetworkPolicy ingress admits TCP 9443 from pods
`app=layerx-testnet-control`, `app=layerx-faucet`, `app=layerx-gateway`,
and from namespace `kubernetes.io/metadata.name=layerx-developer`
(`platform/hosted/identity/deployment.yaml:51-58`). Egress is DNS 53
only (`platform/hosted/identity/deployment.yaml:60-68`). Default
`topology-check.sh` manifests include
`platform/hosted/identity/deployment.yaml`
(`platform/hosted/tests/topology-check.sh:17-25`;
`platform/hosted/tests/topology-check.sh:83-91`).

---

## Tests

Crate unit tests (no cluster):

| Test | Proves |
| --- | --- |
| `service_resolution_is_exact` | Bearer match is exact; truncated, padded, and empty tokens miss (`platform/hosted/identity/src/main.rs:1029-1049`) |
| `session_token_form_is_strict` | `ses_{32hex}.{64hex}` only; missing dot, `tok_` prefix, uppercase hex, and short id miss (`platform/hosted/identity/src/main.rs:1051-1068`) |
| `subject_and_key_validation_matches_the_gateway_rules` | lowercase `sub`; uppercase hex keys, duplicates, and 129 keys refuse; empty key list is valid (`platform/hosted/identity/src/main.rs:1071-1085`) |
| `introspection_shapes_serialize_exactly` | Gateway, developer, subject, and ramp JSON field order (`platform/hosted/identity/src/main.rs:1087-1126`) |
| `refusal_bodies_follow_the_hosted_contract` | `retry` `never`/`after`, `Retry-After`, `Cache-Control: no-store`, `Connection: close` (`platform/hosted/identity/src/main.rs:1128-1149`) |
| `request_parser_rejects_unbounded_and_ambiguous_messages` | Chunked encoding, duplicate `Host`, missing `Host`, query string, and oversized `Content-Length` refuse (`platform/hosted/identity/src/main.rs:1151-1172`) |
| `state_survives_reopen_and_compaction` | Principal, two sessions, idempotent revoke survive reopen; journal length becomes 0 (`platform/hosted/identity/src/store.rs:318-381`) |
| `torn_trailing_record_is_discarded_and_malformed_records_refuse` | Half-line discarded; unknown revoke and non-JSON refuse open (`platform/hosted/identity/src/store.rs:383-411`) |
| `replaced_journal_refuses_readiness_and_writes` | Unlinked-and-replaced journal fails `probe_writable` and `put_principal` (`platform/hosted/identity/src/store.rs:414-423`) |
| `failed_journal_write_requires_restart` | Read-only journal sets failed; restart accepts writes (`platform/hosted/identity/src/store.rs:425-441`) |
| `session_requires_a_known_principal_and_unique_identifier` | Unknown principal and duplicate session id refuse; `ready.marker` exists after probe (`platform/hosted/identity/src/store.rs:443-461`) |
| `seal_round_trips_and_binds_the_key` | Seal/open round-trip; other derived key fails open (`platform/hosted/identity/src/seal.rs:152-165`) |
| `seal_uses_a_fresh_nonce_and_rejects_tampering` | Distinct ciphertexts; flipped ciphertext byte and short/non-hex refuse (`platform/hosted/identity/src/seal.rs:167-182`) |
| `hmac_matches_rfc_4231_case_two` | HMAC-SHA256 tag `5bdcc146…ec3843` (`platform/hosted/identity/src/seal.rs:184-196`) |
| `hex_round_trips` | Lowercase hex only; odd length and uppercase refuse (`platform/hosted/identity/src/seal.rs:198-205`) |

Integration tests spawn `CARGO_BIN_EXE_layerx-identity` over real TLS
(`platform/hosted/identity/tests/service.rs:203-227`):

| Test | Proves |
| --- | --- |
| `health_routes_answer_without_a_service_token` | `/livez` and `/readyz` 200; unknown path `404 not_found`; `X-Forwarded-For` `400 untrusted_identity_header`; `ready.marker` exists (`platform/hosted/identity/tests/service.rs:469-498`) |
| `readiness_fails_when_the_store_is_not_writable` | Removed or replaced state dir is `503 store_unavailable` with `Retry-After: 5`; restart recovers (`platform/hosted/identity/tests/service.rs:501-539`) |
| `every_introspection_shape_matches_its_consumer` | Active shapes for gateway/webhooks/dashboard/faucet/testnet/ramp; ramp wrong audience is inactive; ramp missing audience is `400 audience_required`; stray audience and unknown field are 400; wrong secret, missing session, and malformed token are inactive (`platform/hosted/identity/tests/service.rs:542-600`) |
| `wrong_service_tokens_are_refused` | Missing/unknown Bearer 401; provisioning introspect 403; non-provisioning principal/session/revoke 403; unknown principal 404; uppercase `sub` and short key 400 (`platform/hosted/identity/tests/service.rs:603-701`) |
| `revoked_sessions_introspect_inactive` | Delete 200 with `revoked_at`; second delete returns the same `revoked_at`; unknown and `../snapshot.json` ids 404 (`platform/hosted/identity/tests/service.rs:704-750`) |
| `expired_sessions_introspect_inactive` | `ttl_seconds: 1` becomes inactive after 2s; `0` and `2592001` are 400 (`platform/hosted/identity/tests/service.rs:753-794`) |
| `state_survives_a_restart` | Active session and CSRF survive restart; revoked session stays inactive; secrets and CSRF plaintext stay out of snapshot/journal; restart truncates the journal (`platform/hosted/identity/tests/service.rs:797-870`) |

Gateway local lifecycle starts the same binary and posts
`/v1/principals` and `/v1/sessions` as 200
(`platform/hosted/gateway/tests/local/lifecycle.rs:111-184`).

---

## Make targets

| Target | Identity use |
| --- | --- |
| `platform-test-identity` | `cargo test --offline --manifest-path platform/Cargo.toml --locked -p layerx-platform-identity` (`platform/Makefile.inc:153-154`) |
| `platform-test-trusted-boundary` | depends on `platform-test-identity` (`platform/Makefile.inc:9`; `platform/Makefile.inc:159`) |
| `platform-test` | workspace `cargo test`, including this crate (`platform/Makefile.inc:112-113`) |
| `platform-hosted-topology-check` | `topology-check.sh`; default manifests include identity (`platform/Makefile.inc:177-178`; `platform/hosted/tests/topology-check.sh:19`; `platform/hosted/tests/topology-check.sh:85`) |

[Home](Home.md)

## Report — spec/layerx-beta task 6.8 do_1: developer-plane network dependency enumeration

Task text: `/root/Layerx-protocol/spec/layerx-beta/tasks.md:214-218`, `/root/Layerx-protocol/spec/layerx-beta/spec.kvx:656-660`. No `observation` for 6.8 exists in `spec/layerx-beta/qualification.kvx` yet (only the 3.7 run at line 618).

---

## 1. Complete dependency inventory

Every URL default lives in one ConfigMap, `layerx-developer-hosted`, at `/root/Layerx-protocol/platform/hosted/webhooks/deployment.yaml:1-54`. It is the **only** place in the repository where `*.layerx-internal.svc` appears (verified by repo-wide grep); there is no `layerx-internal` namespace manifest, no Service, no Redis, no KMS anywhere.

| # | Host | Env var (default) | Consumer | Protocol / port |
|---|---|---|---|---|
| 1 | `redis.layerx-internal.svc` | `LAYERX_WEBHOOKS_REDIS_URL=rediss://redis.layerx-internal.svc:6379` (`deployment.yaml:7`) | webhooks `HostedService` + `HostedReader` | RESP over TLS 1.2+, 6379 |
| 2 | `redis.layerx-internal.svc` | `LAYERX_DASHBOARD_GATEWAY_REDIS_URL=rediss://redis.layerx-internal.svc:6379` (`deployment.yaml:44`) | dashboard `gateway::Store` | RESP over TLS, 6379 — **same host, different key space, different ACL user** |
| 3 | `kms.layerx-internal.svc` | `LAYERX_WEBHOOKS_KMS_URL` (`:8`) | webhooks `KmsClient` | HTTPS/1.1 + mTLS, **443** |
| 4 | `identity.layerx-internal.svc` | `LAYERX_WEBHOOKS_IDENTITY_URL` (`:9`) | webhooks `DeveloperIdentity` | HTTPS + mTLS, 443 |
| 5 | `identity.layerx-internal.svc` | `LAYERX_DASHBOARD_IDENTITY_URL` (`:49`) | dashboard `DeveloperIdentity` | HTTPS + mTLS, 443 |
| 6 | `component.layerx-internal.svc` | `LAYERX_WEBHOOKS_COMPONENT_URL` (`:10`) | webhooks `ReceiptVerifier` | HTTPS + mTLS, 443 |
| 7 | `authority.layerx-internal.svc` | `LAYERX_WEBHOOKS_AUTHORITY_URL` (`:11`) | webhooks `ReceiptVerifier` | HTTPS + mTLS, 443 |
| 8 | `journeys.layerx-internal.svc` | `LAYERX_WEBHOOKS_JOURNEY_SOURCE_URL` (`:12`) | webhooks `TrustedSources` | HTTPS + mTLS, 443 |
| 9 | `payments.layerx-internal.svc` | `LAYERX_WEBHOOKS_PAYMENT_SOURCE_URL` (`:13`) | webhooks `TrustedSources` | HTTPS + mTLS, 443 |
| 10 | `approvals.layerx-internal.svc` | `LAYERX_WEBHOOKS_APPROVAL_SOURCE_URL` (`:14`) | webhooks `TrustedSources` | HTTPS + mTLS, 443 |
| 11 | `programs.layerx-internal.svc` | `LAYERX_WEBHOOKS_PROGRAM_SOURCE_URL` (`:15`) | webhooks `TrustedSources` | HTTPS + mTLS, 443 |
| 12 | **arbitrary developer-registered receiver URLs** | endpoint `url` supplied at registration | webhooks delivery worker | HTTPS to public internet, `Client::public` — public-IP-only guard |

The four source URLs are built by string interpolation over `JOURNEY|PAYMENT|APPROVAL|PROGRAM` at `/root/Layerx-protocol/platform/hosted/webhooks/src/trusted.rs:137-153`, so the env-var names are `LAYERX_WEBHOOKS_{STEM}_SOURCE_URL` / `_TOKEN_FILE`.

**Endpoint parsing constraints** (`/root/Layerx-protocol/platform/hosted/webhooks/src/boundary.rs:20-61`): scheme must be `https://` (plain HTTP rejected), host must be a canonical DNS name (**IP literals rejected**, `boundary.rs:49`), port defaults to 443. Redis URL must be `rediss://` (`hosted.rs:301-324`, `dashboard/src/gateway.rs:315-337`); a trailing `/0` database suffix is rejected.

**Load-bearing TLS detail:** the SNI/verification name is always the literal env-var host — `connector.connect(&endpoint.host, tcp)` at `boundary.rs:243`, `hosted.rs:445`, `dashboard/src/gateway.rs:250`. If identity/component/authority are provided as `ExternalName` aliases to `layerx-testnet`, the *target's* server certificate must still carry SANs for `identity.layerx-internal.svc`, `component.layerx-internal.svc`, `authority.layerx-internal.svc`, `redis.layerx-internal.svc`, `kms.layerx-internal.svc` etc. A selector-based Service in `layerx-internal` has the same requirement. `beta-cluster.sh` issues no such certificate today (`beta-cluster.sh:324-338`).

---

## 2. Per-dependency contract detail

### 2.1 Redis (webhooks) — `redis.layerx-internal.svc:6379`

Client: `RedisRepository`, `/root/Layerx-protocol/platform/hosted/webhooks/src/hosted.rs:333-466`.

- Auth: **`AUTH <username> <password>`** (two-arg ACL form) sent on **every** command, `hosted.rs:447-454`; must reply `+OK`. Credentials read from `LAYERX_WEBHOOKS_REDIS_USERNAME_FILE` / `_PASSWORD_FILE` (`deployment.yaml:29-30`, `hosted.rs:660-661`, `733-734`).
- TLS: root CA only, from `LAYERX_WEBHOOKS_INTERNAL_CA_DER` (`hosted.rs:635`, `684`). **No client certificate is presented** (`hosted.rs:439-443`) → the Redis server must run `tls-auth-clients no`.
- Connection model: a fresh TCP+TLS connection + `AUTH` per command, `Connection: close` semantics; 3 s connect / 8 s IO timeouts (`hosted.rs:30-31`); up to 8 resolved addresses tried.
- RESP: hand-rolled writer/reader, `hosted.rs:1827-1925`. Refuses inline/`-ERR` (returns Err), nesting > 8, bulk > 20 MiB (`MAX_REDIS_RESPONSE`), array > 200 000, line > 8192.
- Commands and keys:
  - `PING` → must be `+PONG` (readiness, `hosted.rs:341-343`)
  - `HMGET webhooks:principal:<sha256(principal)> revision json` (`hosted.rs:346-348`, key format at `hosted.rs:1747-1749`)
  - `EVAL <CAS_SCRIPT> 2 <shard key> webhooks:principals <expected-rev> <json> <principal-digest>` → `Integer(1)` on success (`hosted.rs:384-396`); script at `hosted.rs:1935-1943` uses `HGET/HSET/HINCRBY/SADD`. **Requires `+@scripting`** in the ACL.
  - `SMEMBERS webhooks:principals` (cap 100 000) and `HGET webhooks:principal:<digest> json` (`hosted.rs:400-414`)
- Shard JSON cap 16 MiB (`MAX_SHARD_BYTES`, `hosted.rs:28`), 16 CAS retries (`CAS_ATTEMPTS`, `hosted.rs:29`) then `WebhookError::Unavailable`.
- ACL needed: at minimum `~webhooks:*` `+@read +@write +@scripting +ping`. `beta-cluster.sh` generates `webhook-redis.username = "layerx-webhooks"` and a password (`beta-cluster.sh:426-427`) but **writes no ACL file for it** (contrast `write_redis_acl` used only for faucet/gateway at `beta-cluster.sh:423-425`).

### 2.2 Redis (dashboard) — same host, gateway key space

Client: `Store`, `/root/Layerx-protocol/platform/hosted/dashboard/src/gateway.rs:44-270`. Same AUTH/TLS shape; CA from `LAYERX_DASHBOARD_REDIS_CA_DER` (`gateway.rs:57-64`); credentials `LAYERX_DASHBOARD_GATEWAY_REDIS_USERNAME_FILE` / `_PASSWORD_FILE` (`gateway.rs:68-69`). Max response 2 MiB, array cap 10 000 (`gateway.rs:20`, `442`).

Commands, all read-only:
- `PING` → `+PONG` (`gateway.rs:74`)
- `SMEMBERS gateway:principal:<sha256(principal)>:keys`, ≤128 members (`gateway.rs:211-229`)
- `HMGET gateway:key:<id> principal quota_requests quota_window_seconds disabled` — requires exactly 4 fields and `principal` to equal the digest (`gateway.rs:82-98`)
- `GET gateway:quota:<id>:<now/window>` (`gateway.rs:107-114`)
- `XREVRANGE gateway:audit + - COUNT <limit*8, ≤1600>` — entries parsed as `[id, [field, value, ...]]`; requires field `event` prefixed `"<principal-digest>:"` and reads optional field `outcome` mapped `rate_limited | receipt_verified|completed | pending | *→Refused` (`gateway.rs:143-192`).

**These keys are already produced in-repo by the hosted gateway**: `gateway:key:*`, `gateway:principal:*:keys`, `gateway:quota:*`, `gateway:audit`, `gateway:audit:head` at `/root/Layerx-protocol/platform/hosted/gateway/src/store.rs:154-165, 217, 327, 425-438`; the audit `XADD ... 'event', ..., 'outcome', ...` fields at `store.rs:727, 767, 777, 788, 803, 816, 827`; the `"<principal_digest>:<action>:<subject>:<outcome>"` event string at `/root/Layerx-protocol/platform/hosted/gateway/src/main.rs:1147-1155`. So `redis.layerx-internal.svc` for the dashboard is functionally the gateway's Redis (`layerx-gateway-redis.layerx-testnet.svc.cluster.local:6379`, `gateway/deployment.yaml:86`) — one Redis serving both key spaces, or an alias.

### 2.3 KMS — `kms.layerx-internal.svc` (HTTPS 443)

Client: `KmsClient`, `/root/Layerx-protocol/platform/hosted/webhooks/src/hosted.rs:468-591`. Auth: `Authorization: Bearer <LAYERX_WEBHOOKS_KMS_TOKEN_FILE>` **plus** the mTLS client identity from `LAYERX_WEBHOOKS_CLIENT_IDENTITY_PKCS12` + `..._PASSWORD_FILE` (`hosted.rs:686-692`, `713-720`).

Three routes, all `Content-Type: application/json`:

1. `POST /v1/signing-keys` with `Idempotency-Key: register:<principal-digest>:<idempotency>` (or `rotate:` scope), body `{"algorithm":"ed25519","purpose":"layerx-webhook-v1"}` (`hosted.rs:497-513`). Accepts 200 or 201, response `KmsKeyResponse { key_id, handle, public_key }` **`#[serde(deny_unknown_fields)]`** (`hosted.rs:474-480`). `public_key` is standard-padded base64 of exactly 32 bytes; `key_id` must satisfy `scheme::valid_key_id` and start with `scheme::KEY_PREFIX`; `handle` 1..=512 bytes, no `\0\r\n` (`hosted.rs:521-530`).
2. `POST /v1/signatures`, body `{"key_handle":<handle>,"algorithm":"ed25519","message":<base64(message)>}` (`hosted.rs:539-543`). Response `KmsSignatureResponse { signature }` **`deny_unknown_fields`** (`hosted.rs:482-486`); must be 64 bytes base64 and **is verified locally against the recorded public key with ed25519-dalek** before use (`hosted.rs:562-570`).
3. `GET /readyz` → response `KmsReadiness { ready, ed25519_non_exportable }` **`deny_unknown_fields`** (`hosted.rs:488-493`); ready only when **both** booleans are true (`hosted.rs:573-590`).

### 2.4 identity — `identity.layerx-internal.svc`

Client: `DeveloperIdentity`, `/root/Layerx-protocol/platform/hosted/webhooks/src/trusted.rs:294-413` (webhooks ctor `:295-322`, dashboard ctor `from_dashboard_environment` `:324-351`).

- Auth: bearer service token (`LAYERX_WEBHOOKS_IDENTITY_TOKEN_FILE` / `LAYERX_DASHBOARD_IDENTITY_TOKEN_FILE`) + mTLS client identity.
- One route: `POST /v1/sessions/introspect`, request body `{"token": "<session token>"}` (`trusted.rs:373-388`).
- Response `SessionResponse { active: bool, sub: String, csrf_token: String (serde default) }`, **`deny_unknown_fields`** (`trusted.rs:62-69`). `sub` must pass `PrincipalId::new` (the gateway's own rule, `events.rs:151-160`). `csrf_token` is compared constant-time against `X-LayerX-CSRF` for cookie-authenticated mutations, and must be 1..=256 bytes (`trusted.rs:397-409`).
- Session token is taken either from `Authorization: Bearer` (≤4096) or the cookie `__Host-layerx-session` (`trusted.rs:360-372`, `546-561`).
- **Not part of `/healthz`** on either service — identity failure yields 401 `session_required` per request (`webhooks/src/main.rs:387-390`, `dashboard/src/main.rs:173-176`).
- The same route name is dialed by the gateway (`platform/hosted/gateway/src/main.rs:1051`), which the identity spec 6.5 says returns a *different* shape per service token.

### 2.5 component — `component.layerx-internal.svc`

`ReceiptVerifier`, `/root/Layerx-protocol/platform/hosted/webhooks/src/trusted.rs:35-44, 435-515`. Bearer `LAYERX_WEBHOOKS_COMPONENT_TOKEN_FILE` + mTLS.

- `GET /readyz` → 200 required for readiness (`trusted.rs:436-447`).
- `GET /internal/v1/receipts/{activity_id}` → `ComponentReceipt { activity_id, receipt }`, **`deny_unknown_fields`** (`trusted.rs:95-100`); `receipt` is hex-encoded canonical receipt bytes; `activity_id` must equal the requested value (`trusted.rs:490`).

This is exactly the route spec 6.4 assigns to `layerx-agent-boundary` (`tasks.md` 6.4: "`GET /internal/v1/receipts/{id}` for the developer plane").

### 2.6 authority — `authority.layerx-internal.svc`

Same verifier. Bearer `LAYERX_WEBHOOKS_AUTHORITY_TOKEN_FILE` + mTLS.

- `GET /readyz` → 200 required for readiness.
- `GET /internal/v1/activities/{activity_id}/authority` → `AuthorityResponse` with **exactly eight fields**, `deny_unknown_fields` (`trusted.rs:102-113`): `activity_id, batch_id, asset, previous_state_root, resulting_state_root, sequencer_public_key, network_id, wire_version`.
- Cross-checks (`trusted.rs:490-500`): `activity_id` matches; `network_id == LAYERX_WEBHOOKS_NETWORK_ID` ("testnet", `deployment.yaml:16`); `wire_version == LAYERX_WEBHOOKS_LXP_WIRE_VERSION` ("2", `deployment.yaml:17`, also asserted equal to `layerx_wire::limits::PROTOCOL_VERSION` at startup, `trusted.rs:155-158`); `sequencer_public_key` (hex, 32 bytes) must equal `LAYERX_WEBHOOKS_SEQUENCER_PUBLIC_KEY_FILE`.
- Then `verify_activity_operation(receipt_bytes, AuthorityFacts::new(batch_id, asset, previous_state_root, resulting_state_root, sequencer), trusted_key, Some(expected_activity_id))` (`trusted.rs:501-513`) — the real `layerx_platform_gateway` verifier, no double.

This matches task 6.3's eight-field `AuthorityBody` for `layerx-receipt-authority`.

### 2.7 journeys / payments / approvals / programs

`TrustedSources`, `/root/Layerx-protocol/platform/hosted/webhooks/src/trusted.rs:46-50, 115-292`. Each source: bearer `LAYERX_WEBHOOKS_{STEM}_SOURCE_TOKEN_FILE` + mTLS (shared `developer-client` identity).

- `GET /readyz` → 200 required for readiness (`trusted.rs:183-198`).
- `GET /internal/v1/events/{source_event_id}` (`trusted.rs:212-227`). `source_event_id` is bounded to ≤128 chars of `[A-Za-z0-9._-]` (`trusted.rs:205-207`, `538-544`). Non-200 or non-`application/json` → `WebhookError::Unavailable` → 503 `dependency_unavailable`.
- Response `SourceRecord`, **`deny_unknown_fields`** (`trusted.rs:71-93`):

```
{ id, principal, subject, subject_sequence: u64, occurred_at: u64,
  facts: [ { name, value } ],            // SourceFact, deny_unknown_fields, ≤32 entries
  activity_id?: String,                  // hex 32 bytes; triggers receipt verification
  amount?: String, asset?: String }      // required for kind=payment
```

- `record.id` must equal the requested id (`trusted.rs:230`); `facts.len() > 32` → `InvalidRequest` (`MAX_SOURCE_FACTS`, `trusted.rs:20`). Fact `name` ≤128, `value` ≤512 (`events.rs:12-14`).
- `id`/`subject` must satisfy `valid_token`; `principal` must satisfy the gateway `PrincipalId` rule.
- **payment kind is strictly stronger**: `activity_id` is mandatory (else `VerificationRequired`), `amount` must be all-ASCII digits and non-empty, `asset` required, the receipt must have `result_code == 0` and verification ≥ `receipt-verified` (`trusted.rs:245-260`, `events.rs:629-670`). journey/approval/program may omit `activity_id`.
- Ordering: `subject_sequence` must strictly exceed the shard high-water per subject or publish returns `OrderViolation` → **409** (`hosted.rs:1002-1009`); re-fetch of an identical event returns `duplicate: true` (`hosted.rs:979-998`). This is what `tests/fault-injection.sh:38, 44` asserts.
- The webhooks route that drives this is `POST /internal/v1/events/{kind}/{source_event}` guarded by `LAYERX_WEBHOOKS_SOURCE_TRIGGER_TOKEN_FILE` (`webhooks/src/main.rs:329-340`); kinds are exactly `journey|payment|approval|program` (`events.rs:114-120`).

### 2.8 Outbound webhook receivers

`HostedService::send`, `/root/Layerx-protocol/platform/hosted/webhooks/src/hosted.rs:1148-1211`. `Client::public(LAYERX_WEBHOOKS_PUBLIC_CA_DER)` — refuses any destination resolving to a private/loopback/link-local/CGNAT/6to4/Teredo address (`boundary.rs:99-162`). POSTs the signed envelope with 10 `X-LayerX-*` headers; signature comes from the KMS.

---

## 3. `/healthz` readiness gating

### webhooks — `webhooks/src/main.rs:359-372`

```rust
let delivery = config.service.ready();
let sources  = config.sources.ready();
// 200 iff delivery && sources, body {ready, components:{delivery_state_and_signer, canonical_sources_and_receipt_authority}}
```

- `delivery` = `HostedService::ready()` = `repository.ready() && kms.ready()` (`hosted.rs:746-748`) → **Redis PING + KMS `GET /readyz` with both booleans true**.
- `sources` = `TrustedSources::ready()` = `verifier.ready() && all 4 sources 200 on /readyz` (`trusted.rs:183-198`) → **component `/readyz` + authority `/readyz` + journeys + payments + approvals + programs `/readyz`**.
- **Six HTTPS `/readyz` calls + one KMS `/readyz` + one Redis PING per probe**, every 10 s (`deployment.yaml:75`), across 3 replicas.
- Identity is **not** gated. The `readinessProbe` uses `scheme: HTTPS`, so it is unauthenticated TLS against the pod cert.

### dashboard — `dashboard/src/main.rs:166-171`, `dashboard/src/service.rs:28-30`

```rust
Dashboard::ready() = self.gateway.ready() && self.webhooks.ready()
```
= gateway-Redis `PING` (`gateway.rs:73-75`) **and** webhooks-Redis `PING` via `HostedReader::ready` (`hosted.rs:667-669`). No KMS, no identity, no sources.

Both `/healthz` handlers are reachable without a session because they are checked before `principal(...)`.

---

## 4. `beta-cluster.sh wait_ready` and the `layerx-developer` manifests

### 4.1 `wait_ready` — `/root/Layerx-protocol/platform/hosted/tests/beta-cluster.sh:662-695`

Two conditions must hold simultaneously, polled every 5 s until `LAYERX_BETA_READY_TIMEOUT` (default 900 s, `:26`, `:70`):

1. `GET $TESTNET_URL/readyz` returns a body satisfying `.state == "ready" and all(.journeys[]; .ready == true)` **and** `all(.dependencies[]; .ready == true) and (.journeys | length) == 4`.
2. `developer_ready` is the empty string, where

```bash
developer_ready=$(kube -n layerx-developer get deployments -o json \
  | jq -r '[.items[] | select((.status.readyReplicas // 0) < .spec.replicas) | .metadata.name] | join(",")')
```

i.e. **every Deployment in `layerx-developer` must have `readyReplicas == spec.replicas`**: `layerx-webhooks` 3/3, `layerx-dashboard-api` 2/2, `layerx-dashboard-web` 2/2 (`webhooks/deployment.yaml:61, 122, 168`). Because readiness is the HTTPS `/healthz` probe, this transitively requires all nine `layerx-internal` dependencies to be live and answering. A StatefulSet for Redis would *not* be covered by this jq (Deployments only) — a Redis StatefulSet in `layerx-internal` is gated only indirectly, via the webhooks/dashboard probes.

Also relevant: `manifests_apply` (`:610-627`) applies `developer.yaml` with `kube -n layerx-developer`; `port_forward developer layerx-developer layerx-webhooks 19450 443` at `:826`; `wait_ready || fail ...` at `:829`.

### 4.2 Secrets/ConfigMaps the developer manifests mount

**ConfigMap** `layerx-developer-hosted` — embedded in `webhooks/deployment.yaml:1-54`, consumed by both Rust containers via `envFrom` (`:70`, `:131`). Not created by the script; it ships in the manifest.

**Secret** `layerx-developer-hosted-runtime` — created at `beta-cluster.sh:505-516`, projected read-only at `/run/layerx` (`deployment.yaml:87-115` webhooks, `:145-161` dashboard-api). Keys → paths:

| Secret key | Mount path | Env var pointing at it | Generated at |
|---|---|---|---|
| `tls-cert.der`, `tls-key.der` | `tls/server.der`, `tls/server-key.der` | `LAYERX_{WEBHOOKS,DASHBOARD}_TLS_{CERT,KEY}_DER` | `issue_cert developer` `:335-336` |
| `internal-ca.der` | `ca/internal.der` | `LAYERX_WEBHOOKS_INTERNAL_CA_DER`, `LAYERX_DASHBOARD_INTERNAL_CA_DER`, `LAYERX_DASHBOARD_REDIS_CA_DER` | `ca_generate` `:317-323` |
| `public-ca.der` | `ca/public.der` | `LAYERX_WEBHOOKS_PUBLIC_CA_DER` | same CA reused (beta) `:508` |
| `client-identity.p12`, `client-password` | `client/identity.p12`, `client/password` | `LAYERX_{WEBHOOKS,DASHBOARD}_CLIENT_IDENTITY_PKCS12` / `_PASSWORD_FILE` | `issue_client_identity developer-client layerx-developer` `:338` |
| `webhook-redis-username/password` | `redis/username`, `redis/password` | `LAYERX_WEBHOOKS_REDIS_USERNAME_FILE` / `_PASSWORD_FILE` | `:426-427` (`layerx-webhooks` + token) |
| `dashboard-redis-username/password` | `dashboard-redis/username`, `.../password` | `LAYERX_DASHBOARD_GATEWAY_REDIS_USERNAME_FILE` / `_PASSWORD_FILE` | `:428-429` (`layerx-dashboard` + token) |
| `kms-token` | `tokens/kms` | `LAYERX_WEBHOOKS_KMS_TOKEN_FILE` | loop `:430-433` |
| `identity-token` | `tokens/identity` | `LAYERX_WEBHOOKS_IDENTITY_TOKEN_FILE`, `LAYERX_DASHBOARD_IDENTITY_TOKEN_FILE` | same loop |
| `component-token`, `authority-token` | `tokens/component`, `tokens/authority` | `LAYERX_WEBHOOKS_{COMPONENT,AUTHORITY}_TOKEN_FILE` | same loop |
| `journey/payment/approval/program-source-token` | `tokens/{journey,payment,approval,program}` | `LAYERX_WEBHOOKS_{JOURNEY,PAYMENT,APPROVAL,PROGRAM}_SOURCE_TOKEN_FILE` | same loop |
| `source-trigger-token`, `webhook-operator-token` | `tokens/source-trigger`, `tokens/operator` | `LAYERX_WEBHOOKS_SOURCE_TRIGGER_TOKEN_FILE`, `_OPERATOR_TOKEN_FILE` | same loop |
| `cursor-key` | `keys/cursor` | `LAYERX_WEBHOOKS_CURSOR_KEY_FILE` (must be 64 hex chars, `hosted.rs:1806-1817`) | `:419` |
| `sequencer-public-key` | `keys/sequencer` | `LAYERX_WEBHOOKS_SEQUENCER_PUBLIC_KEY_FILE` (64 hex) | `:435` from `ca_generate` |

Also in the namespace: `layerx-internal-ca` (`:504`, not referenced by these pods) and `layerx-developer-ingress-tls` (`:517`, used by both Ingresses `deployment.yaml:222, 239`).

One env var has **no secret and no ConfigMap entry**: `LAYERX_WEBHOOKS_INSTANCE_ID` — supplied by downward API `metadata.uid` (`deployment.yaml:72-73`), required at `hosted.rs:721-725` and must match `[A-Za-z0-9._-]{1,128}` (a pod UID does).

`layerx-dashboard-web` mounts only an `emptyDir` at `/tmp` (`deployment.yaml:187-191`) and has no network dependency of its own (`egress: []`, `:295`) — but it is still counted by `wait_ready`.

**NetworkPolicy egress** for both Rust pods is port-only, `UDP 53 / TCP 443 / TCP 6379` with no destination selector (`deployment.yaml:265-269`, `280-284`) — permissive enough for `layerx-internal`, but there is no reciprocal ingress policy on the `layerx-internal` side because that namespace does not exist.

---

## 5. Do in-repo implementations exist to back these?

### 5.1 KMS — **no HTTP KMS service exists; the one production KMS crate speaks a different protocol**

- Repo-wide grep for `/v1/signing-keys` and `/v1/signatures` finds only **clients**, never a server: `platform/hosted/webhooks/src/hosted.rs:507, 550` and `platform/ramps/toolkit/src/clients.rs:1052`.
- `/root/Layerx-protocol/platform/ramps/toolkit/src/clients.rs:1028-1070` is the closest contract match: `POST /v1/signatures` with `{key_handle, algorithm:"ed25519", message: base64}` → `Response { signature }` with `deny_unknown_fields`, locally verified via `ed25519::verify_digest`. It is a **client**; the ramp docs name the KMS as an owner-supplied external dependency (`platform/ramps/OPERATIONS.md:5, 9`, `platform/ramps/README.md:23-27`, `platform/docs/content/beta.md:260`).
- `human/crates/layerx-human-service/src/custody/provider.rs:490-670` — `RemoteKmsProvider` is the production provider, but it speaks a **bounded binary framed protocol over mutual TLS** (`OP_PROBE/OP_CREATE/OP_DESCRIBE/OP_ROTATE/OP_DESTROY/OP_SIGN`, `encode_key_request`, `decode_description`), not HTTP/JSON. Its counterparty is also external.
- `human/crates/layerx-human-service/src/custody/mod.rs:205-256` — `EnvelopeKms` is a file-envelope provider explicitly `ProviderDeployment::DevelopmentOnly` (`provider.rs:681-685`) and is **refused in production** (`CustodyError::DevelopmentProviderInProduction`, `custody/mod.rs:309, 355`).
- No crate or binary named `*kms*` exists (`find`/`grep -l` across `Cargo.toml` returns nothing).

→ There is a reusable **contract shape** (ramps client + human-service provider trait) but **no server**. Under do_2's wording this is the "otherwise recorded blocked with the input named" branch unless a new `POST /v1/signing-keys` + `POST /v1/signatures` + `GET /readyz {ready, ed25519_non_exportable}` boundary is written. Note the webhooks client additionally needs **key creation**, which the ramps contract does not cover.

### 5.2 Redis with TLS + ACLs — **two working in-repo templates**

- `platform/hosted/gateway/deployment.yaml:1-54`: ConfigMap `layerx-gateway-redis-config` with `redis.conf` (`port 0`, `tls-port 6379`, `tls-cert-file/tls-key-file/tls-ca-cert-file`, **`tls-auth-clients no`**, `aclfile /run/layerx/auth/users.acl`, `appendonly yes`, `appendfsync always`, `maxmemory-policy noeviction`, `protected-mode yes`), a `redis:8.2.1-alpine` StatefulSet with a 50 Gi `volumeClaimTemplate`, a headless Service on 6379, and a NetworkPolicy pair (`:147-166`).
- `platform/hosted/testnet/deployment.yaml:96, 149-151` uses the same pattern for `layerx-faucet-redis`.
- ACL generation helper: `beta-cluster.sh:410-413` — `write_redis_acl` emits `user default off\nuser <name> on ><password> ~* &* +@all`; invoked for faucet and gateway at `:423-425`. `tls-auth-clients no` is required by the webhooks/dashboard clients, which present no client cert.
- Cert issuance template for a Redis server cert: `beta-cluster.sh:333-334`.

→ A `layerx-internal` Redis StatefulSet can be derived verbatim from the gateway one; the only new work is the SAN `redis.layerx-internal.svc`, two ACL users (`layerx-webhooks` scoped to `webhooks:*` incl. `+@scripting`, `layerx-dashboard` read-only over `gateway:*`), and deciding whether the dashboard reads the *same* instance the gateway writes (it must, per §2.2).

### 5.3 identity / component / authority — **all three are placeholder stubs today**

`platform/hosted/identity/src/main.rs`, `platform/hosted/authority/src/main.rs`, `platform/hosted/agent-boundary/src/main.rs` (and `core/`, `paxeer/`) each contain exactly:

```rust
fn main() -> std::process::ExitCode { eprintln!("not yet implemented"); std::process::ExitCode::FAILURE }
```

Cargo manifests exist (`layerx-platform-identity`/`layerx-identity`, `layerx-platform-authority`/`layerx-receipt-authority`, `layerx-platform-agent-boundary`/`layerx-agent-boundary`). Task 6.8 therefore hard-depends on 6.3, 6.4 and 6.5 landing first — the aliasing "where the contract matches" is exactly the eight-field authority body (§2.6), `/internal/v1/receipts/{id}` (§2.5) and the `{active, sub, csrf_token}` introspection shape (§2.4).

### 5.4 Event-source producers

- **journeys / approvals** — the human service has a resumable event stream whose kinds are `journey-progress, approval-created, approval-approved, approval-rejected, approval-expired, notification` with `POST /v1/stream` + `GET /v1/stream/{cursor}` returning `{events:[{cursor, kind, observed_at, journey?, approval?, notification?}], next_cursor}` (`/root/Layerx-protocol/human/schema/human-api/stream.kvx:9-38`), plus `GET /v1/journeys` and `GET /v1/journeys/{journey_id}` (`human/schema/human-api/journeys.kvx:57-67`). Journal implementation at `human/crates/layerx-human-service/src/server/stream_journal.rs`, approvals at `human/crates/layerx-human-service/src/approvals/{mod,decide,render}.rs`. **None of these is `GET /internal/v1/events/{id}` and none emits the `SourceRecord` shape** — an adapter is required.
- **payments** — the gateway is the receipt-bearing surface: `/v1/activities`, `/v1/programs/call`, `/v1/receipts/{activity_id}`, `/v1/authorized-batches/by-activity/{activity_id}` (`platform/hosted/gateway/src/main.rs:1393, 1869-1871, 2110, 2276`), and it already writes the per-principal audit stream the dashboard reads (`gateway/src/store.rs:460-500`). No `/internal/v1/events/{id}` route exists.
- **programs** — the registry route table is `GET /healthz`, `POST /__registry/deployments`, `POST /__registry/head`, `POST /__registry/sources`, then `program_route` (`platform/hosted/registry/src/routes.rs:222-248`); it maintains a node program-state change feed with a cursor and per-program notices (`routes.rs:255-300`, `node_state.rs`, `program_state.rs`). That feed is the natural `program` source, again behind an adapter.
- **No producer anywhere serves `GET /internal/v1/events/{id}` or `GET /readyz` in the source shape** — grep for `internal/v1/events` returns only the webhooks consumer (`trusted.rs:212`) and the fault-injection script (`webhooks/tests/fault-injection.sh:27, 33, 37, 43`).

---

## 6. Gaps the implementation phase (do_2/do_3) must close

1. **No `layerx-internal` namespace exists.** `beta-cluster.sh:468-469` creates only `layerx-testnet` and `layerx-developer`. Nothing in the repo declares it, and the `LAYERX_BETA_EXTERNAL_MANIFESTS` comment (`beta-cluster.sh:14-19`) explicitly names these nine as owner-supplied.
2. **No certificates with `*.layerx-internal.svc` SANs.** `ca_generate` (`beta-cluster.sh:317-350`) issues certs only for testnet-control, gateway, faucet, registry, the two Redises and `developer`. Because the client uses the env-var host as the TLS verification name (§1), aliasing without new SANs will fail the handshake.
3. **Port mismatch on the aliases.** `identity/component/authority.layerx-internal.svc` default to **443**, while the testnet Services of tasks 6.3–6.5 listen on **9443** — the aliasing Services must remap 443 → 9443 (an `ExternalName` cannot remap ports; a selector Service or a `Service` + `EndpointSlice` is needed).
4. **No ACL entries for the two developer Redis users.** Usernames/passwords are generated (`beta-cluster.sh:426-429`) but no `users.acl` is written or mounted for them.
5. **`topology-check.sh` does not see the developer plane.** Default manifests are testnet + gateway + registry only (`platform/hosted/tests/topology-check.sh:75-79`); `SEPARATELY_OPERATED` (`:100-108`) has no `layerx-internal` entries and `webhooks/deployment.yaml` is not in the default set.
6. **No `platform-test-tooling` coverage for these crates.** `platform/Makefile.inc:113-126` runs cli/emulator/faucet/testnet tests and syntax checks; `layerx-platform-webhooks` and `layerx-platform-dashboard` have no cargo-test line, and `platform/hosted/dashboard` has no `deployment.yaml` of its own (both API and web live inside the webhooks manifest).
7. **`beta.md` does not list any of the nine.** The endpoints table (`platform/docs/content/beta.md:118-129`) and the external-dependency table (`:258-260`) name the testnet boundaries and the ramp KMS but no `layerx-internal` service — `tools/ci/beta-contract-check.sh` will need the new rows.
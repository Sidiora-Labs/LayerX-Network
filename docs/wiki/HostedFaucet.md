# Hosted faucet

An exact successful claim and the resulting JSON-RPC payment flow are in
[Public payment API](PublicAPI.md).

`layerx-faucet` is the public claim surface for hosted testnet funds
(`platform/hosted/faucet/Cargo.toml:8-10`;
`platform/hosted/faucet/src/main.rs:1025`). The crate is
`layerx-platform-faucet`; the binary path is `src/main.rs`. A
developer or agent with a hosted session Bearer posts
`POST /v1/faucet/claims` and receives either a funded body or a typed
refusal.

The same grant is also reachable without curl. `POST
/v1/faucet/service-claims` is a service route the public gateway proxies
as the JSON-RPC method `lx_requestFunds`
(`platform/hosted/gateway/src/rpc_faucet.rs`), and that method backs the
`layerx faucet` command (`platform/cli/src/main.rs:66`;
`platform/cli/src/faucet.rs`), the SDK call `RpcClient::request_funds`
(`agent/crates/layerx-sdk/src/rpc.rs`), and the served MCP tool
`faucet.request` (`platform/cli/src/toolset.rs:38-44`). See
[Public JSON-RPC](PublicRpc.md) and [CLI](Cli.md).

The image is `ghcr.io/sidiora-labs/layerx-faucet:0.1.0`, user
`4020:4020`, entrypoint `/usr/local/bin/layerx-faucet`
(`platform/hosted/faucet/Dockerfile:5-11`;
`platform/hosted/testnet/deployment.yaml:138`;
`platform/hosted/tests/beta-cluster.sh:116`). There is no
`platform/hosted/faucet/deployment.yaml`. The Deployment, Redis,
Service, and NetworkPolicy live in
`platform/hosted/testnet/deployment.yaml`.

The Deployment has three replicas, listens on `0.0.0.0:9443`, and
exposes Service `layerx-faucet-public` port `443` to container `9443`
(`platform/hosted/testnet/deployment.yaml:128-141`;
`platform/hosted/testnet/deployment.yaml:158`;
`platform/hosted/testnet/deployment.yaml:174-181`). That Service is
`type: LoadBalancer` with `externalTrafficPolicy: Local`. Beta-cluster
render appends Ingress `layerx-faucet-public` host
`faucet.testnet.layerx.network` (override `LAYERX_BETA_FAUCET_HOST`)
path `/` backend that Service
(`platform/hosted/tests/beta-cluster.sh:74`;
`platform/hosted/tests/beta-cluster.sh:837-854`). Bring-up
port-forwards `19445:443` and exports `LAYERX_FAUCET_URL`
(`platform/hosted/tests/beta-cluster.sh:83`;
`platform/hosted/tests/beta-cluster.sh:1116`;
`platform/hosted/tests/beta-cluster.sh:1260`;
`platform/hosted/tests/beta-cluster.sh:1280`). Library
`platform_testnet` names the same faucet origin
`https://faucet.testnet.layerx.network`
(`platform/hosted/testnet/src/lib.rs:77`). The source Deployment has
no Ingress object; the cluster apply path adds one. Those two
manifests differ.

The developer payments path that starts with this claim is
[Payments developer path](PaymentsQuickstart.md).

This page covers that binary, its Redis, and the claim path through
testnet-control. It does not document treasury SEND construction.
That path is on [Hosted core](HostedCore.md). Journey admission is on
[Hosted testnet control](HostedTestnetControl.md). Session minting is
on [Hosted identity](HostedIdentity.md).

---

## TLS

Inbound TLS is rustls with no client authentication. The certificate
is `LAYERX_FAUCET_TLS_CERT_DER`; the PKCS#8 key is
`LAYERX_FAUCET_TLS_KEY_DER`
(`platform/hosted/faucet/src/main.rs:233-249`). Each accepted TCP
connection becomes a rustls `ServerConnection`
(`platform/hosted/faucet/src/main.rs:1164-1165`). At most 128
connections are live; further accepts are dropped without a response
(`platform/hosted/faucet/src/main.rs:23`;
`platform/hosted/faucet/src/main.rs:1199-1201`). Request bodies are
bounded at 16 KiB (`platform/hosted/faucet/src/main.rs:18`;
`platform/hosted/faucet/src/main.rs:446-447`). Query strings,
`Transfer-Encoding`, duplicate headers, and a missing `Host` are
refused (`platform/hosted/faucet/src/main.rs:417-420`;
`platform/hosted/faucet/src/main.rs:461-466`).

Outbound HTTPS uses `native_tls` `TlsConnector` with
`LAYERX_OUTBOUND_CA_DER` and a minimum protocol of TLS 1.2
(`platform/hosted/faucet/src/main.rs:344-347`). Endpoints must be
`https://` or `rediss://` with a DNS host, not a literal IP
(`platform/hosted/faucet/src/main.rs:34-65`). The client writes
`Authorization: Bearer`, `Content-Type: application/json`, and
optional `Idempotency-Key`
(`platform/hosted/faucet/src/main.rs:353-361`). Connect timeout is 3s;
I/O timeout is 8s (`platform/hosted/faucet/src/main.rs:20-21`).

Redis is `rediss://` only, TLS 1.2, the same outbound CA, then `AUTH`
username and password (`platform/hosted/faucet/src/main.rs:290-296`;
`platform/hosted/faucet/src/main.rs:514-534`). The Redis server
manifest sets `tls-auth-clients no`
(`platform/hosted/testnet/deployment.yaml:24`). HTTPS upstreams do not
present a client certificate.

---

## Tokens

The faucet accepts one inbound scheme and three outbound credentials.
It never forwards a caller session Bearer as the upstream
`Authorization` value.

| Credential | Accepted from | Used as | Never |
| --- | --- | --- | --- |
| `Bearer` session | `POST /v1/faucet/claims` (`platform/hosted/faucet/src/main.rs:470-504`) | JSON body `{"token": …}` to identity `POST` at `LAYERX_IDENTITY_INTROSPECTION_URL` (`platform/hosted/faucet/src/main.rs:481-493`) | Upstream `Authorization`. That header carries `LAYERX_IDENTITY_SERVICE_TOKEN_FILE` |
| Identity service token | File `LAYERX_IDENTITY_SERVICE_TOKEN_FILE` (`platform/hosted/faucet/src/main.rs:282`; `platform/hosted/testnet/deployment.yaml:146`) | `Authorization: Bearer` to identity introspect | Presented by humans |
| Testnet-control admin token | File `LAYERX_TESTNET_ADMIN_TOKEN_FILE` (`platform/hosted/faucet/src/main.rs:289`; `platform/hosted/testnet/deployment.yaml:148`) | `Authorization: Bearer` to `LAYERX_TESTNET_FUNDING_URL` | Presented by humans |
| Redis username and password | `LAYERX_FAUCET_REDIS_USERNAME_FILE`, `LAYERX_FAUCET_REDIS_PASSWORD_FILE` (`platform/hosted/faucet/src/main.rs:295-296`) | Redis `AUTH` (`platform/hosted/faucet/src/main.rs:524-530`) | HTTP |

`authenticate` requires prefix `Bearer `, a non-empty token of at most
4096 bytes, identity HTTP 200, JSON `active: true`, and `sub` a
1..=512 identifier of alnum/`-`/`_`/`.`/`:`
(`platform/hosted/faucet/src/main.rs:189-195`;
`platform/hosted/faucet/src/main.rs:470-504`). Identity non-200 or an
inactive/`sub` miss is `401 identity_required`. Transport or JSON
failure is `503 identity_unavailable`
(`platform/hosted/faucet/src/main.rs:483-500`). Incoming headers and
bodies are zeroized on drop
(`platform/hosted/faucet/src/main.rs:144-150`).

The hosted identity path for this caller is `/v1/introspect`, not
`/v1/sessions/introspect`
(`platform/hosted/testnet/deployment.yaml:145`;
`platform/hosted/identity/src/main.rs:863`;
`docs/wiki/HostedIdentity.md`). The faucet response shape is
`SubjectShape` `active` plus `sub`
(`platform/hosted/identity/src/main.rs:131-134`;
`platform/hosted/identity/src/main.rs:637-650`;
`platform/hosted/faucet/src/main.rs:115-119`). Bring-up copies
`identity-client.token` to `identity-tokens/faucet`
(`platform/hosted/tests/beta-cluster.sh:528`).

---

## Public routes

Unauthenticated probes are dispatched before the claim route
(`platform/hosted/faucet/src/main.rs:1010-1026`). HTTP/1.1 only.

| Method and path | Inputs | Result |
| --- | --- | --- |
| `GET /livez` | none | `200` `{"status":"live","service":"faucet"}` (`platform/hosted/faucet/src/main.rs:1011-1012`) |
| `GET /readyz` | none | Redis `PING` `PONG` → `200` `{"status":"ready","service":"faucet"}`; else `503 dependency_unavailable` (`platform/hosted/faucet/src/main.rs:1014-1020`) |
| `POST /v1/faucet/claims` | `Authorization: Bearer`, `Idempotency-Key`, `Content-Type: application/json`, JSON `did` and `public_key` only (`platform/hosted/faucet/src/main.rs:100-105`; `platform/hosted/faucet/src/main.rs:1025-1041`) | `200` funded body, `202` `still_checking`, or a typed refusal |
| `POST /v1/faucet/service-claims` | `Authorization: Bearer <service token>`, `Idempotency-Key`, `Content-Type: application/json`, JSON `principal`, `did` and `public_key` only (`platform/hosted/faucet/src/main.rs:107-113`; `platform/hosted/faucet/src/main.rs:979-1008`) | `200` funded body, `202` `still_checking`, or a typed refusal |

Unknown method or path is `404 not_found`
(`platform/hosted/faucet/src/main.rs:1025-1026`). Probe JSON does not
wrap `ok`. Claim success is not an `{"ok": true, …}` envelope.

`/v1/faucet/service-claims` exists only when
`LAYERX_FAUCET_SERVICE_CLAIM_TOKEN_FILE` is configured; without it the
route answers `404 not_found` and no service claim is possible
(`platform/hosted/faucet/src/main.rs:980-982`). The presented Bearer is
compared against that secret in constant time and only at equal length
(`platform/hosted/faucet/src/main.rs:959-965`). The preflight the public
claim route uses runs first, so content type, forwarded-identity headers,
network admission, and the idempotency key are enforced identically
(`platform/hosted/faucet/src/main.rs:919-953`). The body must parse as a JSON
object before it is deserialised, so a positional array is refused;
`ServiceClaimRequest` then denies unknown fields and requires `principal`
to be an identifier at max 512, `did` to be a DID, and `public_key` to be
64 hex digits (`platform/hosted/faucet/src/main.rs:107-113`;
`platform/hosted/faucet/src/main.rs:967-977`). The caller-supplied
`principal` becomes the identity quota key, so a principal shares one
identity window with its own direct `POST /v1/faucet/claims`; only the
network bucket is scoped `service:<principal>` instead of the calling
service's peer address (`platform/hosted/faucet/src/main.rs:955-957`;
`platform/hosted/faucet/src/main.rs:997-1007`).

`ClaimRequest` denies unknown fields
(`platform/hosted/faucet/src/main.rs:100-105`). `did` must start with
`did:` and pass `valid_identifier` at max 512
(`platform/hosted/faucet/src/main.rs:197-199`). `public_key` is 64
ASCII hex digits (`platform/hosted/faucet/src/main.rs:201-203`;
`platform/hosted/faucet/src/main.rs:1040-1041`). `Idempotency-Key` is
1–128 alnum/`-`/`_`/`.`/`:` (`platform/hosted/faucet/src/main.rs:189-195`;
`platform/hosted/faucet/src/main.rs:946-950`). Those claim fields
match [Testnet quickstart](Quickstart.md).

Headers `forwarded`, `x-forwarded-for`, `x-real-ip`,
`x-layerx-client-ip`, and `x-layerx-principal` are refused as
`400 untrusted_identity_header` before admission
(`platform/hosted/faucet/src/main.rs:927-933`). Network admission
uses the TCP peer address, not a forwarded header
(`platform/hosted/faucet/src/main.rs:642-663`;
`platform/hosted/faucet/src/main.rs:1168`).

Hosted smoke:

```sh
jq -n --arg did "$LAYERX_TEST_SOURCE_DID" --arg public_key "$LAYERX_TEST_SOURCE_PUBLIC_KEY" \
  '{did:$did, public_key:$public_key}' > faucet-request.json
curl --fail --silent --show-error --max-time 30 --cacert "$LAYERX_TEST_CA_FILE" \
  --header "Authorization: Bearer $(tr -d '\r\n' < "$LAYERX_TEST_AUTH_TOKEN_FILE")" \
  --request POST "$LAYERX_FAUCET_URL/v1/faucet/claims" \
  --header "Idempotency-Key: faucet-quickstart-01" \
  --header 'Content-Type: application/json' --data-binary @faucet-request.json
```

(`platform/hosted/testnet/tests/hosted-smoke.sh:101-109`;
`docs/wiki/Quickstart.md`). Smoke asserts
`.funded == true and .funding_id != null`
(`platform/hosted/testnet/tests/hosted-smoke.sh:110`).

---

## Claim reservation and funding

Quota and idempotency are one Redis `EVAL` (`RESERVE_SCRIPT`)
(`platform/hosted/faucet/src/main.rs:666-693`;
`platform/hosted/faucet/src/main.rs:695-785`). The request digest is
SHA-256 over `identity`, `did`, `public_key`, and the configured
amount, each part followed by a 0 byte
(`platform/hosted/faucet/src/main.rs:180-187`;
`platform/hosted/faucet/src/main.rs:1053-1058`). `funding_id` is
SHA-256 over the idempotency key and that digest
(`platform/hosted/faucet/src/main.rs:710`).

| Reservation | HTTP |
| --- | --- |
| `Reserved` (fresh) or `Pending` (same digest, not yet `funded`) | `fund` then complete or pending (`platform/hosted/faucet/src/main.rs:1071-1088`) |
| `Funded` same digest | `200` with the stored response body (`platform/hosted/faucet/src/main.rs:773-775`; `platform/hosted/faucet/src/main.rs:1070`) |
| Different digest under the same idempotency key | `409 idempotency_conflict` (`platform/hosted/faucet/src/main.rs:764-770`; `platform/hosted/faucet/src/main.rs:1090`) |
| Identity, address, or network window exceeded | `429` with code `identity_quota`, `address_quota`, or `network_quota` (`platform/hosted/faucet/src/main.rs:753-760`; `platform/hosted/faucet/src/main.rs:1091-1097`) |

`fund` POSTs JSON `funding_id`, `did`, `public_key`, `amount` to
`LAYERX_TESTNET_FUNDING_URL` with the control-admin token and
`Idempotency-Key` equal to `funding_id`
(`platform/hosted/faucet/src/main.rs:121-127`;
`platform/hosted/faucet/src/main.rs:878-893`;
`platform/hosted/testnet/deployment.yaml:147`). Upstream HTTP 400–499
is `FundingResult::Rejected`: Redis rollback, then
`503 funding_rejected` (`platform/hosted/faucet/src/main.rs:897-898`;
`platform/hosted/faucet/src/main.rs:1080-1085`). HTTP 200 with matching
`funding_id` and `state == "funded"` becomes the public body
(`platform/hosted/faucet/src/main.rs:906-915`). Any other status,
transport failure, or JSON miss is `FundingResult::Unknown` → `202`
(`platform/hosted/faucet/src/main.rs:894-901`;
`platform/hosted/faucet/src/main.rs:1087`). Complete-script failure
after a funded upstream is also `202`
(`platform/hosted/faucet/src/main.rs:1073-1077`).

A 200 claim body is `funded` `true`, `funding_id`, optional
`transaction_id`, `amount` as a decimal string of
`LAYERX_FAUCET_CLAIM_AMOUNT` (default `1000000`), and `network`
`layerx-testnet` (`platform/hosted/faucet/src/main.rs:255`;
`platform/hosted/faucet/src/main.rs:909-915`). A 202 body is `state`
`still_checking`, `retry` `after`, `retry_after_seconds` `10`
(`platform/hosted/faucet/src/main.rs:1109-1115`). Retrying an
indeterminate result repeats the same `funding_id`. A definite
upstream 4xx releases the quota reservation
(`platform/hosted/faucet/src/main.rs:795-807`;
`platform/hosted/faucet/src/main.rs:1080-1082`).

Faucet `did` is any `did:` identifier. Testnet-control admin requires
the same prefix and a 64-hex `public_key`
(`platform/hosted/testnet/src/main.rs:1235-1248`). Core fund
additionally requires `did == did:layerx:` plus the lowercase public
key, and refuses the treasury DID
(`platform/hosted/core/src/main.rs:1475-1484`;
`docs/wiki/HostedCore.md`). Those three DID checks differ. A core
`400`/`422` returns to the faucet as rejected and is published as
`503 funding_rejected`.

The in-cluster funding URL is

`https://layerx-testnet-admin.layerx-testnet.svc.cluster.local/admin/v1/testnet/fund`

(`platform/hosted/testnet/deployment.yaml:147`). Identity is

`https://layerx-identity.layerx-testnet.svc.cluster.local:9443/v1/introspect`

(`platform/hosted/testnet/deployment.yaml:145`). Redis is

`rediss://layerx-faucet-redis.layerx-testnet.svc.cluster.local:6379`

(`platform/hosted/testnet/deployment.yaml:149`).

---

## Config keys

| Key | Role |
| --- | --- |
| `LAYERX_FAUCET_LISTEN` | Bind address; default `0.0.0.0:9443` (`platform/hosted/faucet/src/main.rs:262-264`; `platform/hosted/testnet/deployment.yaml:141`) |
| `LAYERX_FAUCET_TLS_CERT_DER` | Inbound server certificate DER; mounted `/run/layerx/tls/server.crt.der` |
| `LAYERX_FAUCET_TLS_KEY_DER` | Inbound PKCS#8 key DER; mounted `/run/layerx/tls/server.key.der` |
| `LAYERX_OUTBOUND_CA_DER` | Trust bundle for HTTPS and Redis; mounted `/run/layerx/tls/ca.crt.der` |
| `LAYERX_IDENTITY_INTROSPECTION_URL` | Identity HTTPS origin including introspect path |
| `LAYERX_IDENTITY_SERVICE_TOKEN_FILE` | Bearer to identity; mounted `/run/layerx/identity/token` |
| `LAYERX_FAUCET_SERVICE_CLAIM_TOKEN_FILE` | Optional shared secret admitting `POST /v1/faucet/service-claims`; absent means that route is `404 not_found` (`platform/hosted/faucet/src/main.rs:218-223`; `platform/hosted/faucet/src/main.rs:285`). The Deployment does not set this key |
| `LAYERX_TESTNET_FUNDING_URL` | Testnet-control admin fund origin including path |
| `LAYERX_TESTNET_ADMIN_TOKEN_FILE` | Bearer to testnet-control; mounted `/run/layerx/control-admin/token` |
| `LAYERX_FAUCET_REDIS_URL` | `rediss://` origin |
| `LAYERX_FAUCET_REDIS_USERNAME_FILE` | Redis ACL user; mounted `/run/layerx/redis-auth/username` |
| `LAYERX_FAUCET_REDIS_PASSWORD_FILE` | Redis ACL password; mounted `/run/layerx/redis-auth/password` |
| `LAYERX_FAUCET_IDENTITY_LIMIT` | Identity window cap; default `10000000` (`platform/hosted/faucet/src/main.rs:297`; `platform/hosted/testnet/deployment.yaml:152`) |
| `LAYERX_FAUCET_ADDRESS_LIMIT` | Address window cap; default `10000000` |
| `LAYERX_FAUCET_NETWORK_LIMIT` | Peer window cap; default `50000000` |
| `LAYERX_FAUCET_NETWORK_REQUEST_LIMIT` | Admission count; default `60` |
| `LAYERX_FAUCET_NETWORK_REQUEST_WINDOW_SECONDS` | Admission window; default `60` |
| `LAYERX_FAUCET_WINDOW_SECONDS` | Quota window; default `86400` (`platform/hosted/faucet/src/main.rs:253`). The Deployment does not set this key |
| `LAYERX_FAUCET_IDEMPOTENCY_SECONDS` | Idempotency TTL; default `604800`; must be ≥ the quota window (`platform/hosted/faucet/src/main.rs:254-260`; `platform/hosted/testnet/deployment.yaml:157`) |
| `LAYERX_FAUCET_CLAIM_AMOUNT` | Funded amount; default `1000000`; must be positive (`platform/hosted/faucet/src/main.rs:255-260`). The Deployment does not set this key |

Secret files are read, trailing CR/LF stripped, and refused when empty
or longer than 4096 bytes (`platform/hosted/faucet/src/main.rs:205-215`).
Volume mounts are TLS Secret `layerx-faucet-tls` at `/run/layerx/tls`,
`layerx-testnet-identity-client` at `/run/layerx/identity`,
`layerx-testnet-control-admin` at `/run/layerx/control-admin`, and
`layerx-faucet-redis-client` at `/run/layerx/redis-auth`
(`platform/hosted/testnet/deployment.yaml:163-172`).

---

## Redis state and ACL

The faucet owns StatefulSet `layerx-faucet-redis`: Redis 8.2.1, TLS
port 6379, `appendonly yes`, `appendfsync always`,
`maxmemory-policy noeviction`, `protected-mode yes`, ACL file
`/run/layerx/auth/users.acl`
(`platform/hosted/testnet/deployment.yaml:17-29`;
`platform/hosted/testnet/deployment.yaml:31-61`). Service
`layerx-faucet-redis` is headless port 6379
(`platform/hosted/testnet/deployment.yaml:63-66`). NetworkPolicy
admits TCP 6379 only from pods `app=layerx-faucet` and
`app=layerx-testnet-control`
(`platform/hosted/testnet/deployment.yaml:286-294`).

Cluster bring-up writes ACL `user default off` and one enabled user
`layerx-faucet` with `~* &* +@all`
(`platform/hosted/tests/beta-cluster.sh:481-483`;
`platform/hosted/tests/beta-cluster.sh:501-503`).

| Key | Contents |
| --- | --- |
| `faucet:admission:{window}:{digest}` | INCR count with EXPIRE window seconds; digest is SHA-256 of the peer (`platform/hosted/faucet/src/main.rs:635-658`) |
| `faucet:quota:{window}:identity:{digest}` | INCRBY claim amount, EXPIRE remaining window (`platform/hosted/faucet/src/main.rs:706`; `platform/hosted/faucet/src/main.rs:685`) |
| `faucet:quota:{window}:address:{digest}` | Same for destination public key (`platform/hosted/faucet/src/main.rs:707`) |
| `faucet:quota:{window}:network:{digest}` | Same for TCP peer (`platform/hosted/faucet/src/main.rs:708`) |
| `faucet:idem:{digest}` | Hash: `digest`, `state`, `funding_id`, `identity_key`, `address_key`, `network_key`, `amount`, later `response` (`platform/hosted/faucet/src/main.rs:688`; `platform/hosted/faucet/src/main.rs:790`) |
| `faucet:audit` / `faucet:audit:head` | Hash-chained `XADD` stream (`platform/hosted/faucet/src/main.rs:681-691`; `platform/hosted/faucet/src/main.rs:713-720`) |

Audit fields are `event`, `result`, optional `funding_id`, and
`chain`. Authentication values do not enter the stream
(`platform/hosted/faucet/src/main.rs:681-690`).

---

## Callers the NetworkPolicy admits

`topology-check.sh` default manifests include
`platform/hosted/testnet/deployment.yaml`
(`platform/hosted/tests/topology-check.sh:21`;
`platform/hosted/tests/topology-check.sh:87`). Faucet ingress admits
TCP 9443 from `0.0.0.0/0` and `::/0`, and from pods
`app=layerx-testnet-control`
(`platform/hosted/testnet/deployment.yaml:296-306`). Egress is
`layerx-plane: trusted-boundary` on 9443, `layerx-testnet-control` on
9444, Redis 6379, and DNS 53
(`platform/hosted/testnet/deployment.yaml:308-321`). Node ingress
admits `app=layerx-faucet` on TCP 9443
(`platform/hosted/node/deployment.yaml:279-280`). The faucet binary
does not call the core URL. Those two edges differ.

Identity ingress admits `app=layerx-faucet`
(`platform/hosted/identity/deployment.yaml:57`). Testnet-control
admin ingress admits `app=layerx-faucet` on TCP 9444
(`platform/hosted/testnet/deployment.yaml:254-262`).

Probes: Deployment `readinessProbe` HTTPS `/readyz` every 5s,
`livenessProbe` HTTPS `/livez` every 15s
(`platform/hosted/testnet/deployment.yaml:159-160`). Readiness is
Redis `PING` only. Identity and testnet-control are not `/readyz`
components (`platform/hosted/faucet/src/main.rs:1014-1020`).

---

## Typed refusals

HTTP refusals use `{"error":{"code":…,"retry":…}}` and optional
`retry_after_seconds`. `Retry-After` is set when a retry delay is
present (`platform/hosted/faucet/src/main.rs:1118-1132`;
`platform/hosted/faucet/src/main.rs:1146-1147`). Success and `202`
bodies are not that envelope.

| HTTP | Code | Condition |
| --- | --- | --- |
| 400 | `invalid_request` | Request framing failed (`platform/hosted/faucet/src/main.rs:1166-1167`) |
| 400 | `content_type_required` | Claim `Content-Type` is not `application/json` (`platform/hosted/faucet/src/main.rs:924-925`) |
| 400 | `untrusted_identity_header` | Forwarded IP or `x-layerx-principal` present (`platform/hosted/faucet/src/main.rs:927-933`) |
| 400 | `idempotency_key_required` | Missing `Idempotency-Key` (`platform/hosted/faucet/src/main.rs:946-947`) |
| 400 | `invalid_idempotency_key` | Key fails `valid_identifier` max 128 (`platform/hosted/faucet/src/main.rs:949-950`) |
| 400 | `invalid_argument` | JSON/`did`/`public_key` rejected (`platform/hosted/faucet/src/main.rs:1036-1041`) |
| 401 | `service_token_required` | Service claim Bearer missing, malformed, or not the configured secret (`platform/hosted/faucet/src/main.rs:987-993`) |
| 401 | `identity_required` | Missing/empty Bearer, identity non-200, inactive session, or invalid `sub` (`platform/hosted/faucet/src/main.rs:475-502`) |
| 404 | `not_found` | Method/path is not a listed route (`platform/hosted/faucet/src/main.rs:1025-1026`) |
| 409 | `idempotency_conflict` | Same key, different request digest (`platform/hosted/faucet/src/main.rs:1090`) |
| 429 | `network_request_rate` | Admission INCR exceeded (`platform/hosted/faucet/src/main.rs:937-942`) |
| 429 | `identity_quota` | Identity window cap (`platform/hosted/faucet/src/main.rs:753-756`) |
| 429 | `address_quota` | Address window cap |
| 429 | `network_quota` | Peer window cap |
| 503 | `dependency_unavailable` | `/readyz` Redis miss (`platform/hosted/faucet/src/main.rs:1019`) |
| 503 | `persistence_unavailable` | Admission or reserve Redis error (`platform/hosted/faucet/src/main.rs:944`; `platform/hosted/faucet/src/main.rs:1066-1067`) |
| 503 | `identity_unavailable` | Introspect encode/transport/JSON failed (`platform/hosted/faucet/src/main.rs:483-500`) |
| 503 | `funding_rejected` | Upstream 4xx and rollback succeeded (`platform/hosted/faucet/src/main.rs:1080-1082`) |

`202` is not a refusal. It is `still_checking` with `Retry-After: 10`
(`platform/hosted/faucet/src/main.rs:1109-1115`). Reason phrase for
any unlisted status is `Service Unavailable`
(`platform/hosted/faucet/src/main.rs:1136-1144`).

---

## Tests

The faucet crate carries a `#[cfg(test)]` module over service-claim
admission and refusal shape: only the exact service token is admitted, a
service claim must name a principal, a DID, and a signer key, the network
bucket is scoped to the principal and is identical across replicas, and
refusals carry a machine-readable code with honest retry advice
(`platform/hosted/faucet/src/main.rs:1230-1346`). `platform-test-tooling`
runs `cargo test -p layerx-platform-faucet`
(`platform/Makefile.inc:118`).

Hosted smoke, against a real faucet URL
(`platform/hosted/testnet/tests/hosted-smoke.sh`):

- `GET /readyz` is 200 with `status=ready` and `service=faucet`
  (`platform/hosted/testnet/tests/hosted-smoke.sh:80-86`)
- Funding journey is admitted, then `POST /v1/faucet/claims` with the
  session Bearer returns `funded` true and a `funding_id`
  (`platform/hosted/testnet/tests/hosted-smoke.sh:101-111`)

`make platform-hosted-smoke` requires `LAYERX_FAUCET_URL` among its
inputs (`platform/Makefile.inc:161-173`). `platform-test` is workspace
`cargo test`, including `layerx-platform-faucet`
(`platform/Makefile.inc:112-113`; `platform/Cargo.toml:7`).

[Home](Home.md)

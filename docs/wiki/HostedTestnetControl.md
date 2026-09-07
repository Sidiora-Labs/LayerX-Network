# Hosted testnet control

`layerx-testnet-control` is the public status, parameter, and journey
admission surface for hosted LayerX Network, and the private funding/reset
proxy (`platform/hosted/testnet/README.md:3`;
`platform/hosted/testnet/Cargo.toml:11-13`;
`platform/hosted/testnet/src/main.rs:1141-1282`). The crate is
`layerx-platform-testnet`; the library path is `src/lib.rs`; the
binary path is `src/main.rs`. It is not a protocol node. A developer
or agent reads `GET /readyz`, `GET /v1/status`, `GET /v1/parameters`,
and `GET /v1/journeys/{journey}` with no Bearer. Faucet replicas and
the reset operator call the admin listener.

The image is `ghcr.io/sidiora-labs/layerx-testnet-control:0.1.0`, user
`4020:4020`, entrypoint `/usr/local/bin/layerx-testnet-control`
(`platform/hosted/testnet/Dockerfile:6-13`;
`platform/hosted/testnet/deployment.yaml:80-82`;
`platform/hosted/tests/beta-cluster.sh:114`). The image also copies
`run-testnet.sh`, `reset-testnet.sh`, and `render-status.sh` to
`/opt/layerx/` (`platform/hosted/testnet/Dockerfile:11`). The
Deployment command is the binary, not `run-testnet.sh`
(`platform/hosted/testnet/deployment.yaml:82`).

The Deployment has two replicas. Public listen is `0.0.0.0:9443`;
admin listen is `0.0.0.0:9444`
(`platform/hosted/testnet/deployment.yaml:72`;
`platform/hosted/testnet/deployment.yaml:86-87`;
`platform/hosted/testnet/deployment.yaml:102`). Service
`layerx-testnet-public` port `443` targets `public-tls` `9443`.
Service `layerx-testnet-admin` is ClusterIP port `443` targeting
`admin-tls` `9444`
(`platform/hosted/testnet/deployment.yaml:116-124`). Ingress
`layerx-testnet-public` host `testnet.layerx.network` path `/`
uses backend protocol HTTPS
(`platform/hosted/testnet/deployment.yaml:183-198`). Bring-up
port-forwards `19443:443` and exports `LAYERX_TESTNET_URL`
(`platform/hosted/tests/beta-cluster.sh:81`;
`platform/hosted/tests/beta-cluster.sh:1114`;
`platform/hosted/tests/beta-cluster.sh:1258`;
`platform/hosted/tests/beta-cluster.sh:1278`). Library
`platform_testnet` names `https://testnet.layerx.network`
(`platform/hosted/testnet/src/lib.rs:75`).

This page covers that binary, both listeners, journey probes, and the
funding/reset proxy. Faucet claim HTTP is on
[Hosted faucet](HostedFaucet.md). Treasury SEND construction is on
[Hosted core](HostedCore.md). Gateway `/v1` is on
[Hosted gateway](HostedGateway.md).

---

## TLS

Inbound TLS is rustls with no client authentication on both listeners.
The certificate is `LAYERX_TESTNET_TLS_CERT_DER`; the PKCS#8 key is
`LAYERX_TESTNET_TLS_KEY_DER`
(`platform/hosted/testnet/src/main.rs:762-783`). Each accepted TCP
connection becomes a rustls `ServerConnection`
(`platform/hosted/testnet/src/main.rs:1331-1333`). At most 128
connections are live across both listeners; further accepts are
dropped (`platform/hosted/testnet/src/main.rs:23`;
`platform/hosted/testnet/src/main.rs:1320-1322`). Messages are bounded
at 64 KiB (`platform/hosted/testnet/src/main.rs:20`;
`platform/hosted/testnet/src/main.rs:951`). Query strings,
`Transfer-Encoding`, duplicate headers, and a missing `Host` are
refused (`platform/hosted/testnet/src/main.rs:974-975`;
`platform/hosted/testnet/src/main.rs:1018-1028`). Incoming headers and
bodies are zeroized on drop
(`platform/hosted/testnet/src/main.rs:666-672`).

Outbound HTTPS uses `native_tls` `TlsConnector` with
`LAYERX_OUTBOUND_CA_DER` and TLS 1.2
(`platform/hosted/testnet/src/main.rs:890-894`). Component URLs must
be `https://`. Redis must be `rediss://` with no path
(`platform/hosted/testnet/src/main.rs:34-43`;
`platform/hosted/testnet/src/main.rs:1658-1667`). The client writes
optional `Authorization: Bearer` and optional `Idempotency-Key`
(`platform/hosted/testnet/src/main.rs:911-918`). Connect timeout is
3s; I/O timeout is 8s (`platform/hosted/testnet/src/main.rs:21-22`).
HTTPS upstreams do not present a client certificate.

---

## Tokens

Public routes accept no credential
(`platform/hosted/testnet/src/main.rs:1141-1175`). Admin routes
compare `Authorization: Bearer` to the inbound admin token in
constant time (`platform/hosted/testnet/src/main.rs:1178-1191`).

| Credential | Accepted from | Used as | Never |
| --- | --- | --- | --- |
| Control admin token | File `LAYERX_TESTNET_CONTROL_ADMIN_TOKEN_FILE`; mounted `/run/layerx/control-admin/token` (`platform/hosted/testnet/src/main.rs:860`; `platform/hosted/testnet/deployment.yaml:101`; `platform/hosted/testnet/deployment.yaml:110`) | Inbound admin `Authorization: Bearer` | Public listener |
| Backend admin token | File `LAYERX_TESTNET_BACKEND_ADMIN_TOKEN_FILE`; mounted `/run/layerx/backend-admin/token` (`platform/hosted/testnet/src/main.rs:859`; `platform/hosted/testnet/deployment.yaml:100`; `platform/hosted/testnet/deployment.yaml:109`) | `Authorization: Bearer` to core-admin fund and reset | Presented by humans |
| Reset token | File `LAYERX_TESTNET_RESET_TOKEN_FILE`; CronJob mounts `/run/layerx/control-admin/token` (`platform/hosted/testnet/deployment.yaml:221`; `platform/hosted/testnet/reset-testnet.sh:6`) | Bearer on `POST /admin/v1/testnet/reset` | Public routes |
| Status publish token | File `LAYERX_STATUS_TOKEN_FILE`; CronJob mounts `/run/layerx/status/token` (`platform/hosted/testnet/deployment.yaml:248`; `platform/hosted/testnet/render-status.sh:7`) | Bearer on `PUT` to the status publisher | Testnet-control HTTP |

The faucet mounts the same control-admin Secret as
`LAYERX_TESTNET_ADMIN_TOKEN_FILE`
(`platform/hosted/testnet/deployment.yaml:148`;
`platform/hosted/testnet/deployment.yaml:166`;
`platform/hosted/testnet/deployment.yaml:171`). Bring-up writes
`control-admin.token` and `backend-admin.token` as distinct files
(`platform/hosted/tests/beta-cluster.sh:489-490`;
`platform/hosted/tests/beta-cluster.sh:582-583`). Testnet-control
sets `LAYERX_TESTNET_IDENTITY_URL` and does not set an identity
service-token env (`platform/hosted/testnet/deployment.yaml:92`;
`platform/hosted/testnet/deployment.yaml:83-101`;
`docs/wiki/HostedIdentity.md`). Identity is probed with
unauthenticated `GET /readyz` only
(`platform/hosted/testnet/src/main.rs:1032-1037`;
`platform/hosted/testnet/src/main.rs:1097`).

---

## Public routes

`public_route` accepts `GET` only. Any other method is `404 not_found`
(`platform/hosted/testnet/src/main.rs:1141-1143`).

| Method and path | Inputs | Result |
| --- | --- | --- |
| `GET /livez` | none | `200` `{"status":"live"}` (`platform/hosted/testnet/src/main.rs:1146`) |
| `GET /readyz` | none | `200` readiness document when every dependency, every journey, and the release gate are ready; else `503` with the same document (`platform/hosted/testnet/src/main.rs:1147-1152`; `platform/hosted/testnet/src/main.rs:614-624`) |
| `GET /v1/status` | none | `200` four-component public status (`platform/hosted/testnet/src/main.rs:1154`; `platform/hosted/testnet/src/main.rs:627-655`) |
| `GET /v1/parameters` | none | `200` `network`, `network_id`, `package_semver`, `lxp_wire_protocol_version`, `reset_schedule` (`platform/hosted/testnet/src/main.rs:1155-1163`) |
| `GET /v1/journeys/funding` | none | Journey admission (`platform/hosted/testnet/src/main.rs:229`; `platform/hosted/testnet/src/main.rs:1165-1171`) |
| `GET /v1/journeys/payment` | none | Journey admission (`platform/hosted/testnet/src/main.rs:230`) |
| `GET /v1/journeys/receipt-inspection` | none | Journey admission (`platform/hosted/testnet/src/main.rs:231`) |
| `GET /v1/journeys/programs` | none | Journey admission (`platform/hosted/testnet/src/main.rs:232`) |

`GET /v1/journeys/settlement` is not a route
(`platform/hosted/testnet/src/main.rs:1498`). Unknown GET paths are
`404 not_found` (`platform/hosted/testnet/src/main.rs:1173`).

`GET /v1/parameters` body:

- `network` `layerx-testnet`
- `network_id` `TESTNET_NETWORK_ID` (`402`)
- `package_semver` from the running crate
- `lxp_wire_protocol_version` `LXP_WIRE_PROTOCOL_VERSION`
- `reset_schedule` `09:00 UTC on the first Tuesday of every month`

(`platform/hosted/testnet/src/main.rs:1155-1163`;
`platform/hosted/testnet/src/lib.rs:3-4`;
`platform/hosted/testnet/src/lib.rs:79`). ConfigMap
`layerx-testnet-release` stores `reset-schedule`
`0 9 * * 2; operator executes only on days 1-7`
(`platform/hosted/testnet/deployment.yaml:11`). Those two schedule
strings differ. The parameters handler does not read the ConfigMap.

`GET /v1/status` keys are `service`, `state`, `package_semver`,
`lxp_wire_protocol_version`, `network_id`, `components`
(`platform/hosted/testnet/src/main.rs:687-694`;
`platform/hosted/testnet/src/main.rs:1626-1636`). `service` is
`layerx-hosted-testnet`. `components` is exactly `testnet`,
`gateway`, `core`, `paxeer`, each `{name, state}`
(`platform/hosted/testnet/src/main.rs:642-654`;
`platform/hosted/testnet/src/main.rs:1642-1651`). Component `testnet`
is `ready` or `degraded` from the release gate. `gateway`, `core`,
and `paxeer` are `ready` or `unavailable` from the matching
dependency probe. Gateway `GET /v1/status` is a different shape
(`docs/wiki/HostedGateway.md`).

---

## Journeys and probes

A journey is ready only when every dependency in its declared set
probes successfully (`platform/hosted/testnet/src/main.rs:242-268`;
`platform/hosted/testnet/src/main.rs:455-472`).

| Journey | Route | Dependencies |
| --- | --- | --- |
| `funding` | `/v1/journeys/funding` | `identity`, `faucet`, `redis`, `core_admin`, `core` (`platform/hosted/testnet/src/main.rs:244-250`) |
| `payment` | `/v1/journeys/payment` | `identity`, `gateway`, `core`, `receipt_authority` (`platform/hosted/testnet/src/main.rs:251-256`) |
| `receipt_inspection` | `/v1/journeys/receipt-inspection` | `gateway`, `receipt_authority`, `core` (`platform/hosted/testnet/src/main.rs:257-261`) |
| `programs` | `/v1/journeys/programs` | `gateway`, `registry`, `core`, `receipt_authority` (`platform/hosted/testnet/src/main.rs:262-267`) |

Admission JSON flattens `JourneyView` plus `admitted`
(`platform/hosted/testnet/src/main.rs:711-716`;
`platform/hosted/testnet/src/main.rs:486-490`). Ready is HTTP 200 with
`admitted` true, `ready` true, empty `failing`. Degraded is HTTP 503
with `admitted` false and `failing` names
(`platform/hosted/testnet/src/main.rs:1165-1171`). Hosted smoke
requires `.admitted == true and .ready == true and (.failing | length) == 0`
(`platform/hosted/testnet/tests/hosted-smoke.sh:38-49`).

Probe kinds (`platform/hosted/testnet/src/main.rs:1094-1105`):

| Dependency | Probe |
| --- | --- |
| `identity`, `faucet`, `core`, `receipt_authority`, `gateway`, `paxeer` | Unauthenticated `GET /readyz`, HTTP 200 only (`platform/hosted/testnet/src/main.rs:1032-1037`) |
| `core_admin` | TLS handshake, no HTTP (`platform/hosted/testnet/src/main.rs:1040-1044`) |
| `registry` | TCP connect; the detail string names client-certificate TLS beyond the probe (`platform/hosted/testnet/src/main.rs:1047-1053`) |
| `redis` | TLS then `PING`; `+PONG` or `-NOAUTH` is success (`platform/hosted/testnet/src/main.rs:1056-1091`) |

Global `/readyz` is ready only when all nine dependencies succeed,
all four journeys are ready, and `ReleaseGate::verify` matches
package semver and wire version
(`platform/hosted/testnet/src/main.rs:519-555`;
`platform/hosted/testnet/src/main.rs:1556-1569`). A Paxeer miss
degrades global state while leaving journeys ready
(`platform/hosted/testnet/src/main.rs:1571-1579`). The readiness
document includes `release`, `dependencies`, and `journeys`
(`platform/hosted/testnet/src/main.rs:728-737`). `state` is `ready`
or `degraded` (`platform/hosted/testnet/src/main.rs:562-567`).

`config` refuses boot when `TestnetConfig::validate` fails
(`platform/hosted/testnet/src/main.rs:794-799`;
`platform/hosted/testnet/src/lib.rs:33-52`). The readiness document
can still represent a degraded release in tests
(`platform/hosted/testnet/src/main.rs:1599-1619`). Those two gates
differ.

In-cluster URLs (`platform/hosted/testnet/deployment.yaml:88-96`):

- Core `https://layerx-pending-core.layerx-testnet.svc.cluster.local:9443`
- Core admin `https://layerx-pending-core-admin.layerx-testnet.svc.cluster.local:9444`
- Gateway `https://layerx-gateway.layerx-testnet.svc.cluster.local:443`
- Paxeer `https://paxeer-boundary.layerx-testnet.svc.cluster.local:9443`
- Identity `https://layerx-identity.layerx-testnet.svc.cluster.local:9443`
- Faucet `https://layerx-faucet-public.layerx-testnet.svc.cluster.local:443`
- Receipt authority `https://layerx-receipt-authority.layerx-testnet.svc.cluster.local:9443`
- Registry `https://layerx-program-registry.layerx-testnet.svc.cluster.local:9420`
- Redis `rediss://layerx-faucet-redis.layerx-testnet.svc.cluster.local:6379`

Testnet-control does not write Redis. It shares the faucet Redis
listener for the `redis` probe. Persistence for claims is the faucet
store (`docs/wiki/HostedFaucet.md`). Persistence for fund/reset is
the core journal (`docs/wiki/HostedCore.md`).

---

## Admin routes

Served only on the admin listener
(`platform/hosted/testnet/src/main.rs:1342-1346`;
`platform/hosted/testnet/src/main.rs:1381-1392`). There is no admin
`/livez` or `/readyz`. Deployment probes use the public port
(`platform/hosted/testnet/deployment.yaml:103-104`).

Every admin request requires the inbound Bearer, then
`Content-Type: application/json`, then `Idempotency-Key` 1–128
alnum/`-`/`_`/`.`/`:` (`platform/hosted/testnet/src/main.rs:1194-1225`;
`platform/hosted/testnet/src/main.rs:1202-1225`).

| Method and path | Inputs | Upstream |
| --- | --- | --- |
| `POST /admin/v1/testnet/fund` | JSON `funding_id`, `did`, `public_key`, `amount` with `deny_unknown_fields` (`platform/hosted/testnet/src/main.rs:740-747`; `platform/hosted/testnet/src/main.rs:1228-1255`) | Core-admin `POST /admin/v1/testnet/fund` with the backend admin token and the same body and idempotency key |
| `POST /admin/v1/testnet/reset` | JSON `{}` (`platform/hosted/testnet/src/main.rs:1257-1264`) | Core-admin `POST /admin/v1/testnet/reset` |

Fund validation: `funding_id` is a valid key; `did` starts with
`did:` and length ≤ 512; `public_key` is 64 ASCII hex; `amount != 0`
(`platform/hosted/testnet/src/main.rs:1235-1248`). Failure is
`400 invalid_argument`. Before proxying, the handler probes the
funding journey; a degraded set is `503 journey_degraded` with
`journey`, `failing`, and `retry` `after`
(`platform/hosted/testnet/src/main.rs:1250-1253`;
`platform/hosted/testnet/src/main.rs:427-435`). Reset does not probe
journeys. Reset body other than `{}` is `400 invalid_argument`.

Upstream HTTP 200 or 202 is returned unchanged. Upstream 400–499 is
returned unchanged. Any other outcome is
`503 core_unavailable` with `retry` `after`
(`platform/hosted/testnet/src/main.rs:1268-1281`). Testnet-control
does not rewrite core 5xx to 422. Core admin itself rewrites many
admin 5xx to 422 (`docs/wiki/HostedCore.md`). Those two mappings
differ.

---

## Treasury SEND funding path

The faucet does not submit a SEND. It posts the funding command to
testnet-control admin (`platform/hosted/faucet/src/main.rs:861-876`;
`platform/hosted/testnet/deployment.yaml:147`). Testnet-control
authenticates the control-admin token, validates the command, requires
the funding journey, and POSTs the same JSON to

`https://layerx-pending-core-admin.layerx-testnet.svc.cluster.local:9444/admin/v1/testnet/fund`

with `LAYERX_TESTNET_BACKEND_ADMIN_TOKEN_FILE`
(`platform/hosted/testnet/src/main.rs:1255`;
`platform/hosted/testnet/src/main.rs:1268-1274`;
`platform/hosted/testnet/deployment.yaml:89`).

Core `fund` / `fund_send` builds an owner-authorised Asset SEND
(ordinal `SEND_ACTIVITY` `5`) from the treasury seed, submits it on
LNI, and waits for a receipt
(`platform/hosted/core/src/lib.rs:19-20`;
`platform/hosted/core/src/lib.rs:105-110`;
`platform/hosted/core/src/main.rs:1471-1571`;
`docs/wiki/HostedCore.md`). Source and destination accounts are
`agent:<did>:main`. A 200 core body is `funding_id`, `state`
`funded`, `transaction_id`. A 202 core body is `state` `pending`.
The faucet accepts only `state == "funded"` as
`FundingResult::Funded`; `pending` is `Unknown` → `202 still_checking`
(`platform/hosted/faucet/src/main.rs:889-890`;
`platform/hosted/faucet/src/main.rs:990`).

Core additionally requires `did == did:layerx:` plus the lowercase
public key and refuses the treasury DID
(`platform/hosted/core/src/main.rs:1475-1484`). Testnet-control does
not apply those two checks. The faucet does not either. Those three
DID alphabets differ.

---

## Config keys

| Key | Role |
| --- | --- |
| `LAYERX_TESTNET_PUBLIC_LISTEN` | Public bind; default `0.0.0.0:9443` (`platform/hosted/testnet/src/main.rs:801-804`; `platform/hosted/testnet/deployment.yaml:86`) |
| `LAYERX_TESTNET_ADMIN_LISTEN` | Admin bind; default `0.0.0.0:9444` (`platform/hosted/testnet/src/main.rs:805-808`; `platform/hosted/testnet/deployment.yaml:87`) |
| `LAYERX_TESTNET_TLS_CERT_DER` | Inbound server certificate DER; mounted `/run/layerx/tls/server.crt.der` |
| `LAYERX_TESTNET_TLS_KEY_DER` | Inbound PKCS#8 key DER; mounted `/run/layerx/tls/server.key.der` |
| `LAYERX_OUTBOUND_CA_DER` | Trust bundle; mounted `/run/layerx/tls/ca.crt.der` |
| `LAYERX_PENDING_PACKAGE_SEMVER` | Pending package; ConfigMap `layerx-testnet-release` key `package-semver` (`platform/hosted/testnet/src/main.rs:788-789`; `platform/hosted/testnet/deployment.yaml:84`) |
| `LAYERX_PENDING_WIRE_PROTOCOL_VERSION` | Pending wire `u16`; ConfigMap key `lxp-wire-protocol-version` (`platform/hosted/testnet/src/main.rs:790-793`; `platform/hosted/testnet/deployment.yaml:85`) |
| `LAYERX_TESTNET_CORE_URL` | Core HTTPS origin |
| `LAYERX_TESTNET_CORE_ADMIN_URL` | Core-admin HTTPS origin |
| `LAYERX_TESTNET_GATEWAY_URL` | Gateway HTTPS origin |
| `LAYERX_TESTNET_PAXEER_URL` | Paxeer-boundary HTTPS origin |
| `LAYERX_TESTNET_IDENTITY_URL` | Identity HTTPS origin |
| `LAYERX_TESTNET_FAUCET_URL` | Faucet HTTPS origin |
| `LAYERX_TESTNET_RECEIPT_AUTHORITY_URL` | Receipt-authority HTTPS origin |
| `LAYERX_TESTNET_REGISTRY_URL` | Program-registry HTTPS origin |
| `LAYERX_TESTNET_REDIS_URL` | `rediss://` origin |
| `LAYERX_TESTNET_BACKEND_ADMIN_TOKEN_FILE` | Bearer to core-admin; mounted `/run/layerx/backend-admin/token` |
| `LAYERX_TESTNET_CONTROL_ADMIN_TOKEN_FILE` | Inbound admin Bearer; mounted `/run/layerx/control-admin/token` |

Secret files are read, trailing CR/LF stripped, and refused when empty
or longer than 4096 bytes (`platform/hosted/testnet/src/main.rs:749-759`).
Volume mounts are TLS Secret `layerx-testnet-control-tls` at
`/run/layerx/tls`, `layerx-testnet-backend-admin` at
`/run/layerx/backend-admin`, and `layerx-testnet-control-admin` at
`/run/layerx/control-admin`
(`platform/hosted/testnet/deployment.yaml:107-114`).

`run-testnet.sh` requires the same keys then `exec`s the binary
(`platform/hosted/testnet/run-testnet.sh:3-24`). The Deployment does
not use that script.

---

## Cluster CronJobs

CronJob `layerx-testnet-reset` runs `/opt/layerx/reset-testnet.sh`
(`platform/hosted/testnet/deployment.yaml:200-225`). The script POSTs
`{}` to `$LAYERX_TESTNET_ADMIN_URL/admin/v1/testnet/reset` with the
reset token and one idempotency key per calendar month, and requires
JSON `state == "reset"` with a non-empty `reset_id`
(`platform/hosted/testnet/reset-testnet.sh:13-33`). It exits `0`
without calling the admin URL when the UTC day-of-month is greater
than 7 (`platform/hosted/testnet/reset-testnet.sh:10-12`).

CronJob `layerx-testnet-status-publisher` runs
`/opt/layerx/render-status.sh`
(`platform/hosted/testnet/deployment.yaml:227-252`). The script GETs
`$LAYERX_TESTNET_STATUS_URL` (`/v1/status` on
`layerx-testnet-public`), checks the four-component shape, and PUTs
that body to `$LAYERX_STATUS_PUBLISH_URL`
(`platform/hosted/testnet/render-status.sh:15-38`).
`status.json` lists the same four component ids
(`platform/hosted/testnet/status.json:3-8`). The status publisher
Service is not in this repository; `topology-check.sh` reports that
edge as `external` (`platform/hosted/tests/topology-check.sh:29-32`).

---

## Callers the NetworkPolicy admits

Admin ingress admits TCP 9444 from `app=layerx-faucet` and
`app=layerx-testnet-reset`. Public ingress admits TCP 9443 from
namespace `ingress-nginx` and `app=layerx-testnet-status-publisher`
(`platform/hosted/testnet/deployment.yaml:254-264`). Egress is
`layerx-plane: trusted-boundary` on 9443 / 9444 / 9445, gateway 9443,
faucet 9443, faucet-redis 6379, program-registry 9420, and DNS 53
(`platform/hosted/testnet/deployment.yaml:266-284`). Gateway ingress
admits `app=layerx-testnet-control` on 9443
(`docs/wiki/HostedGateway.md`). Node ingress admits
`app=layerx-testnet-control` on 9443 / 9444 / 9445
(`platform/hosted/node/deployment.yaml:273-274`).

`topology-check.sh` default manifests include
`platform/hosted/testnet/deployment.yaml`
(`platform/hosted/tests/topology-check.sh:21`;
`platform/hosted/tests/topology-check.sh:87`). Beta-cluster apply
order is testnet, then gateway, then registry, then developer
(`platform/hosted/tests/beta-cluster.sh:868-872`).

---

## Typed refusals

Public and admin JSON refusals use `{"error":{"code":…}}`
(`platform/hosted/testnet/src/main.rs:1143`;
`platform/hosted/testnet/src/main.rs:1204-1280`). There is no
`Retry-After` header (`platform/hosted/testnet/src/main.rs:1295-1310`).
Faucet refusals set `Retry-After` when a delay is present
(`docs/wiki/HostedFaucet.md`). Those two header behaviors differ.

| HTTP | Code | Listener | Condition |
| --- | --- | --- | --- |
| 400 | `invalid_request` | both | Request framing failed (`platform/hosted/testnet/src/main.rs:1334-1339`) |
| 400 | `content_type_required` | admin | `Content-Type` is not `application/json` (`platform/hosted/testnet/src/main.rs:1209-1212`) |
| 400 | `idempotency_key_required` | admin | Missing `Idempotency-Key` (`platform/hosted/testnet/src/main.rs:1215-1218`) |
| 400 | `invalid_idempotency_key` | admin | Key fails `valid_key` (`platform/hosted/testnet/src/main.rs:1221-1224`) |
| 400 | `invalid_argument` | admin | Fund JSON/fields rejected, or reset body is not `{}` (`platform/hosted/testnet/src/main.rs:1229-1262`) |
| 401 | `unauthorized` | admin | Missing or mismatched Bearer (`platform/hosted/testnet/src/main.rs:1203-1207`) |
| 404 | `not_found` | public | Non-GET, or unknown GET path (`platform/hosted/testnet/src/main.rs:1142-1143`; `platform/hosted/testnet/src/main.rs:1173`) |
| 404 | `not_found` | admin | Method/path is not fund or reset (`platform/hosted/testnet/src/main.rs:1266`) |
| 503 | `journey_degraded` | admin | Funding journey probe failed (`platform/hosted/testnet/src/main.rs:1250-1253`) |
| 503 | `core_unavailable` | admin | Upstream not 200/202/4xx (`platform/hosted/testnet/src/main.rs:1278-1280`) |

`GET /readyz` and journey GETs use 503 with the readiness or
admission document, not `core_unavailable`. JSON serialization
failure is `500` `serialization_failure`
(`platform/hosted/testnet/src/main.rs:1285-1291`). Reason phrase for
any unlisted status is `Service Unavailable`
(`platform/hosted/testnet/src/main.rs:1296-1302`).

---

## Tests

Portable crate tests (no cluster):

| Test | Proves |
| --- | --- |
| `package_and_wire_versions_are_independent_release_gates` | `TestnetConfig::validate` accepts matching pending package and wire, refuses either mismatch (`platform/hosted/testnet/src/lib.rs:88-107`) |
| `dependency_reports_are_indexed_by_dependency` | Reports index equals `Dependency::ALL` (`platform/hosted/testnet/src/main.rs:1435-1442`) |
| `journeys_declare_their_dependency_sets` | Exact four journeys and four routes; settlement is absent (`platform/hosted/testnet/src/main.rs:1444-1498`) |
| `journey_readiness_is_the_conjunction_of_its_declared_dependencies` | Identity miss degrades only journeys that declare it (`platform/hosted/testnet/src/main.rs:1501-1529`) |
| `degraded_journey_names_every_failing_dependency_once` | Funding names `faucet` then `core`; refusal code `journey_degraded` (`platform/hosted/testnet/src/main.rs:1532-1552`) |
| `global_ready_needs_every_journey_every_dependency_and_the_release` | Nine dependencies and four journeys; Paxeer miss degrades global state only (`platform/hosted/testnet/src/main.rs:1555-1595`) |
| `release_mismatch_degrades_the_global_state_without_touching_journeys` | Package mismatch → public `testnet` component `degraded` (`platform/hosted/testnet/src/main.rs:1598-1619`) |
| `public_status_keeps_the_published_four_component_shape` | Keys and component names `testnet`, `gateway`, `core`, `paxeer` (`platform/hosted/testnet/src/main.rs:1622-1654`) |
| `redis_endpoint_requires_the_rediss_scheme` | `rediss://` only, no path; `https` parse rejects `rediss` (`platform/hosted/testnet/src/main.rs:1657-1672`) |

Hosted smoke (`platform/hosted/testnet/tests/hosted-smoke.sh`):

- Testnet `GET /readyz` is 200, `state == "ready"`, nine named
  dependencies ready, four journeys ready
  (`platform/hosted/testnet/tests/hosted-smoke.sh:52-65`)
- `GET /v1/parameters` is 200 and matches readiness network id,
  package, and wire
  (`platform/hosted/testnet/tests/hosted-smoke.sh:89-96`)
- Journeys `funding`, `payment`, `receipt-inspection`, `programs`
  are admitted in that order
  (`platform/hosted/testnet/tests/hosted-smoke.sh:101`;
  `platform/hosted/testnet/tests/hosted-smoke.sh:113`;
  `platform/hosted/testnet/tests/hosted-smoke.sh:134`;
  `platform/hosted/testnet/tests/hosted-smoke.sh:151`)

Make:

| Target | Testnet use |
| --- | --- |
| `platform-test` | workspace `cargo test`, including `layerx-platform-testnet` (`platform/Makefile.inc:112-113`) |
| `platform-test-tooling` | `cargo test -p layerx-platform-testnet` and `sh -n` of `run-testnet.sh`, `reset-testnet.sh`, `render-status.sh`, `hosted-smoke.sh` (`platform/Makefile.inc:119-121`) |
| `platform-hosted-smoke` | requires `LAYERX_TESTNET_URL` and `LAYERX_FAUCET_URL` (`platform/Makefile.inc:161-173`) |
| `platform-hosted-topology-check` | default manifests include testnet (`platform/Makefile.inc:177-178`; `platform/hosted/tests/topology-check.sh:21`) |
| `platform-beta-cluster-up` | waits until testnet `GET /readyz` has `state == "ready"` and every journey ready (`platform/hosted/tests/beta-cluster.sh:1286-1287`) |

[Home](Home.md)

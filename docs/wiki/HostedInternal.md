# Hosted internal

`layerx-kms` is the Ed25519 signing boundary behind
`kms.layerx-internal.svc`. Keys are generated inside the process from
operating-system entropy, sealed at rest under the operator seal
secret, and never leave the process: the API returns an opaque handle
and the public key, and signs messages on request
(`platform/hosted/internal/src/kms.rs:1-8`;
`platform/hosted/internal/src/lib.rs:1-4`). `layerx-event-source` is the
durable observe adapter behind `journeys`, `payments`, `approvals`, and
`programs.layerx-internal.svc` (`platform/hosted/internal/src/lib.rs:1-4`;
`platform/hosted/internal/src/events.rs:16-35`). The crate is
`layerx-platform-internal`. The binaries are `layerx-kms` and
`layerx-event-source`. The platform string is
`layerx-internal-signing-boundary-and-verified-event-sources`
(`platform/hosted/internal/Cargo.toml:2`;
`platform/hosted/internal/Cargo.toml:11-17`;
`platform/hosted/internal/src/lib.rs:21-23`).

The image is `ghcr.io/sidiora-labs/layerx-internal:0.1.0`, user
`4020:4020`. The build produces both binaries. The image `ENTRYPOINT`
is `/usr/local/bin/layerx-kms`; event-source Deployments set
`command: [/usr/local/bin/layerx-event-source]`
(`platform/hosted/internal/Dockerfile:5-11`;
`platform/hosted/internal/deployment.yaml:99-100`;
`platform/hosted/internal/deployment.yaml:160-161`).

`platform-test` runs `cargo test` on the platform workspace, which
lists `hosted/internal` (`platform/Makefile.inc:112-113`;
`platform/Cargo.toml:12`). `platform/Makefile.inc` has no
crate-specific internal test target. `platform-hosted-topology-check`
runs `topology-check.sh` (`platform/Makefile.inc:177-178`).
`platform-test-tooling` syntax-checks `topology-check.sh` and
`beta-cluster.sh` (`platform/Makefile.inc:123-126`).

---

## Namespace and apply

Namespace `layerx-internal` carries label `layerx.internal-events: "true"`
and a default-deny NetworkPolicy for Ingress and Egress
(`platform/hosted/internal/deployment.yaml:3`;
`platform/hosted/internal/deployment.yaml:6-8`). The same manifest
declares Redis, the KMS Deployment, four event-source Deployments, and
ExternalName aliases `identity`, `component`, and `authority` that
point at `internal-identity`, `internal-component`, and
`internal-authority` in `layerx-testnet`
(`platform/hosted/internal/deployment.yaml:29-63`;
`platform/hosted/internal/deployment.yaml:86-124`;
`platform/hosted/internal/deployment.yaml:147-188`;
`platform/hosted/internal/deployment.yaml:219-260`;
`platform/hosted/internal/deployment.yaml:291-332`;
`platform/hosted/internal/deployment.yaml:363-404`;
`platform/hosted/internal/deployment.yaml:429-457`).

Each of KMS, journeys, payments, approvals, and programs is one
replica, strategy `Recreate`, `runAsUser`/`runAsGroup`/`fsGroup`
`4020`, read-only root filesystem, and a 1Gi `ReadWriteOnce` PVC
mounted at `/var/lib/layerx`. Service port `443` targets container
`9443`. HTTPS probes are `/readyz` and `/livez`
(`platform/hosted/internal/deployment.yaml:82-119`;
`platform/hosted/internal/deployment.yaml:89-90`;
`platform/hosted/internal/deployment.yaml:109-111`;
`platform/hosted/internal/deployment.yaml:149-151`;
`platform/hosted/internal/deployment.yaml:173-175`).

`beta-cluster.sh` names `INTERNAL_NAMESPACE=layerx-internal` and puts
`authority.`, `component.`, and `identity.` under
`$INTERNAL_NAMESPACE.svc` on the receipt-authority, agent-boundary, and
identity certificates (`platform/hosted/tests/beta-cluster.sh:92`;
`platform/hosted/tests/beta-cluster.sh:380-390`). `IMAGE_NAMES` does
not include an internal image
(`platform/hosted/tests/beta-cluster.sh:89-90`). `manifests_render`
writes node, identity, paxeer, testnet, gateway, registry, and
developer YAML and does not read
`platform/hosted/internal/deployment.yaml`
(`platform/hosted/tests/beta-cluster.sh:820-826`). `manifests_apply`
applies testnet, gateway, registry, and developer only
(`platform/hosted/tests/beta-cluster.sh:859-864`). Those two sources
disagree on whether the cluster apply path installs the internal
workloads.

`topology-check.sh` default manifests are node, identity, paxeer,
testnet, gateway, and registry
(`platform/hosted/tests/topology-check.sh:17-24`;
`platform/hosted/tests/topology-check.sh:81-88`). That default set omits
`platform/hosted/internal/deployment.yaml` and
`platform/hosted/webhooks/deployment.yaml`.

---

## TLS and HTTP

Both binaries require `<prefix>_CLIENT_CA_DER` at start
(`platform/hosted/internal/src/bin/layerx-kms.rs:14-16`;
`platform/hosted/internal/src/bin/layerx-event-source.rs:15-17`). The
listener is rustls. When a client CA is configured, the verifier
`allow_unauthenticated` so kubelet probes can connect; the request
records whether a verified peer certificate was presented
(`platform/hosted/internal/src/tls.rs:26-30`;
`platform/hosted/internal/src/tls.rs:46-58`;
`platform/hosted/internal/src/http.rs:32-34`;
`platform/hosted/internal/src/http.rs:281-284`).

The parser is HTTP/1.1 only, one request per connection, no query
string, a Host header, no transfer encoding, no duplicate headers, and
a body exactly `Content-Length` long. Headers plus body are bounded at
96 KiB. I/O timeout is 8s. At most 128 connections are served; further
accepts are dropped (`platform/hosted/internal/src/http.rs:1-3`;
`platform/hosted/internal/src/http.rs:17-22`;
`platform/hosted/internal/src/http.rs:116-117`;
`platform/hosted/internal/src/http.rs:182-212`;
`platform/hosted/internal/src/http.rs:316-318`). Responses are JSON,
`Cache-Control: no-store`, `Connection: close`
(`platform/hosted/internal/src/http.rs:239-244`). A parse failure is
`400 invalid_request` (`platform/hosted/internal/src/http.rs:278-279`).

The refusal envelope is
`{"error":{"code":…,"retry":"never"|"after"}}`, with
`retry_after_seconds` when retry is `after`
(`platform/hosted/internal/src/http.rs:97-114`). Bearer tokens are
compared in constant time (`platform/hosted/internal/src/http.rs:50-56`).

Event-source upstreams are `https://` DNS origins, native-tls, TLS 1.2,
a pinned CA, connect timeout 5s, I/O timeout 8s, and a 512 KiB
response bound. Literal IP hosts are refused
(`platform/hosted/internal/src/tls.rs:21-24`;
`platform/hosted/internal/src/tls.rs:87-113`;
`platform/hosted/internal/src/tls.rs:240-244`).

---

## Who may call

KMS authenticated routes require a verified client certificate and
`Authorization: Bearer` equal to `LAYERX_KMS_TOKEN_FILE` (minimum 16
bytes). Missing peer certificate is `401 client_certificate_required`;
bearer mismatch is `401 unauthorized`
(`platform/hosted/internal/src/bin/layerx-kms.rs:21`;
`platform/hosted/internal/src/kms.rs:295-302`;
`platform/hosted/internal/src/secret.rs:13-14`;
`platform/hosted/internal/src/secret.rs:57-64`). `GET /livez` and
`GET /readyz` do not authenticate (`platform/hosted/internal/src/kms.rs:416-417`).

Event-source `GET /livez` and `GET /readyz` do not authenticate. Every
other route requires a verified client certificate and
`Authorization: Bearer` equal to `LAYERX_EVENTS_TOKEN_FILE`. Either
failure is `401 unauthorized`
(`platform/hosted/internal/src/events.rs:243-255`).

NetworkPolicy ingress to KMS and to each event source admits
`layerx-developer` pods `app=layerx-webhooks` and `layerx-testnet` pods
`app=layerx-testnet-control` on TCP 9443. KMS egress is empty.
Event-source egress is kube-dns UDP/TCP 53 and `layerx-testnet` pods
`app` in `layerx-human`, `layerx-gateway` on TCP 9443 and 443
(`platform/hosted/internal/deployment.yaml:132-139`;
`platform/hosted/internal/deployment.yaml:196-211`;
`platform/hosted/internal/deployment.yaml:268-283`;
`platform/hosted/internal/deployment.yaml:340-355`;
`platform/hosted/internal/deployment.yaml:412-427`).

Webhooks dials `https://kms.layerx-internal.svc` with
`LAYERX_WEBHOOKS_KMS_TOKEN_FILE` and a PKCS#12 client identity, and
dials each event source with
`LAYERX_WEBHOOKS_{JOURNEY|PAYMENT|APPROVAL|PROGRAM}_SOURCE_TOKEN_FILE`
(`platform/hosted/webhooks/deployment.yaml:8`;
`platform/hosted/webhooks/deployment.yaml:12-15`;
`platform/hosted/webhooks/deployment.yaml:31`;
`platform/hosted/webhooks/deployment.yaml:35-38`;
`platform/hosted/webhooks/src/hosted.rs:720-727`;
`platform/hosted/webhooks/src/trusted.rs:139-154`;
`platform/hosted/webhooks/src/trusted.rs:219-227`). Webhooks egress
lists UDP 53, TCP 443, and TCP 6379 with no pod peer
(`platform/hosted/webhooks/deployment.yaml:265-269`).

`layerx-testnet-control` egress admits trusted-boundary, gateway,
faucet, faucet-redis, program-registry, and DNS, and does not list
`layerx-internal` (`platform/hosted/testnet/deployment.yaml:268-284`).
No path under `platform/hosted/testnet` names `kms.layerx-internal.svc`
or the event-source hosts. The internal ingress rule and the
testnet-control egress rule disagree on that edge.

Redis ingress admits `layerx-developer` pods `app` in `layerx-webhooks`,
`layerx-dashboard-api` and `layerx-testnet` pods `app=layerx-gateway` on
TCP 6379; Redis egress is empty
(`platform/hosted/internal/deployment.yaml:71-78`).

---

## KMS request and response

| Method and path | Input | Success |
| --- | --- | --- |
| `GET /livez` | none | `200` `{"alive":true}` (`platform/hosted/internal/src/kms.rs:416`) |
| `GET /readyz` | none | `200` or `503` `{"ready":…,"ed25519_non_exportable":true}` (`platform/hosted/internal/src/kms.rs:305-316`) |
| `POST /v1/signing-keys` | Bearer, client cert, `Idempotency-Key` (token, max 128), JSON `{"algorithm":"ed25519","purpose":…}` (`deny_unknown_fields`, purpose token max 64) | `201` fresh or `200` existing `{"key_id","handle","public_key"}` with `public_key` standard base64 (`platform/hosted/internal/src/kms.rs:319-355`; `platform/hosted/internal/src/kms.rs:47-52`; `platform/hosted/internal/src/kms.rs:432-437`) |
| `GET /v1/signing-keys/{key_id}` | Bearer, client cert, `key_id` token max 64 | `200` same key body (`platform/hosted/internal/src/kms.rs:358-371`; `platform/hosted/internal/src/kms.rs:420-422`) |
| `POST /v1/signatures` | Bearer, client cert, JSON `{"key_handle","algorithm":"ed25519","message"}` (`deny_unknown_fields`, `message` standard padded base64, max 64 KiB decoded) | `200` `{"signature":…}` base64 (`platform/hosted/internal/src/kms.rs:374-409`; `platform/hosted/internal/src/kms.rs:54-60`; `platform/hosted/internal/src/kms.rs:29-30`) |

Scope for create is `{purpose}:{Idempotency-Key}`. The request digest is
SHA-256 of the raw body. The same scope and digest returns the existing
key; the same scope with a different digest is `409 idempotency_conflict`
(`platform/hosted/internal/src/kms.rs:342-350`;
`platform/hosted/internal/src/kms.rs:181-199`). `key_id` is `whk_` plus
12 random hex bytes; `handle` is `kms-ed25519-` plus 16 random hex bytes
(`platform/hosted/internal/src/kms.rs:25-28`;
`platform/hosted/internal/src/kms.rs:207-209`). The store holds at most
100_000 keys (`platform/hosted/internal/src/kms.rs:31`;
`platform/hosted/internal/src/kms.rs:201-202`).

Webhooks `KmsClient` posts `purpose` `layerx-webhook-v1` to
`/v1/signing-keys` and `/v1/signatures`, accepts status `200` or `201`
on create, and verifies the signature against the returned public key
(`platform/hosted/webhooks/src/hosted.rs:496-570`). It does not call
`GET /v1/signing-keys/{key_id}`.

---

## Event source request and response

`LAYERX_EVENTS_KIND` is one of `journeys`, `approvals`, `payments`,
`programs` (`platform/hosted/internal/src/events.rs:28-35`;
`platform/hosted/internal/src/bin/layerx-event-source.rs:14`).

| Kind | Upstream URL in the manifest | Upstream GET | Principal credential to upstream |
| --- | --- | --- | --- |
| `journeys` | `https://layerx-human.layerx-testnet.svc:9443` (`platform/hosted/internal/deployment.yaml:169-170`) | `/v1/journeys/{resource}` (`platform/hosted/internal/src/events.rs:37-44`) | `Cookie: __Host-layerx_access=…` (`platform/hosted/internal/src/events.rs:46-51`) |
| `approvals` | `https://layerx-human.layerx-testnet.svc:9443` (`platform/hosted/internal/deployment.yaml:313-314`) | `/v1/approvals/{resource}` (`platform/hosted/internal/src/events.rs:40`) | same cookie |
| `payments` | `https://layerx-gateway.layerx-testnet.svc` (`platform/hosted/internal/deployment.yaml:241-242`) | `/v1/receipts/{resource}` (`platform/hosted/internal/src/events.rs:41`) | `Authorization: LayerX-Key …` (`platform/hosted/internal/src/events.rs:52-56`) |
| `programs` | `https://layerx-gateway.layerx-testnet.svc` (`platform/hosted/internal/deployment.yaml:385-386`) | `/v1/programs/registry/{resource}` (`platform/hosted/internal/src/events.rs:42`) | `Authorization: LayerX-Key …` |

No `layerx-human` workload is declared under `platform/hosted` other
than as that NetworkPolicy peer and those two upstream URLs.

| Method and path | Input | Success |
| --- | --- | --- |
| `GET /livez` | none | `200` `{"alive":true}` (`platform/hosted/internal/src/events.rs:243-244`) |
| `GET /readyz` | none | `200` or `503` `{"ready":…}` (`platform/hosted/internal/src/events.rs:246-251`) |
| `POST /internal/v1/observe` | client cert, Bearer, JSON `{"principal","resource"}` (`deny_unknown_fields`), `Content-Type` starting `application/json`, `resource` identifier max 128 | `200` `Record` (`platform/hosted/internal/src/events.rs:147-152`; `platform/hosted/internal/src/events.rs:256-261`; `platform/hosted/internal/src/events.rs:68-83`; `platform/hosted/internal/src/http.rs:59-64`) |
| `GET /internal/v1/events/{id}` | client cert, Bearer, `id` 32 lowercase hex bytes | `200` `Record` (`platform/hosted/internal/src/events.rs:263-283`) |

`POST /internal/v1/observe` without a JSON content type, and any other
unmatched method or path, is `404 not_found`
(`platform/hosted/internal/src/events.rs:256`;
`platform/hosted/internal/src/events.rs:286`). Observe maps every
internal `Err` to `503 source_unavailable` with `retry_after_seconds`
5 (`platform/hosted/internal/src/events.rs:258-259`).

No source file under `platform/hosted/*/src` or `human/crates` issues
`POST /internal/v1/observe`. The route is defined only at
`platform/hosted/internal/src/events.rs:256`. Webhooks
`POST /internal/v1/events/{kind}/{id}` authorizes
`LAYERX_WEBHOOKS_SOURCE_TRIGGER_TOKEN_FILE` and then `GET`s
`/internal/v1/events/{id}` on the matching source
(`platform/hosted/webhooks/src/main.rs:329-338`;
`platform/hosted/webhooks/src/trusted.rs:207-230`).

Observe binds the principal, fetches the kind route, and derives a
record (`platform/hosted/internal/src/events.rs:226-238`). Bind GETs
`/internal/v1/principal` on the same upstream. Journeys and approvals
require `result.active == true` and `result.sub == principal`.
Payments and programs require `result.principal_digest` equal to
SHA-256 hex of the principal
(`platform/hosted/internal/src/events.rs:204-211`;
`platform/hosted/internal/src/events.rs:290-297`). Upstream responses
must be HTTP 200, `Content-Type` starting `application/json`,
`ok: true`, and a `result` object
(`platform/hosted/internal/src/events.rs:181-202`).

Human `GET /internal/v1/principal` is the session cookie
`__Host-layerx_access` and returns envelope `ok` / `result`
`{"active":true,"sub":…}`
(`human/crates/layerx-human-service/src/server/http.rs:20`;
`human/crates/layerx-human-service/src/server/http.rs:141`;
`human/crates/layerx-human-service/src/server/http.rs:278-284`;
`human/crates/layerx-human-service/src/server/http.rs:644-654`).
Gateway `GET /internal/v1/principal` is `Authorization: LayerX-Key`
and returns `result.principal_digest`
(`platform/hosted/gateway/src/main.rs:1726-1738`;
`platform/hosted/gateway/src/main.rs:1162-1173`).

Derived facts, after requiring `result.{identity} == resource`:

| Kind | Identity field | Copied facts |
| --- | --- | --- |
| Journey | `journey_id` | `kind`, `state`, `updated_at` (`platform/hosted/internal/src/events.rs:319`) |
| Approval | `approval_id` | `agent_id`, `state`, `created_at` (`platform/hosted/internal/src/events.rs:320`) |
| Program | `program_id` | `lifecycle`, `version`, `code_hash`, `receipt_digest` (`platform/hosted/internal/src/events.rs:321-324`) |
| Payment | `activity_id` | none as facts; `receipt` hex is decoded as a protocol receipt; `activity_id`, `amount`, `asset`, and `occurred_at` are taken from that receipt (`platform/hosted/internal/src/events.rs:325`; `platform/hosted/internal/src/events.rs:341-359`) |

Human `Journey` requires `journey_id`, `kind`, `state`, `updated_at`
among other fields (`human/schema/human-api/journeys.kvx:36-37`;
`human/schema/human-api/journeys.kvx:57-62`). Human `ApprovalDetail`
requires `approval_id`, `agent_id`, `state`, `created_at` among other
fields (`human/schema/human-api/baseline.kvx:222`;
`human/schema/human-api/agents.kvx:203-208`). Gateway receipt `result`
is `activity_id` and hex `receipt`
(`platform/hosted/gateway/src/main.rs:2526-2532`). Gateway program
registry `result` includes `program_id`, `lifecycle`, `version`,
`code_hash`, and `receipt_digest`
(`platform/hosted/gateway/src/main.rs:2549-2559`).

`Record.id` is SHA-256 hex of the JSON encoding of
`(principal, resource, snapshot)`
(`platform/hosted/internal/src/events.rs:305-308`). GET of an event
re-runs bind on the stored principal; bind failure is
`503 source_unavailable` (`platform/hosted/internal/src/events.rs:276-280`).

---

## Durability and ordering

The journal is `journal.log` under the state directory: one JSON line
per record, `write_all` then `sync_all` before the caller observes
success, directory mode `0o700`, file mode `0o600`, exclusive lock,
replay in append order on open. A failed append marks the journal
unavailable. Record bound 64 KiB; journal bound 1_000_000 records.
Readiness writes and syncs `ready.marker` in the same directory
(`platform/hosted/internal/src/journal.rs:1-4`;
`platform/hosted/internal/src/journal.rs:12-15`;
`platform/hosted/internal/src/journal.rs:36-48`;
`platform/hosted/internal/src/journal.rs:110-156`).

Event `subject_sequence` is per `(principal, subject)`, starting at 1.
A duplicate `id` returns the stored record without appending. Replay
refuses invalid hex ids, invalid principals, invalid subjects, more
than 32 facts, sequence gaps, or duplicate ids
(`platform/hosted/internal/src/events.rs:99-136`;
`platform/hosted/internal/src/events.rs:103-108`).

KMS seeds are AES-256-GCM sealed under
`SealKey::derive(b"layerx-kms-ed25519-seed", seal_secret)` with a fresh
12-byte nonce; the journal stores `sealed_seed` hex, not a `seed`
field. Open authenticates every sealed seed and checks it against the
journaled public key. The wrong seal secret refuses open
(`platform/hosted/internal/src/kms.rs:23-24`;
`platform/hosted/internal/src/kms.rs:114-140`;
`platform/hosted/internal/src/seal.rs:18-23`;
`platform/hosted/internal/src/seal.rs:31-48`). Sign opens the sealed
seed, checks the public key, signs, and self-verifies
(`platform/hosted/internal/src/kms.rs:242-256`).

---

## Readiness

KMS `/readyz` is `200` when the store lock is held and
`probe_writable` succeeds; otherwise `503`. The body always sets
`ed25519_non_exportable` to `true`
(`platform/hosted/internal/src/kms.rs:305-316`).

Event-source `/readyz` is `200` only when upstream `GET /readyz` is
200, bind succeeds for every provisioned principal, and the journal is
writable (`platform/hosted/internal/src/events.rs:213-224`). Open
refuses an empty principal map, more than 10_000 principals, or an
invalid principal (`platform/hosted/internal/src/events.rs:165-171`).

---

## Configuration

| Variable | Role |
| --- | --- |
| `LAYERX_KMS_LISTEN` | KMS TLS bind (`platform/hosted/internal/src/bin/layerx-kms.rs:8-10`; `platform/hosted/internal/deployment.yaml:102`) |
| `LAYERX_KMS_STATE_DIR` | Key journal directory (`platform/hosted/internal/src/bin/layerx-kms.rs:13`; `platform/hosted/internal/deployment.yaml:103`) |
| `LAYERX_KMS_TOKEN_FILE` | Bearer secret, min 16 bytes (`platform/hosted/internal/src/bin/layerx-kms.rs:11`; `platform/hosted/internal/deployment.yaml:104`) |
| `LAYERX_KMS_TLS_CERT_DER` | Server certificate DER (`platform/hosted/internal/src/tls.rs:36-37`; `platform/hosted/internal/deployment.yaml:105`) |
| `LAYERX_KMS_TLS_KEY_DER` | PKCS#8 key DER (`platform/hosted/internal/src/tls.rs:38-39`; `platform/hosted/internal/deployment.yaml:106`) |
| `LAYERX_KMS_CLIENT_CA_DER` | Required client CA DER (`platform/hosted/internal/src/bin/layerx-kms.rs:14-16`; `platform/hosted/internal/deployment.yaml:107`) |
| `LAYERX_KMS_SEAL_SECRET_FILE` | Seal secret (`platform/hosted/internal/src/bin/layerx-kms.rs:12`; `platform/hosted/internal/deployment.yaml:108`) |
| `LAYERX_EVENTS_LISTEN` | Event-source TLS bind (`platform/hosted/internal/src/bin/layerx-event-source.rs:11-13`; `platform/hosted/internal/deployment.yaml:163`) |
| `LAYERX_EVENTS_STATE_DIR` | Event journal directory (`platform/hosted/internal/src/bin/layerx-event-source.rs:33`; `platform/hosted/internal/deployment.yaml:164`) |
| `LAYERX_EVENTS_TOKEN_FILE` | Bearer secret, min 16 bytes (`platform/hosted/internal/src/bin/layerx-event-source.rs:38`; `platform/hosted/internal/deployment.yaml:165`) |
| `LAYERX_EVENTS_TLS_CERT_DER` | Server certificate DER (`platform/hosted/internal/deployment.yaml:166`) |
| `LAYERX_EVENTS_TLS_KEY_DER` | PKCS#8 key DER (`platform/hosted/internal/deployment.yaml:167`) |
| `LAYERX_EVENTS_CLIENT_CA_DER` | Required client CA DER (`platform/hosted/internal/src/bin/layerx-event-source.rs:15-17`; `platform/hosted/internal/deployment.yaml:168`) |
| `LAYERX_EVENTS_KIND` | `journeys` \| `approvals` \| `payments` \| `programs` (`platform/hosted/internal/src/bin/layerx-event-source.rs:14`; `platform/hosted/internal/deployment.yaml:169`) |
| `LAYERX_EVENTS_UPSTREAM_URL` | Bare `https://` origin (`platform/hosted/internal/src/tls.rs:152-156`; `platform/hosted/internal/deployment.yaml:170`) |
| `LAYERX_EVENTS_UPSTREAM_CA_DER` | Upstream CA DER (`platform/hosted/internal/src/tls.rs:157-160`; `platform/hosted/internal/deployment.yaml:171`) |
| `LAYERX_EVENTS_CREDENTIALS_FILE` | JSON map of principal to secret path, max 1_048_576 bytes (`platform/hosted/internal/src/bin/layerx-event-source.rs:18-32`; `platform/hosted/internal/deployment.yaml:172`) |
| `LAYERX_EVENTS_UPSTREAM_TOKEN_FILE` | Optional default upstream Bearer (`platform/hosted/internal/src/tls.rs:161-164`) |
| `LAYERX_EVENTS_UPSTREAM_COOKIE_FILE` | Optional default `__Host-layerx_access` cookie (`platform/hosted/internal/src/tls.rs:185-188`) |
| `LAYERX_EVENTS_UPSTREAM_CLIENT_IDENTITY_PKCS12` | Optional upstream PKCS#12 (`platform/hosted/internal/src/tls.rs:165-179`) |
| `LAYERX_EVENTS_UPSTREAM_CLIENT_IDENTITY_PASSWORD_FILE` | Required when the PKCS#12 path is set (`platform/hosted/internal/src/tls.rs:167-170`) |

Manifest listen is `0.0.0.0:9443`; state is `/var/lib/layerx/state`;
secrets are files under `/run/layerx`
(`platform/hosted/internal/deployment.yaml:102-108`). Observe and GET
event use per-principal credentials from
`LAYERX_EVENTS_CREDENTIALS_FILE` via `get_as`, not the optional
default upstream token or cookie
(`platform/hosted/internal/src/events.rs:181-189`;
`platform/hosted/internal/src/tls.rs:277-281`).

Redis in the same namespace listens TLS 6379, `tls-auth-clients no`,
`appendonly yes`, `appendfsync always`
(`platform/hosted/internal/deployment.yaml:14-26`). Webhooks and
dashboard ConfigMap values point at `rediss://redis.layerx-internal.svc:6379`
(`platform/hosted/webhooks/deployment.yaml:7`;
`platform/hosted/webhooks/deployment.yaml:44`).

---

## Refusals

Shared HTTP parse failure is `400 invalid_request`
(`platform/hosted/internal/src/http.rs:278-279`).

| Code | Status | Retry | Source |
| --- | --- | --- | --- |
| `client_certificate_required` | 401 | never | KMS authenticated routes without a verified peer cert (`platform/hosted/internal/src/kms.rs:296-297`) |
| `unauthorized` | 401 | never | KMS bearer mismatch (`platform/hosted/internal/src/kms.rs:299-300`); event-source missing peer cert or bearer (`platform/hosted/internal/src/events.rs:253-254`) |
| `idempotency_key_required` | 400 | never | KMS create (`platform/hosted/internal/src/kms.rs:323-328`) |
| `invalid_request` | 400 | never | KMS create/sign missing JSON or body (`platform/hosted/internal/src/kms.rs:330-334`; `platform/hosted/internal/src/kms.rs:378-382`) |
| `invalid_message` | 400 | never | KMS sign message not canonical base64 (`platform/hosted/internal/src/kms.rs:390-391`) |
| `unsupported_algorithm` | 422 | never | KMS algorithm not `ed25519` (`platform/hosted/internal/src/kms.rs:336-337`; `platform/hosted/internal/src/kms.rs:384-385`) |
| `invalid_purpose` | 422 | never | KMS purpose (`platform/hosted/internal/src/kms.rs:339-340`) |
| `idempotency_conflict` | 409 | never | KMS same scope, different body digest (`platform/hosted/internal/src/kms.rs:350`) |
| `unknown_key` | 404 | never | KMS lookup or sign (`platform/hosted/internal/src/kms.rs:362-369`; `platform/hosted/internal/src/kms.rs:387-404`) |
| `message_too_large` | 413 | never | KMS sign (`platform/hosted/internal/src/kms.rs:393-394`) |
| `method_not_allowed` | 405 | never | KMS wrong method on known paths (`platform/hosted/internal/src/kms.rs:424-425`) |
| `not_found` | 404 | never | KMS unmatched path (`platform/hosted/internal/src/kms.rs:427`); event-source unmatched path or non-JSON observe (`platform/hosted/internal/src/events.rs:286`) |
| `dependency_unavailable` | 503 | 5 | KMS lock, create, or sign failure (`platform/hosted/internal/src/kms.rs:344-353`; `platform/hosted/internal/src/kms.rs:365-366`; `platform/hosted/internal/src/kms.rs:396-407`) |
| `source_unavailable` | 503 | 5 | Event observe `Err` or GET bind failure (`platform/hosted/internal/src/events.rs:258-259`; `platform/hosted/internal/src/events.rs:277-278`) |
| `event_not_found` | 404 | never | Event GET unknown id (`platform/hosted/internal/src/events.rs:274-275`) |
| `serialization_failed` | 500 | never | JSON helper (`platform/hosted/internal/src/http.rs:86-88`) |

---

## Tests

`keys_are_idempotent_sealed_and_durable` proves create is idempotent
on the same digest, conflicts on a different digest, signatures verify,
unknown handles return `None`, the journal contains hex public key and
`sealed_seed` and not `"seed":`, reopen restores the key, and a wrong
seal secret fails authentication
(`platform/hosted/internal/src/kms.rs:456-523`).

`real_tls_kms_refuses_missing_credentials_and_preserves_signing_identity_after_restart`
drives the real `layerx-kms` binary over TLS: `/readyz` is `200` with
`ed25519_non_exportable` without a client cert; create without a client
cert or with the wrong bearer is `401`; create with both is `201`;
repeat is `200` with the same body and no `seed`; after kill and
respawn the same create is `200` with that body; a signature verifies
and a tampered message does not
(`platform/hosted/internal/tests/kms_tls.rs:257-324`).

`journal_preserves_immutable_event_order_and_deduplicates_observations`
proves first append sequence 1, duplicate id stays 1, a different
snapshot gets sequence 2, reopen keeps both, identity mismatch and a
malformed payment receipt refuse derive
(`platform/hosted/internal/src/events.rs:387-431`).
`source_credentials_cannot_be_reassigned_to_a_foreign_principal` proves
journey/approval `sub` and `active`, and payment/program
`principal_digest`, reject a foreign principal
(`platform/hosted/internal/src/events.rs:367-384`).

`appends_are_replayed_in_order_after_reopen` and
`corrupt_lines_refuse_to_open` cover the journal
(`platform/hosted/internal/src/journal.rs:195-249`).
`request_parser_rejects_unbounded_and_ambiguous_messages` and
`refusal_bodies_follow_the_hosted_contract` cover HTTP
(`platform/hosted/internal/src/http.rs:340-398`).
`origins_are_bare_https_dns_names` and
`responses_must_be_framed_by_content_length` cover TLS client parsing
(`platform/hosted/internal/src/tls.rs:357-396`). Seal tests prove
round-trip, key binding, fresh nonces, and tamper refusal
(`platform/hosted/internal/src/seal.rs:125-158`). Base64 tests prove
padding round-trips and reject non-canonical input
(`platform/hosted/internal/src/base64.rs:96-119`). Identifier tests
prove principal, token, and lowercase hex rules
(`platform/hosted/internal/src/secret.rs:201-215`).

[Home](Home.md)

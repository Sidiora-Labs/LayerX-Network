# Hosted webhooks

`layerx-webhooks` is durable outbound delivery of protocol events to
subscriber HTTPS endpoints
(`platform/hosted/webhooks/src/lib.rs:1-12`;
`platform/hosted/webhooks/Cargo.toml:2, 11-13`). The crate is
`layerx-platform-webhooks`. The platform string is
`signed-ordered-at-least-once-webhook-delivery-with-dead-letter-and-redelivery`
(`platform/hosted/webhooks/src/lib.rs:35-38`).

It delivers journey, payment, approval, and program events
(`platform/hosted/webhooks/src/lib.rs:1-2`;
`platform/hosted/webhooks/src/events.rs:84-96`). Deliveries are signed
Ed25519 under scheme `LayerX/webhooks/v1`, ordered per subject, retried
with bounded deterministic backoff, dead-lettered when attempts are
exhausted, and replayable from stable cursors
(`platform/hosted/webhooks/src/lib.rs:4-9`;
`platform/hosted/webhooks/src/scheme.rs:42`). Nothing is reported
`delivered` without an accepting status from the developer's own
endpoint (`platform/hosted/webhooks/src/lib.rs:9-10`;
`platform/hosted/webhooks/src/deliveries.rs:47-48`).

This page covers that binary, its Redis, KMS, event-source fetch, and the
tests under `platform/hosted/webhooks/`. It does not document the
dashboard.

---

## Deployment

The image is `ghcr.io/sidiora-labs/layerx-webhooks:0.1.0`
(`platform/hosted/webhooks/deployment.yaml:69`;
`platform/hosted/tests/beta-cluster.sh:118`). Dashboard images in the
same list also use `ghcr.io/sidiora-labs/...`
(`platform/hosted/tests/beta-cluster.sh:119-120`). Testnet, gateway, faucet, registry, node, boundary, identity, and paxd
images in that list use `ghcr.io/sidiora-labs/...`
(`platform/hosted/tests/beta-cluster.sh:114-117, 121-128`). These images share the same
registry prefix. The Dockerfile builds
`-p layerx-platform-webhooks --bin layerx-webhooks`, copies
`/src/platform/target/release/layerx-webhooks`, sets `USER 65532:65532`,
and entrypoint `/usr/local/bin/layerx-webhooks`
(`platform/hosted/webhooks/Dockerfile:4-10`). The Deployment sets
`runAsNonRoot: true` without `runAsUser`
(`platform/hosted/webhooks/deployment.yaml:80-83`). Those two user
bindings differ.

Deployment `layerx-webhooks` has three replicas, listens on
`0.0.0.0:9444`, exposes Service port `443` to container `9444`, PDB
`minAvailable` `2`
(`platform/hosted/webhooks/deployment.yaml:6, 61, 74, 195-198, 250-251`).
Ingress `layerx-developer` host `developers.layerx.example` path
`/v1/webhooks` uses backend protocol HTTPS and body size `512k`
(`platform/hosted/webhooks/deployment.yaml:217-232`). NetworkPolicy
ingress admits `ingress-nginx` and namespaces labeled
`layerx.internal-events: "true"` on TCP `9444`; egress is UDP `53`,
TCP `443`, and TCP `6379`
(`platform/hosted/webhooks/deployment.yaml:253-269`). Namespace
`layerx-internal` carries that label
(`platform/hosted/internal/deployment.yaml:3`).

Beta-cluster `IMAGE_NAMES` includes `layerx-webhooks`
(`platform/hosted/tests/beta-cluster.sh:89`). Render writes
`platform/hosted/webhooks/deployment.yaml` as `developer.yaml`
(`platform/hosted/tests/beta-cluster.sh:826`). Bring-up port-forwards
Service `layerx-webhooks` `19450:443` and exports `WEBHOOKS_URL`
(`platform/hosted/tests/beta-cluster.sh:1252, 1272, 1117`).
`--boundary-checks` runs `platform/hosted/webhooks/tests/fault-injection.sh`
(`platform/hosted/tests/beta-cluster.sh:49-50, 1184-1195`).

Default `topology-check.sh` manifests are node, identity, paxeer,
testnet, gateway, and registry. They do not include
`platform/hosted/webhooks/deployment.yaml` or
`platform/hosted/internal/deployment.yaml`
(`platform/hosted/tests/topology-check.sh:17-23, 81-88`). Node
NetworkPolicy admits `layerx-webhooks` from namespace
`layerx-developer` on TCP `9445` only
(`platform/hosted/node/deployment.yaml:278-279`). Receipt-authority
container port `authority-tls` is `9445`; agent-boundary
`agent-tls` is `9446`
(`platform/hosted/node/deployment.yaml:181, 210`).

---

## Subscriptions

A subscription is one `StoredEndpoint` under an authenticated
developer `Principal` (`platform/hosted/webhooks/src/hosted.rs:43-64`;
`platform/hosted/webhooks/src/events.rs:145-160`). Construction reuses
the hosted gateway `PrincipalId` rule
(`platform/hosted/webhooks/src/events.rs:145-159`).

Who may create one: a caller whose session introspects as `active`
with `sub` accepted as `Principal`
(`platform/hosted/webhooks/src/trusted.rs:369-428`;
`platform/hosted/webhooks/src/main.rs:167-174, 387-390`).
Authentication is `Authorization: Bearer` or cookie
`__Host-layerx-session`. Cookie `POST` and `DELETE` require header
`x-layerx-csrf` equal to the introspected `csrf_token`
(`platform/hosted/webhooks/src/trusted.rs:376-426, 577-591`;
`platform/hosted/webhooks/src/main.rs:171-172`). Failed
authentication is `401 session_required`
(`platform/hosted/webhooks/src/main.rs:387-390`). Introspection is
`POST /v1/sessions/introspect` to `LAYERX_WEBHOOKS_IDENTITY_URL` with
the identity bearer (`platform/hosted/webhooks/src/trusted.rs:393-404`).

`POST /v1/webhooks/endpoints` requires header `Idempotency-Key`
(`platform/hosted/webhooks/src/main.rs:189-191`). Body fields are
`url`, optional `kinds`, optional `minimum_verification`
(`platform/hosted/webhooks/src/main.rs:37-45`). Missing kinds is every
family; missing minimum is `unverified`
(`platform/hosted/webhooks/src/hosted.rs:67-70`;
`platform/hosted/webhooks/src/main.rs:205-211`;
`platform/hosted/webhooks/src/endpoints.rs:86-89`). The destination
must be `https://` with a canonical DNS name, not a literal IP, and
must resolve only to public addresses
(`platform/hosted/webhooks/src/boundary.rs:27-51, 198-215, 99-147`;
`platform/hosted/webhooks/src/hosted.rs:797-800`). At most 32 endpoints
per principal; a second registration of the same URL is
`EventConflict` (`platform/hosted/webhooks/src/hosted.rs:25, 819-824`).
The same idempotency scope returns the existing registration
(`platform/hosted/webhooks/src/hosted.rs:807-817`). Success is `201`
with endpoint id, key id, public key, and `receiver_obligation`
(`platform/hosted/webhooks/src/main.rs:212-215`;
`platform/hosted/webhooks/src/hosted.rs:230-251`).

Endpoint identifiers are `whep_` plus 16 random bytes as hex
(`platform/hosted/webhooks/src/events.rs:240-246`). Signing keys are
created at KMS `POST /v1/signing-keys` with algorithm `ed25519` and
purpose `layerx-webhook-v1`; key ids must start with `whk_`
(`platform/hosted/webhooks/src/hosted.rs:495-535`;
`platform/hosted/webhooks/src/scheme.rs:66`). Rotation announces a
pending key that activates after
`LAYERX_WEBHOOKS_KEY_OVERLAP_SECONDS` (default `86400`)
(`platform/hosted/webhooks/src/hosted.rs:749, 884-911`;
`platform/hosted/webhooks/deployment.yaml:22`).

`POST .../suspensions` sets `suspended` with a reason of 1..=256 bytes
and no CR/LF/NUL (`platform/hosted/webhooks/src/main.rs:267-276`;
`platform/hosted/webhooks/src/hosted.rs:960-979`).
`POST .../resumptions` clears suspension and consecutive dead letters
(`platform/hosted/webhooks/src/hosted.rs:985-995`). Dispatch skips a
suspended endpoint (`platform/hosted/webhooks/src/hosted.rs:1137-1139`).

---

## Event contracts

Four families:

| Kind | Wire word | Source facts | Receipt path |
| --- | --- | --- | --- |
| Journey | `journey` | `journey_id` must equal the resource; facts `kind`, `state`, `updated_at` (`platform/hosted/internal/src/events.rs:318-339`; `platform/hosted/webhooks/src/events.rs:88-89, 115-116`) | Optional `activity_id` on the source record (`platform/hosted/webhooks/src/trusted.rs:247-290`) |
| Payment | `payment` | Source `activity_id`, `amount`, `asset` required; amount digits only (`platform/hosted/webhooks/src/trusted.rs:252-267`; `platform/hosted/webhooks/src/events.rs:630-650`; `platform/hosted/internal/src/events.rs:325, 341-359`) | Required. `settled_payment` refuses empty receipt bytes, non-zero `result_code`, or a level weaker than `receipt-verified` (`platform/hosted/webhooks/src/events.rs:622-642`) |
| Approval | `approval` | `approval_id` must equal the resource; facts `agent_id`, `state`, `created_at` (`platform/hosted/internal/src/events.rs:320, 330-339`; `platform/hosted/webhooks/src/events.rs:92-93, 117-118`) | Optional `activity_id` |
| Program | `program` | `program_id` must equal the resource; facts `lifecycle`, `version`, `code_hash`, `receipt_digest` (`platform/hosted/internal/src/events.rs:321-324, 330-339`; `platform/hosted/webhooks/src/events.rs:94-95, 119-120`) | Optional `activity_id` |

Source `Kind::parse` accepts `journeys`, `approvals`, `payments`,
`programs` (`platform/hosted/internal/src/events.rs:28-35`). Webhook
`EventKind::parse` accepts `journey`, `approval`, `payment`, `program`
(`platform/hosted/webhooks/src/events.rs:114-121`). Those two
vocabularies differ. Internal observe routes are `/v1/journeys`,
`/v1/approvals`, `/v1/receipts`, `/v1/programs/registry`
(`platform/hosted/internal/src/events.rs:37-44`). Journey and approval
credentials are cookie `__Host-layerx_access`; payment and program
credentials are `Authorization: LayerX-Key`
(`platform/hosted/internal/src/events.rs:46-57`).

Observe `POST /internal/v1/observe` derives a record, journals it, and
returns it (`platform/hosted/internal/src/events.rs:226-261`). Payment
observe decodes the upstream receipt, sets `activity_id` to the
resource, `amount` from `receipt.amount()`, `asset` from hex of
`receipt.asset()`, and `occurred_at` from `receipt.timestamp()`
(`platform/hosted/internal/src/events.rs:341-359`). GET
`/internal/v1/events/{id}` returns that immutable record when the id
is 32 hex (`platform/hosted/internal/src/events.rs:263-284`).

Webhooks do not call observe. They fetch `GET /internal/v1/events/{id}`
from the per-kind source URL after
`POST /internal/v1/events/{kind}/{source_event}` with the source-trigger
bearer (`platform/hosted/webhooks/src/trusted.rs:207-241`;
`platform/hosted/webhooks/src/main.rs:329-339`). `layerx-event-source`
serves `Service::route` only
(`platform/hosted/internal/src/bin/layerx-event-source.rs:41-46`). The
in-repo caller of the webhook publish POST is
`platform/hosted/webhooks/tests/fault-injection.sh:25-43`.

Payment facts written by `settled_payment`:

| Fact | Level | Source |
| --- | --- | --- |
| `state` | `receipt-verified` or stronger | `"settled"` (`platform/hosted/webhooks/src/events.rs:647-648`) |
| `amount` | `unverified` | canonical payment source; not promoted (`platform/hosted/webhooks/src/events.rs:614-619, 649`) |
| `asset` | `unverified` | canonical payment source (`platform/hosted/webhooks/src/events.rs:617-619, 650`) |
| `activity_id` | operation level | hex of verified activity id (`platform/hosted/webhooks/src/events.rs:651-656`) |
| `receipt_bytes` | operation level | receipt length as decimal (`platform/hosted/webhooks/src/events.rs:657-662`) |

Event header verification is the weakest fact
(`platform/hosted/webhooks/src/events.rs:476-477, 490-495`). Because
amount and asset stay `unverified`, a payment event's header level is
`unverified`. An endpoint with `minimum_verification` above
`unverified` therefore does not accept payment events
(`platform/hosted/webhooks/src/hosted.rs:67-70`).

Non-payment source facts are stored `unverified`. When `activity_id`
is present they are joined by verified `activity_id` and `result_code`
(`platform/hosted/webhooks/src/trusted.rs:269-290`). Gateway
`VerifiedOperation::verification_level` is the string
`receipt-verified` (`platform/hosted/gateway/src/lib.rs:490-493`).

Displayed levels:

| Level | Wire word |
| --- | --- |
| `Unverified` | `unverified` |
| `ReceiptVerified` | `receipt-verified` |
| `CheckpointFinalised` | `checkpoint-finalised` |
| `PaxeerFinalised` | `paxeer-finalised` |

(`platform/hosted/webhooks/src/events.rs:18-40`). A fact above
`unverified` must carry a 64-hex receipt digest
(`platform/hosted/webhooks/src/events.rs:77-81, 353-361, 426-437`).

Publish admits an event into the principal Redis shard, refuses reuse
of an id with different content (`EventConflict`), refuses a
non-advancing subject sequence (`OrderViolation`), then enqueues one
`Pending` delivery per accepting endpoint before any HTTP POST
(`platform/hosted/webhooks/src/hosted.rs:1003-1075, 1569-1599`).

---

## Trusted-component credential

Inbound TLS is rustls, no client authentication, cert
`LAYERX_WEBHOOKS_TLS_CERT_DER`, PKCS#8 key
`LAYERX_WEBHOOKS_TLS_KEY_DER`
(`platform/hosted/webhooks/src/main.rs:75-97`). Outbound trusted HTTPS
uses `native_tls` with internal CA
`LAYERX_WEBHOOKS_INTERNAL_CA_DER` and PKCS#12
`LAYERX_WEBHOOKS_CLIENT_IDENTITY_PKCS12`
(`platform/hosted/webhooks/src/trusted.rs:118-137`;
`platform/hosted/webhooks/src/hosted.rs:691-699`;
`platform/hosted/webhooks/src/boundary.rs:236-242`). Public deliveries
use `LAYERX_WEBHOOKS_PUBLIC_CA_DER` and no client identity
(`platform/hosted/webhooks/src/hosted.rs:700-745`).

| Credential | Presented to | As |
| --- | --- | --- |
| PKCS#12 client identity | identity, KMS, event sources, component, authority | mTLS (`platform/hosted/webhooks/src/trusted.rs:127-137, 163-164`; `platform/hosted/webhooks/src/hosted.rs:720-721`) |
| `LAYERX_WEBHOOKS_IDENTITY_TOKEN_FILE` | `LAYERX_WEBHOOKS_IDENTITY_URL` `POST /v1/sessions/introspect` | `Authorization: Bearer` (`platform/hosted/webhooks/src/trusted.rs:327-331, 393-402`) |
| `LAYERX_WEBHOOKS_KMS_TOKEN_FILE` | `LAYERX_WEBHOOKS_KMS_URL` `/v1/signing-keys`, `/v1/signatures`, `/readyz` | `Authorization: Bearer` (`platform/hosted/webhooks/src/hosted.rs:720-726, 502-512, 544-555, 573-583`) |
| `LAYERX_WEBHOOKS_{JOURNEY,PAYMENT,APPROVAL,PROGRAM}_SOURCE_TOKEN_FILE` | matching `LAYERX_WEBHOOKS_*_SOURCE_URL` `GET /internal/v1/events/{id}` and `GET /readyz` | `Authorization: Bearer` (`platform/hosted/webhooks/src/trusted.rs:139-154, 191-201, 220-230`) |
| `LAYERX_WEBHOOKS_COMPONENT_TOKEN_FILE` | `LAYERX_WEBHOOKS_COMPONENT_URL` `GET /internal/v1/receipts/{activity}` and `GET /readyz` | `Authorization: Bearer` (`platform/hosted/webhooks/src/trusted.rs:165-169, 478-493, 457-475`) |
| `LAYERX_WEBHOOKS_AUTHORITY_TOKEN_FILE` | `LAYERX_WEBHOOKS_AUTHORITY_URL` `GET /internal/v1/activities/{activity}/authority` and `GET /readyz` | `Authorization: Bearer` (`platform/hosted/webhooks/src/trusted.rs:170-174, 481-505, 457-475`) |
| `LAYERX_WEBHOOKS_SOURCE_TRIGGER_TOKEN_FILE` | callers of `POST /internal/v1/events/...` | compared to inbound `Authorization: Bearer` (`platform/hosted/webhooks/src/trusted.rs:431-452`; `platform/hosted/webhooks/src/main.rs:329-334`) |
| `LAYERX_WEBHOOKS_OPERATOR_TOKEN_FILE` | callers of `POST /internal/v1/dispatch` | compared to inbound `Authorization: Bearer` (`platform/hosted/webhooks/src/trusted.rs:441-445`; `platform/hosted/webhooks/src/main.rs:341-347`) |
| Redis username and password files | `LAYERX_WEBHOOKS_REDIS_URL` | Redis `AUTH` (`platform/hosted/webhooks/src/hosted.rs:447-454, 733-741`) |
| `LAYERX_WEBHOOKS_SEQUENCER_PUBLIC_KEY_FILE` | not sent; compared to authority `sequencer_public_key` | 32-byte hex (`platform/hosted/webhooks/src/trusted.rs:156-176, 528-531`) |
| `LAYERX_AUTHORITY_TOKEN_FILES` entry `/run/layerx/webhooks-authority/token` | receipt-authority inbound | Secret `layerx-webhooks-authority-client` (`platform/hosted/node/deployment.yaml:174, 192, 236`; `platform/hosted/tests/beta-cluster.sh:614`) |

ConfigMap URLs
(`platform/hosted/webhooks/deployment.yaml:7-15`):

| Key | Value |
| --- | --- |
| `LAYERX_WEBHOOKS_REDIS_URL` | `rediss://redis.layerx-internal.svc:6379` |
| `LAYERX_WEBHOOKS_KMS_URL` | `https://kms.layerx-internal.svc` |
| `LAYERX_WEBHOOKS_IDENTITY_URL` | `https://identity.layerx-internal.svc` |
| `LAYERX_WEBHOOKS_COMPONENT_URL` | `https://component.layerx-internal.svc` |
| `LAYERX_WEBHOOKS_AUTHORITY_URL` | `https://authority.layerx-internal.svc` |
| `LAYERX_WEBHOOKS_JOURNEY_SOURCE_URL` | `https://journeys.layerx-internal.svc` |
| `LAYERX_WEBHOOKS_PAYMENT_SOURCE_URL` | `https://payments.layerx-internal.svc` |
| `LAYERX_WEBHOOKS_APPROVAL_SOURCE_URL` | `https://approvals.layerx-internal.svc` |
| `LAYERX_WEBHOOKS_PROGRAM_SOURCE_URL` | `https://programs.layerx-internal.svc` |

`layerx-internal` ExternalName Services alias identity, component, and
authority into `layerx-testnet`
(`platform/hosted/internal/deployment.yaml:429-457`).
`internal-component` targets node port `agent-tls`
(`platform/hosted/internal/deployment.yaml:440-442`).
`internal-authority` targets `authority-tls`
(`platform/hosted/internal/deployment.yaml:449-452`). Node ingress
admits webhooks on `9445` and does not list `9446`
(`platform/hosted/node/deployment.yaml:278-279`).

`LAYERX_WEBHOOKS_NETWORK_ID` is `testnet`
(`platform/hosted/webhooks/deployment.yaml:16`). Receipt authority
`LAYERX_AUTHORITY_NETWORK_ID` is ConfigMap `network-name`
`layerx-testnet` (`platform/hosted/node/deployment.yaml:5-6, 169`).
`ReceiptVerifier::verify` refuses when `authority.network_id` is not
exactly `LAYERX_WEBHOOKS_NETWORK_ID`
(`platform/hosted/webhooks/src/trusted.rs:521-526`). Those two
network-id strings differ. `LAYERX_WEBHOOKS_LXP_WIRE_VERSION` is `"3"`
and must equal `STATE_COMMITMENT_PROTOCOL_VERSION` (`3`)
(`platform/hosted/webhooks/deployment.yaml:17`;
`platform/hosted/webhooks/src/trusted.rs:157-161`;
`agent/crates/layerx-wire/src/limits.rs:16`).

---

## Delivery semantics

Durability before transmission: `publish` compare-and-sets the Redis
shard with the event and `StoredDeliveryState::Pending` rows, then
returns (`platform/hosted/webhooks/src/hosted.rs:1003-1075, 1569-1599,
372-397`). Redis is `rediss://` only, TLS 1.2, then `AUTH`
(`platform/hosted/webhooks/src/hosted.rs:300-323, 439-454`). Dispatch
is a background loop every
`LAYERX_WEBHOOKS_DISPATCH_INTERVAL_SECONDS` (default `1`, must be
positive) with budget `LAYERX_WEBHOOKS_DISPATCH_BUDGET` (default `64`)
(`platform/hosted/webhooks/src/main.rs:111-116, 395-406`). `prepare`
moves a due row to `InFlight` with a lease before `send`
(`platform/hosted/webhooks/src/hosted.rs:1097-1162`).

Retry: `RetryPolicy` defaults eight attempts, base `10s`, cap `3600s`,
spread `20%`, suspend after `20` consecutive dead letters, in-flight
timeout `120s` (`platform/hosted/webhooks/src/endpoints.rs:26-38`;
`platform/hosted/webhooks/src/hosted.rs:704-717`). Backoff doubles from
the base, then spreads by the delivery digest
(`platform/hosted/webhooks/src/endpoints.rs:58-76`). Accepting status
is `200..300` (`platform/hosted/webhooks/src/hosted.rs:1270-1300`).
`410` is `FailureKind::Gone`, which is permanent
(`platform/hosted/webhooks/src/hosted.rs:1272`;
`platform/hosted/webhooks/src/deliveries.rs:40-44`). Redirects
`300..400` are `Refused` (`platform/hosted/webhooks/src/hosted.rs:1242-1244`).
Permanent failure or `attempt >= maximum_attempts` dead-letters
(`platform/hosted/webhooks/src/hosted.rs:1305-1321`). Consecutive dead
letters at the suspend bound set
`suspended_reason` to `consecutive dead letters reached the suspension bound`
(`platform/hosted/webhooks/src/hosted.rs:1315-1320`).

Delivery states (`platform/hosted/webhooks/src/deliveries.rs:49-91, 94-105`):

| State | Wire word | Meaning |
| --- | --- | --- |
| `Pending` | `pending` | Queued, never attempted |
| `InFlight` | `in-flight` | Attempt outstanding |
| `Retrying` | `retrying` | Last attempt failed, next scheduled |
| `Delivered` | `delivered` | Destination accepted |
| `DeadLettered` | `dead-lettered` | Attempts exhausted |

There is no `unknown` delivery state. Unattempted is `pending`.
Unknown endpoint and unknown delivery are typed refusals
(`platform/hosted/webhooks/src/error.rs:14-17`;
`platform/hosted/webhooks/src/main.rs:126-127`).
`FailureKind::Suspended` and `WebhookError::EndpointSuspended` exist
on the wire and in HTTP mapping
(`platform/hosted/webhooks/src/deliveries.rs:22-23, 36`;
`platform/hosted/webhooks/src/error.rs:20-21`;
`platform/hosted/webhooks/src/main.rs:129`). No path constructs either
value. A suspended endpoint is skipped
(`platform/hosted/webhooks/src/hosted.rs:1137-1139`).

Signature on deliveries: `send` signs
`canonical_message(event_id, timestamp, body)` via KMS
`POST /v1/signatures` and verifies the signature against the public
key before POST (`platform/hosted/webhooks/src/hosted.rs:1183-1225`;
`platform/hosted/webhooks/src/scheme.rs:113-120`;
`platform/hosted/webhooks/src/hosted.rs:538-570`). Authenticated
headers (`platform/hosted/webhooks/src/scheme.rs:3-10, 43-50`):

```text
layerx-webhook-id:        <event-id>
layerx-webhook-timestamp: <unix-seconds>
layerx-webhook-key-id:    <endpoint-signing-key-id>
layerx-webhook-signature: v1=<standard padded base64 of a 64-byte Ed25519 signature>
```

`layerx-webhook-id` is the event identifier, repeated on every attempt
and redelivery over a byte-identical body
(`platform/hosted/webhooks/src/scheme.rs:18-21`;
`platform/hosted/webhooks/src/hosted.rs:1193`). Operational headers
`layerx-webhook-delivery`, `attempt`, `kind`, `subject`, `sequence`,
`endpoint` are outside the signature
(`platform/hosted/webhooks/src/scheme.rs:28-33, 51-62`;
`platform/hosted/webhooks/src/hosted.rs:1203-1223`). Receiver
obligation is published at `GET /v1/webhooks/scheme` and on every
registration (`platform/hosted/webhooks/src/scheme.rs:75-76`;
`platform/hosted/webhooks/src/main.rs:373-382`). Default tolerance
`300s`, future skew `30s`
(`platform/hosted/webhooks/src/scheme.rs:67-70`). `ReplayGuard` admits
an id once inside the window; a repeat is `ReplayRejected`
(`platform/hosted/webhooks/src/scheme.rs:201-206, 231-252`).

Redelivery `POST .../redeliveries` enqueues from a signed cursor
(`platform/hosted/webhooks/src/hosted.rs:1401-1450, 1732-1761`). Dead
letter replay `POST /v1/webhooks/dead-letters/{id}/replay` requires
`DeadLettered` (`platform/hosted/webhooks/src/hosted.rs:1457-1506`;
`platform/hosted/webhooks/src/main.rs:310-320`).

---

## HTTP surface

Unauthenticated: `GET /healthz`, `GET /v1/webhooks/scheme`
(`platform/hosted/webhooks/src/main.rs:357-383`). Paths under
`/internal/` use source or operator bearers
(`platform/hosted/webhooks/src/main.rs:384-385, 326-354`). All other
routes require a session (`platform/hosted/webhooks/src/main.rs:387-390`).
Ingress `layerx-developer` path `/v1/webhooks` targets this Service
(`platform/hosted/webhooks/deployment.yaml:230-232`).

| Method | Path | Auth | Success |
| --- | --- | --- | --- |
| GET | `/healthz` | none | `200` or `503` (`platform/hosted/webhooks/src/main.rs:359-371`) |
| GET | `/v1/webhooks/scheme` | none | `200` (`platform/hosted/webhooks/src/main.rs:373-382`) |
| GET | `/v1/webhooks/endpoints` | session | `200` (`platform/hosted/webhooks/src/main.rs:176-184`) |
| POST | `/v1/webhooks/endpoints` | session + CSRF if cookie + `Idempotency-Key` | `201` (`platform/hosted/webhooks/src/main.rs:186-215`) |
| GET | `/v1/webhooks/endpoints/{id}/events` | session | `200` (`platform/hosted/webhooks/src/main.rs:231-239`) |
| GET | `/v1/webhooks/endpoints/{id}/keys` | session | `200` (`platform/hosted/webhooks/src/main.rs:240-243`) |
| POST | `/v1/webhooks/endpoints/{id}/keys` | session + `Idempotency-Key` | `201` (`platform/hosted/webhooks/src/main.rs:244-250`) |
| POST | `/v1/webhooks/endpoints/{id}/redeliveries` | session + `Idempotency-Key` | `202` (`platform/hosted/webhooks/src/main.rs:251-266`) |
| POST | `/v1/webhooks/endpoints/{id}/suspensions` | session | `200` (`platform/hosted/webhooks/src/main.rs:267-276`) |
| POST | `/v1/webhooks/endpoints/{id}/resumptions` | session | `200` (`platform/hosted/webhooks/src/main.rs:277-280`) |
| GET | `/v1/webhooks/events` | session | `200` (`platform/hosted/webhooks/src/main.rs:292-295`) |
| GET | `/v1/webhooks/deliveries` | session | `200` (`platform/hosted/webhooks/src/main.rs:296-302`) |
| GET | `/v1/webhooks/dead-letters` | session | `200` (`platform/hosted/webhooks/src/main.rs:303-309`) |
| POST | `/v1/webhooks/dead-letters/{id}/replay` | session + `Idempotency-Key` | `202` (`platform/hosted/webhooks/src/main.rs:310-320`) |
| POST | `/internal/v1/events/{kind}/{source_event}` | source-trigger bearer | `202` (`platform/hosted/webhooks/src/main.rs:329-339`) |
| POST | `/internal/v1/dispatch` | operator bearer | `200` (`platform/hosted/webhooks/src/main.rs:341-351`) |

Page `limit` defaults to `50`, clamped `1..=200`
(`platform/hosted/webhooks/src/main.rs:21, 159-165`). At most `256`
TLS connections (`platform/hosted/webhooks/src/main.rs:20, 413-416`).
Request cap `128 KiB` (`platform/hosted/webhooks/src/http.rs:8-9`).

---

## Refusals

`WebhookError` HTTP mapping (`platform/hosted/webhooks/src/main.rs:123-145`;
`platform/hosted/webhooks/src/error.rs:11-49`):

| Error | Status | Code | Retry-After |
| --- | --- | --- | --- |
| `InvalidRequest` | 400 | `invalid_request` | never |
| `UnknownEndpoint` | 404 | `unknown_endpoint` | never |
| `UnknownDelivery` | 404 | `unknown_delivery` | never |
| `NotDeadLettered` | 409 | `not_dead_lettered` | never |
| `EndpointSuspended` | 409 | `endpoint_suspended` | never |
| `EventConflict` | 409 | `conflict` | never |
| `OrderViolation` | 409 | `order_violation` | never |
| `InvalidCursor` | 400 | `invalid_cursor` | never |
| `CursorExpired` | 410 | `cursor_expired` | never |
| `VerificationRequired` | 422 | `verification_required` | never |
| `SignatureRejected` | 401 | `signature_rejected` | never |
| `ReplayRejected` | 409 | `replay_rejected` | never |
| `StaleTimestamp` | 400 | `stale_timestamp` | never |
| `ReplayCapacity` | 503 | `replay_capacity` | 10 |
| `Entropy` | 503 | `entropy_unavailable` | 5 |
| `CorruptStore`, `Unavailable`, `Io` | 503 | `dependency_unavailable` | 5 |
| `Gateway` | 422 | `verification_refused` | never |

Additional HTTP codes not in `WebhookError`
(`platform/hosted/webhooks/src/main.rs:189-191, 244-249, 251-253, 310-312, 334, 346, 389, 149-151, 186-187`):

| Status | Code |
| --- | --- |
| 400 | `idempotency_key_required` |
| 401 | `session_required` |
| 401 | `source_authentication_required` |
| 401 | `operator_authentication_required` |
| 404 | `not_found` |
| 503 | `encoding_failed` |

Refusal body is `{"error":{"code":"...","retry":...}}`
(`platform/hosted/webhooks/src/http.rs:73-85`).

---

## Readiness

`GET /healthz` is `200` when both `delivery` and `sources` are ready,
else `503`. JSON names components `delivery_state_and_signer` and
`canonical_sources_and_receipt_authority`
(`platform/hosted/webhooks/src/main.rs:359-371`).
`HostedService::ready` is Redis `PING`/`PONG` and KMS `/readyz` JSON
with `ready` and `ed25519_non_exportable`
(`platform/hosted/webhooks/src/hosted.rs:753-756, 341-343, 573-590`).
`TrustedSources::ready` is component and authority `/readyz` `200` and
every event source `/readyz` `200`
(`platform/hosted/webhooks/src/trusted.rs:187-203, 456-476`).
Deployment `readinessProbe` is HTTPS `/healthz` every 10s;
`livenessProbe` is TCP `9444` every 20s
(`platform/hosted/webhooks/deployment.yaml:75-76`).

Event-source `/readyz` requires upstream `/readyz` `200`, every
provisioned principal to bind, and a writable journal
(`platform/hosted/internal/src/events.rs:213-225, 246-251`).

---

## Config keys

| Key | Role | Default / manifest |
| --- | --- | --- |
| `LAYERX_WEBHOOKS_LISTEN` | Bind address | `0.0.0.0:9444` (`platform/hosted/webhooks/src/main.rs:101-104`; `platform/hosted/webhooks/deployment.yaml:6`) |
| `LAYERX_WEBHOOKS_TLS_CERT_DER` | Server cert | required (`platform/hosted/webhooks/src/main.rs:80-82`) |
| `LAYERX_WEBHOOKS_TLS_KEY_DER` | Server key | required (`platform/hosted/webhooks/src/main.rs:87-88`) |
| `LAYERX_WEBHOOKS_INTERNAL_CA_DER` | Outbound/Redis CA | required (`platform/hosted/webhooks/src/hosted.rs:691`; `platform/hosted/webhooks/src/trusted.rs:121`) |
| `LAYERX_WEBHOOKS_PUBLIC_CA_DER` | Delivery CA | required (`platform/hosted/webhooks/src/hosted.rs:700`) |
| `LAYERX_WEBHOOKS_CLIENT_IDENTITY_PKCS12` | mTLS identity | required (`platform/hosted/webhooks/src/hosted.rs:694-697`) |
| `LAYERX_WEBHOOKS_CLIENT_IDENTITY_PASSWORD_FILE` | PKCS#12 password | required (`platform/hosted/webhooks/src/hosted.rs:693`) |
| `LAYERX_WEBHOOKS_REDIS_URL` | Durable store | must be `rediss://` (`platform/hosted/webhooks/src/hosted.rs:736-737, 302-304`) |
| `LAYERX_WEBHOOKS_REDIS_USERNAME_FILE` | Redis AUTH | required (`platform/hosted/webhooks/src/hosted.rs:740`) |
| `LAYERX_WEBHOOKS_REDIS_PASSWORD_FILE` | Redis AUTH | required (`platform/hosted/webhooks/src/hosted.rs:741`) |
| `LAYERX_WEBHOOKS_KMS_URL` | Signing | required (`platform/hosted/webhooks/src/hosted.rs:723-725`) |
| `LAYERX_WEBHOOKS_KMS_TOKEN_FILE` | KMS bearer | required (`platform/hosted/webhooks/src/hosted.rs:726`) |
| `LAYERX_WEBHOOKS_IDENTITY_URL` | Session introspect | required (`platform/hosted/webhooks/src/trusted.rs:327-330`) |
| `LAYERX_WEBHOOKS_IDENTITY_TOKEN_FILE` | Identity bearer | required (`platform/hosted/webhooks/src/trusted.rs:331`) |
| `LAYERX_WEBHOOKS_COMPONENT_URL` | Receipt bytes | required (`platform/hosted/webhooks/src/trusted.rs:165-168`) |
| `LAYERX_WEBHOOKS_COMPONENT_TOKEN_FILE` | Component bearer | required (`platform/hosted/webhooks/src/trusted.rs:169`) |
| `LAYERX_WEBHOOKS_AUTHORITY_URL` | Authority facts | required (`platform/hosted/webhooks/src/trusted.rs:170-173`) |
| `LAYERX_WEBHOOKS_AUTHORITY_TOKEN_FILE` | Authority bearer | required (`platform/hosted/webhooks/src/trusted.rs:174`) |
| `LAYERX_WEBHOOKS_*_SOURCE_URL` | Four event sources | required (`platform/hosted/webhooks/src/trusted.rs:139-151`) |
| `LAYERX_WEBHOOKS_*_SOURCE_TOKEN_FILE` | Four source bearers | required (`platform/hosted/webhooks/src/trusted.rs:152`) |
| `LAYERX_WEBHOOKS_SOURCE_TRIGGER_TOKEN_FILE` | Internal publish | required (`platform/hosted/webhooks/src/trusted.rs:436`) |
| `LAYERX_WEBHOOKS_OPERATOR_TOKEN_FILE` | Internal dispatch | required (`platform/hosted/webhooks/src/trusted.rs:444`) |
| `LAYERX_WEBHOOKS_CURSOR_KEY_FILE` | 32-byte hex HMAC key | required (`platform/hosted/webhooks/src/hosted.rs:702-703`) |
| `LAYERX_WEBHOOKS_SEQUENCER_PUBLIC_KEY_FILE` | Pinned sequencer | required (`platform/hosted/webhooks/src/trusted.rs:156`) |
| `LAYERX_WEBHOOKS_NETWORK_ID` | Authority network match | required, manifest `testnet` (`platform/hosted/webhooks/src/trusted.rs:177`; `platform/hosted/webhooks/deployment.yaml:16`) |
| `LAYERX_WEBHOOKS_LXP_WIRE_VERSION` | Must be protocol `3` | required (`platform/hosted/webhooks/src/trusted.rs:157-161`) |
| `LAYERX_WEBHOOKS_INSTANCE_ID` | Lease prefix | required; Deployment uses pod UID (`platform/hosted/webhooks/src/hosted.rs:728-732`; `platform/hosted/webhooks/deployment.yaml:72-73`) |
| `LAYERX_WEBHOOKS_DISPATCH_INTERVAL_SECONDS` | Dispatch period | `1` (`platform/hosted/webhooks/src/main.rs:111-114`) |
| `LAYERX_WEBHOOKS_DISPATCH_BUDGET` | Max attempts per tick | `64` (`platform/hosted/webhooks/src/main.rs:115-116`) |
| `LAYERX_WEBHOOKS_RETENTION_EVENTS` | Prune bound | `10000`, clamped `1..=20000` (`platform/hosted/webhooks/src/main.rs:117-119`) |
| `LAYERX_WEBHOOKS_KEY_OVERLAP_SECONDS` | Rotation overlap | `86400` (`platform/hosted/webhooks/src/hosted.rs:749`) |
| `LAYERX_WEBHOOKS_BASE_DELAY_SECONDS` | Retry base | `10` (`platform/hosted/webhooks/src/hosted.rs:705`) |
| `LAYERX_WEBHOOKS_MAXIMUM_DELAY_SECONDS` | Retry cap | `3600` (`platform/hosted/webhooks/src/hosted.rs:706`) |
| `LAYERX_WEBHOOKS_MAXIMUM_ATTEMPTS` | Attempts before dead letter | `8` (`platform/hosted/webhooks/src/hosted.rs:707`) |
| `LAYERX_WEBHOOKS_SPREAD_PERCENT` | Backoff spread | `20` (`platform/hosted/webhooks/src/hosted.rs:709`) |
| `LAYERX_WEBHOOKS_SUSPEND_AFTER_DEAD_LETTERS` | Auto-suspend | `20` (`platform/hosted/webhooks/src/hosted.rs:711-714`) |
| `LAYERX_WEBHOOKS_LEASE_SECONDS` | In-flight timeout | `120` (`platform/hosted/webhooks/src/hosted.rs:716`; `platform/hosted/webhooks/deployment.yaml:20`) |

---

## Tests

In-crate tests are the destination IP guard in `boundary.rs`
(`platform/hosted/webhooks/src/boundary.rs:365-393`):

| Test | Proves |
| --- | --- |
| `public_destination_guard_rejects_translation_and_tunnel_ranges` | NAT64 `64:ff9b:`, 6to4 `2002:`, Teredo `2001:0:`, discard `0100::` are not public (`platform/hosted/webhooks/src/boundary.rs:371-385`) |
| `public_destination_guard_preserves_global_addresses` | `1.1.1.1` and `2606:4700:4700::1111` are public (`platform/hosted/webhooks/src/boundary.rs:387-393`) |

Fault-injection script against a live service
(`platform/hosted/webhooks/tests/fault-injection.sh`):

- Reads `next_cursor` from `GET .../events?limit=1` (`:21-23`)
- Publishes first source event (`:25-27`)
- Deletes running `layerx-webhooks` pods (`:29`)
- Publishes second source event (`:31-33`)
- Republishing the first event returns `duplicate == true` (`:35-38`)
- A stale (older) source event is HTTP `409` (`:40-44`)
- Receiver observations contain both events, first before second, and
  identical `body_digest` per `event_id` (`:46-65`)
- Redelivery from the saved cursor produces a second delivery of the
  first event with the same `body_digest` (`:67-91`)

Event-source crate tests
(`platform/hosted/internal/src/events.rs:363-432`):

| Test | Proves |
| --- | --- |
| `source_credentials_cannot_be_reassigned_to_a_foreign_principal` | Journey/approval match `active`+`sub`; payment/program match `principal_digest` (`:367-384`) |
| `journal_preserves_immutable_event_order_and_deduplicates_observations` | First observe sequence `1`; identical id stays `1`; new snapshot new id sequence `2`; identity mismatch refuses (`:387-432`) |

---

## Make targets

`platform/Makefile.inc` has no `platform-test-webhooks` target
(`platform/Makefile.inc:5-16, 112-159`). Webhook inputs that do appear:

| Target | Use |
| --- | --- |
| `platform-test` | workspace `cargo test`, members include `hosted/webhooks` (`platform/Makefile.inc:112-113`; `platform/Cargo.toml:16`) |
| `platform-hosted-topology-check` | `topology-check.sh`; default manifests omit webhooks (`platform/Makefile.inc:177-178`; `platform/hosted/tests/topology-check.sh:17-23, 81-88`) |
| `platform-beta-cluster-up` | renders and applies the webhooks manifest as `developer.yaml` (`platform/Makefile.inc:184-185`; `platform/hosted/tests/beta-cluster.sh:826`) |
| `platform-real-agent-integration` | requires non-empty `LAYERX_WEBHOOK_DELIVERY_PATH` (`platform/Makefile.inc:331-334`) |
| `platform-real-ios-integration` | requires non-empty `LAYERX_SAMPLE_WEBHOOK_DELIVERY_PATH` (`platform/Makefile.inc:336-338`) |
| `platform-real-android-integration` | requires non-empty `LAYERX_SAMPLE_WEBHOOK_DELIVERY_PATH` (`platform/Makefile.inc:340-342`) |

[Home](Home.md)

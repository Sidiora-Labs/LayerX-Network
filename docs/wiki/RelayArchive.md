# Relay and archive node

The relay/archive role lets an independent operator follow an existing
LayerX network, retain its complete canonical byte history, serve
public reads, and forward users' original signed activities
(`platform/relay_archive/README.md`). It does not order activities,
execute state transitions, hold a sequencer key, or participate in a
consensus or finality quorum.

`layerxd --relay-archive CONFIG` replaces the native process with the
installed Python standard-library runtime. The runtime verifies signed
bootstrap and batch material through `layerx-archive-codec`. Python
does not reimplement or weaken the native codecs.

This page is operator documentation for `platform/relay_archive`. It
is not a hosted beta service. The public hosted JSON-RPC surface
remains [Public JSON-RPC](PublicRpc.md).

---

## Trust pins

An operator must obtain these values independently of every
synchronization server and put them in the configuration:

- protocol `network_id`
- SHA-256 of the exact signed genesis manifest
- sequencer identifier and Ed25519 public key
- optionally, the first and last batch authorized for that sequencer

An upstream's `GET /v1/sync/network` document is discovery metadata,
not a trust anchor. The daemon refuses a different network, genesis
digest, sequencer identity, signature, batch number, predecessor,
committed section root, or canonical byte encoding.

---

## Install and run

The release is a flat directory containing `layerxd`,
`layerx-archive-codec`, `__init__.py`, `runtime.py`, `store.py`,
`protocol.py`, `forward.py`, `peers.py`,
`layerx-relay-archive.service`, and `manifest.sha256`.
`manifest.sha256` contains exactly one canonical `sha256sum` line for
each of the nine payload files. Obtain the manifest digest through an
independent release channel.

```sh
sudo platform/relay_archive/install.sh \
  --bundle /media/layerx-relay-archive \
  --manifest-sha256 MANIFEST_SHA256_FROM_RELEASE_CHANNEL \
  --network-id 402 \
  --genesis-sha256 PINNED_GENESIS_MANIFEST_SHA256 \
  --sequencer-id PINNED_SEQUENCER_ID \
  --sequencer-public-key PINNED_SEQUENCER_PUBLIC_KEY \
  --data-dir /var/lib/layerx/relay-archive \
  --listen 0.0.0.0:9443 \
  --public-url https://relay.example.net \
  --upstream https://archive-1.example.net \
  --upstream https://archive-2.example.net \
  --submission-upstream https://api.layerx.network/v1/activities \
  --peer-seed https://relay-seed.example.net \
  --tls-cert /etc/layerx/tls/relay.crt \
  --tls-key /etc/layerx/tls/relay.key \
  --ca-file /etc/ssl/certs/ca-certificates.crt
```

Hostnames in that example other than
`https://api.layerx.network/v1/activities` are installer
placeholders from `platform/relay_archive/README.md`. Replace them
with independently pinned origins. The installer never accepts or
copies a bearer token, API key, TLS private key, or other credential.

Rootless form:

```sh
LAYERX_RELAY_ARCHIVE_RUNTIME="$RUN_ROOT/install/runtime.py" \
  "$RUN_ROOT/install/layerxd" --relay-archive "$RUN_ROOT/relay-archive.json"
```

Local HTTP requires both a literal loopback endpoint and
`--allow-loopback-dev`. Loopback peers are never publicly advertised.

---

## Configuration

[`platform/relay_archive/config.example.json`](https://github.com/Sidiora-Labs/Paxeer-X-Network/blob/main/platform/relay_archive/config.example.json)
lists every ordinary operator setting. `REPLACE_...` fields are
invalid until independently pinned.

| Field | Role |
| --- | --- |
| `network_id`, `genesis_sha256`, `sequencer_id`, `sequencer_public_key` | Trust pins |
| `sequencer_first_batch`, `sequencer_last_batch` | Optional authorised batch range |
| `data_dir` | Durable store |
| `listen`, `public_url` | Listener and advertised origin |
| `upstreams` | Read-only synchronisation origins |
| `submission_upstreams` | The only forwarding destinations; may name an origin, `/v1/activities`, or `/rpc` |
| `allow_loopback_dev` | Required for loopback HTTP |
| `poll_interval_seconds`, `request_timeout_seconds` | Sync timing |
| `max_activity_bytes`, `max_batch_bytes`, `max_response_bytes` | Size bounds |
| `history_page_limit`, `max_history_page_limit` | History pagination (example 100 / 500) |
| `max_concurrency` | Concurrent work (example 8) |
| `peer_discovery` | Compatible-peer advertisement |
| `source_log` | Optional colocated sequencer source log |
| `source_lni_socket`, `source_submission_token_file` | Optional local LNI submission; never advertised |

URLs cannot carry credentials, query strings, or fragments. Read
discovery never mutates `submission_upstreams`.

---

## Public HTTP

Public synchronisation and archive reads do not accept credentials.
Unknown routes are refused. Numeric batch cursors are canonical
unsigned decimal. IDs and digests are 64 lowercase hexadecimal
characters.

### Synchronisation

| Method and route | Contract |
| --- | --- |
| `GET /v1/sync/network` | JSON `{version, network_id, genesis_sha256, snapshot_sha256, sequencer_id, sequencer_public_key, first_batch, last_batch}` |
| `GET /v1/sync/genesis` | Exact signed genesis-manifest octets |
| `GET /v1/sync/snapshot` | Exact canonical genesis-snapshot octets |
| `GET /v1/sync/head` | JSON `{version, network_id, genesis_sha256, head_batch, head_batch_id, head_raw_sha256, next_batch}` |
| `GET /v1/sync/batches/N` | Exact canonical bytes of batch `N` after native verification and durable commit |
| `GET /v1/peers` | Bounded, expiring compatible-peer advertisement |
| `GET /healthz` | Process liveness |
| `GET /readyz` | Ready only after pinned bootstrap and durable synchronisation state |

Raw synchronisation responses use their canonical media type and
include `Content-Length`, a quoted SHA-256 `ETag`, and
`X-Content-SHA256`. Batch responses also include `X-LayerX-Batch`.
Servers do not redirect synchronisation clients.

### History

```text
GET /v1/history/activities?cursor=&limit=&actor=&account=&module=&batch=
GET /v1/history/activities/ACTIVITY_ID
GET /v1/history/receipts?cursor=&limit=&batch=
GET /v1/history/receipts/ACTIVITY_ID
GET /v1/history/batches?cursor=&limit=
GET /v1/history/batches/BATCH_NUMBER
GET /v1/history/maintenance?cursor=&limit=&batch=
GET /v1/history/maintenance/CURSOR
```

Pages are `{version:1, items:[...], next_cursor:string|null}`. Activity
and receipt details include `canonical_hex`. A cryptographic inclusion
label means only that native verification proved inclusion in the
committed canonical batch. It does not claim that this non-executing
role replayed execution or independently established settlement
finality.

### Submission

`POST /v1/activities` accepts only bounded original signed activity
octets and an optional `Idempotency-Key`. The relay never signs,
decodes and rebuilds, or otherwise changes a user's activity.

### JSON-RPC subset

`POST /rpc` serves archive reads and forwards `lx_sendActivity`. This
is **not** the hosted gateway's 18-method contract. The relay methods
are (`platform/relay_archive/runtime.py`):

- `lx_sendActivity`
- `lx_getNodeInfo`
- `lx_getArchiveNetwork`
- `lx_getArchiveHead`
- `lx_getActivityStatus`
- `lx_getReceipt`
- `lx_getBatchHeader`
- `lx_listActivities`
- `lx_listBatches`

Incoming `Authorization` or `LayerX-Key` is forwarded only to
configured submission endpoints. No credential is sent to a discovered
read peer.

---

## Container deployment

```sh
docker build -f platform/relay_archive/Dockerfile \
  --build-arg LXP_REVISION="$(git rev-parse HEAD)" \
  -t layerx-relay-archive:local .
```

Mount a read-only configuration at `/etc/layerx/relay-archive.json`,
durable storage at `/var/lib/layerx/relay-archive`, and TLS material
at the paths named by the configuration.
[`deployment.example.yaml`](https://github.com/Sidiora-Labs/Paxeer-X-Network/blob/main/platform/relay_archive/deployment.example.yaml)
is a non-root Kubernetes example whose placeholders fail closed until
replaced.

[Home](Home.md)

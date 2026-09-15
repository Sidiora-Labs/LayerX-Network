# LayerX relay/archive node

The relay/archive role lets an independent operator follow an existing LayerX
network, retain its complete canonical byte history, serve public reads, and
forward users' original signed activities. It does not order activities,
execute state transitions, hold a sequencer key, or participate in a consensus
or finality quorum.

`layerxd --relay-archive CONFIG` replaces the native process with the installed
Python standard-library runtime. The runtime verifies signed bootstrap and
batch material through `layerx-archive-codec`; Python does not reimplement or
weaken the native codecs.

## Trust and network model

An operator must obtain these values independently of every synchronization
server and put them in the configuration:

- protocol `network_id`;
- SHA-256 of the exact signed genesis manifest;
- sequencer identifier and Ed25519 public key;
- optionally, the first and last batch authorized for that sequencer.

An upstream's `/v1/sync/network` document is discovery metadata, not a trust
anchor. The daemon refuses a different network, genesis digest, sequencer
identity, signature, batch number, predecessor, committed section root, or
canonical byte encoding. It atomically stores the raw genesis, snapshot,
batches, activities, receipts, and maintenance records before advancing its
head. Restart resumes at the durable next-batch position without accepting a
gap or conflicting bytes.

Peer discovery extends read synchronization only. A compatible `/v1/peers`
document must carry the same four trust pins, a bounded expiration, and a
bounded list of credential-free HTTPS origins. Discovered peers are first
probed and pin-checked; they are never added to submission failover. DNS is
resolved and checked again immediately before use, and connections are made to
the checked address while TLS authenticates the original hostname. Redirects,
proxy environment variables, link-local, private, reserved, and loopback
advertisements are refused. Literal loopback HTTP is available only for an
operator-configured development seed when `allow_loopback_dev` is explicitly
true, and loopback peers are never publicly advertised.

## Operator quickstart

The release is a flat directory containing:

```text
layerxd
layerx-archive-codec
__init__.py
runtime.py
store.py
protocol.py
forward.py
peers.py
layerx-relay-archive.service
manifest.sha256
```

`manifest.sha256` contains exactly one canonical `sha256sum` line for each of
the nine payload files. Obtain the manifest digest through
an independent release channel. Computing a digest from the same untrusted
download and passing it back to the installer is trust on first use and is not
a valid installation.

Install a locally transferred bundle and generate a new service configuration:

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
  --submission-upstream https://api.testnet.layerx.network/v1/activities \
  --peer-seed https://relay-seed.example.net \
  --tls-cert /etc/layerx/tls/relay.crt \
  --tls-key /etc/layerx/tls/relay.key \
  --ca-file /etc/ssl/certs/ca-certificates.crt
```

For a published tar archive, pin both the archive and its internal manifest:

```sh
sudo platform/relay_archive/install.sh \
  --release https://releases.example.net/layerx-relay-archive-VERSION.tar.gz \
  --expected-sha256 PINNED_ARCHIVE_SHA256 \
  --manifest-sha256 PINNED_MANIFEST_SHA256 \
  --network-id 402 \
  --genesis-sha256 PINNED_GENESIS_MANIFEST_SHA256 \
  --sequencer-id PINNED_SEQUENCER_ID \
  --sequencer-public-key PINNED_SEQUENCER_PUBLIC_KEY \
  --data-dir /var/lib/layerx/relay-archive \
  --listen 0.0.0.0:9443 \
  --public-url https://relay.example.net \
  --upstream https://archive-1.example.net \
  --tls-cert /etc/layerx/tls/relay.crt \
  --tls-key /etc/layerx/tls/relay.key
```

The remote archive URL must be credential-free HTTPS. The installer permits no
undeclared archive entry, link, path traversal, or checksum mismatch. Releases
are installed into an immutable manifest-digest directory and stable software
symlinks are changed atomically. An existing regular configuration and data
directory are never deleted or replaced. If `--config FILE` already exists,
the installer preserves it byte-for-byte and refuses all config-generation
arguments. It never accepts or copies a bearer token, API key, TLS private key,
or other credential; configuration references operator-managed files.

The service install creates the unprivileged `layerx-relay-archive` account and
enables `layerx-relay-archive.service`. Non-loopback listeners require a TLS
certificate and key. Ensure that the service account can read operator-managed
TLS files without making them world-readable.

For a rootless installation or the real-process qualification path:

```sh
platform/relay_archive/install.sh \
  --bundle "$BUNDLE" \
  --manifest-sha256 "$PINNED_MANIFEST_SHA256" \
  --prefix "$RUN_ROOT/install" \
  --no-service \
  --config "$RUN_ROOT/relay-archive.json"

LAYERX_RELAY_ARCHIVE_RUNTIME="$RUN_ROOT/install/runtime.py" \
  "$RUN_ROOT/install/layerxd" --relay-archive "$RUN_ROOT/relay-archive.json"
```

That form expects the named configuration to exist already. To have the
rootless installer create it, add the same explicit pins, paths, listener,
public URL, and upstream flags used by the service example. Local HTTP requires
both a literal `127.0.0.1` or `::1` endpoint and `--allow-loopback-dev`.

## Configuration

[`config.example.json`](config.example.json) lists every ordinary operator
setting. Its `REPLACE_...` fields are deliberately invalid until independently
pinned. `upstreams` are read-only synchronization origins.
`submission_upstreams` are the only forwarding destinations and may name an
origin, `/v1/activities`, or `/rpc`; URLs cannot carry credentials, query
strings, or fragments. Read discovery never mutates that list.

An origin colocated with the sequencer may additionally configure
`source_log`. To accept activity submissions over the real local LNI it must
configure both `source_lni_socket` and `source_submission_token_file`. Those
paths are operator configuration, are never advertised, and are not accepted
by the installer as credential-copy inputs.

## Canonical synchronization contract

All numeric batch cursors in paths are canonical unsigned decimal. IDs and
digests are 64 lowercase hexadecimal characters. Unknown routes are refused.
Public synchronization and archive reads do not accept credentials.

| Method and route | Contract |
| --- | --- |
| `GET /v1/sync/network` | JSON `{version, network_id, genesis_sha256, snapshot_sha256, sequencer_id, sequencer_public_key, first_batch, last_batch}`. Consumers compare the pins before downloading history. |
| `GET /v1/sync/genesis` | Exact signed genesis-manifest octets. |
| `GET /v1/sync/snapshot` | Exact canonical genesis-snapshot octets. |
| `GET /v1/sync/head` | JSON `{version, network_id, genesis_sha256, head_batch, head_batch_id, head_raw_sha256, next_batch}` from the durable head. |
| `GET /v1/sync/batches/N` | Exact canonical bytes of batch `N`, only after native verification and durable commit. |
| `GET /v1/peers` | A bounded, expiring compatible-peer advertisement described below. |
| `GET /healthz` | Process liveness only. |
| `GET /readyz` | Ready only after pinned bootstrap and durable synchronization state are available. |

Raw synchronization responses use their canonical media type and include
`Content-Length`, a quoted SHA-256 `ETag`, and `X-Content-SHA256`. Batch responses
also include `X-LayerX-Batch`. Servers do not redirect synchronization clients.

Complete public history is cursor-paginated under:

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

Pages are `{version:1, items:[...], next_cursor:string|null}`. Activity and
receipt details include `canonical_hex`; maintenance details include
`receipt_hex`; exact batch bytes remain available at the synchronization route.
The canonical bytes remain the authority. A cryptographic inclusion label means
only that native verification proved inclusion in the committed canonical
batch. It does not claim that this non-executing role replayed execution or
independently established settlement finality.

`POST /v1/activities` accepts only bounded original signed activity octets and
an optional `Idempotency-Key`. `POST /rpc` serves archive read methods and
forwards `lx_sendActivity`. The relay never signs, decodes and rebuilds, or
otherwise changes a user's activity. It durably binds idempotency to the exact
bytes, retries the same bytes only after transport or availability failure,
preserves a definitive upstream refusal, and reports an unresolved transport
outcome as unknown. Incoming `Authorization` or `LayerX-Key` is forwarded only
to configured submission endpoints; no credential is sent to a discovered
read peer.

The peer document has exactly this versioned shape:

```json
{
  "version": 1,
  "network_id": 402,
  "genesis_sha256": "64 lowercase hex",
  "sequencer_id": "64 lowercase hex",
  "sequencer_public_key": "64 lowercase hex",
  "generated_at": 0,
  "expires_at": 0,
  "peers": [
    {"url": "https://relay.example.net", "expires_at": 0}
  ]
}
```

## Container deployment

Build the image from the repository root so both native executables and the
Python runtime come from the same source revision:

```sh
docker build -f platform/relay_archive/Dockerfile \
  --build-arg LXP_REVISION="$(git rev-parse HEAD)" \
  -t layerx-relay-archive:local .
```

Mount a read-only configuration at `/etc/layerx/relay-archive.json`, mount
durable storage at `/var/lib/layerx/relay-archive`, and mount TLS material at
the paths named by the configuration. [`deployment.example.yaml`](deployment.example.yaml)
provides a non-root Kubernetes Deployment, persistent volume, HTTPS Service and
Ingress, and an egress policy that excludes common private and link-local
networks. Replace every pin, hostname, certificate Secret, and image digest
before applying it; the checked-in placeholders intentionally fail closed.

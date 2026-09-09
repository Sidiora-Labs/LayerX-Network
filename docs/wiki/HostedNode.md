# Hosted node

The hosted node is the in-cluster sequencer pod. Image
`ghcr.io/sidiora-labs/layerx-node:0.1.0` builds `layerxd`,
`layerx-genesis-build`, and `layerxctl`, copies
`bootstrap.sh` and `supervisor.sh`, and sets `USER 4020:4020`
with `ENTRYPOINT` `/opt/layerx/supervisor.sh`
(`platform/hosted/node/Dockerfile:10-11`;
`platform/hosted/node/Dockerfile:20-27`). The binary
`layerxd` is `cmd/layerxd/`; its CLI is `--check-config`,
`--serve`, or `--authority-replica` plus a configuration path
(`cmd/layerxd/lxp_daemon_cli.c:8-16`). Any other argv is
`LXP_ERR_NON_CANONICAL`.

StatefulSet `layerx-node` in `layerx-testnet` has one replica
and headless Service `layerx-node` on port `9443`
(`platform/hosted/node/deployment.yaml:12-21`). Pod labels are
`app: layerx-node` and `layerx-plane: trusted-boundary`
(`platform/hosted/node/deployment.yaml:24`). There is no Ingress
object in that manifest.

This page covers bootstrap, the supervisor, `layerxd`
listeners, and the node StatefulSet. The colocated TLS
boundaries are [Hosted core](HostedCore.md),
[Hosted authority](HostedAuthority.md), and
[Hosted agent boundary](HostedAgentBoundary.md). The loopback
Paxeer relay is documented with [Paxeer boundary](PaxeerBoundary.md).

---

## Cluster shape

Containers in the pod (`platform/hosted/node/deployment.yaml:28-223`):

| Container | Image | Role |
| --- | --- | --- |
| `layerxd` | `layerx-node:0.1.0` | Sequencer supervisor: bootstrap, `layerxd --serve`, supervisor Unix socket |
| `layerxd-authority` | `layerx-node:0.1.0` | Replica supervisor: `layerxd --authority-replica` |
| `paxeer-relay` | `layerx-node:0.1.0` | `socat` TCP `127.0.0.1:18545` to `OPENSSL:paxeer-boundary.layerx-testnet.svc.cluster.local:9443` (`platform/hosted/node/deployment.yaml:78-90`) |
| `core-boundary` | `layerx-core-boundary:0.1.0` | TLS core/admin planes; see [Hosted core](HostedCore.md) |
| `receipt-authority` | `layerx-receipt-authority:0.1.0` | TLS receipt authority; see [Hosted authority](HostedAuthority.md) |
| `agent-boundary` | `layerx-agent-boundary:0.1.0` | TLS agent boundary; see [Hosted agent boundary](HostedAgentBoundary.md) |

Services (`platform/hosted/node/deployment.yaml:12-14`;
`platform/hosted/node/deployment.yaml:246-264`):

| Service | Type | Port | Target |
| --- | --- | --- | --- |
| `layerx-node` | headless | `9443` | `core-tls` |
| `layerx-pending-core` | ClusterIP | `9443` | `core-tls` (`9443`) |
| `layerx-pending-core-admin` | ClusterIP | `9444` | `core-admin-tls` (`9444`) |
| `layerx-receipt-authority` | ClusterIP | `9443` | `authority-tls` (`9445`) |
| `layerx-agent-boundary` | ClusterIP | `9443` | `agent-tls` (`9446`) |

Loopback listeners inside the pod are not Services:
program HTTP `127.0.0.1:9401`, replica HTTP `127.0.0.1:9402`,
LNI Unix socket `/run/layerx/node/layerxd.lni.sock`, supervisor
Unix socket `/run/layerx/node/supervisor.sock`
(`platform/hosted/node/deployment.yaml:55-58`;
`platform/hosted/node/deployment.yaml:130-131`;
`platform/hosted/node/bootstrap.sh:358-359`).

`platform-hosted-topology-check` loads
`platform/hosted/node/deployment.yaml` among its default
manifests (`platform/hosted/tests/topology-check.sh:18`;
`platform/hosted/tests/topology-check.sh:84`). Beta-cluster
apply of `node.yaml` must create the trusted-boundary Services
including `layerx-pending-core`,
`layerx-pending-core-admin`, `layerx-receipt-authority`, and
`layerx-agent-boundary`
(`platform/hosted/tests/beta-cluster.sh:91`;
`platform/hosted/tests/beta-cluster.sh:862-864`). Qualification
binds `LAYERX_QUALIFICATION_NODE_URL` to Service
`layerx-pending-core` and `LAYERX_QUALIFICATION_AGENT_URL` to
Service `layerx-agent-boundary`
(`platform/hosted/tests/beta-cluster.sh:1134-1137`).

---

## Bootstrap artefacts

`bootstrap.sh` writes a signed genesis and the files the
supervisor sources (`platform/hosted/node/bootstrap.sh:4-8`;
`platform/hosted/node/bootstrap.sh:73-78`). Required flags:
`--data-dir`, `--run-dir`, `--network-id`, `--sequencer-key`,
`--treasury-key` (`platform/hosted/node/bootstrap.sh:14-26`;
`platform/hosted/node/bootstrap.sh:191-195`). `--data-dir` and
`--run-dir` must differ, and the run directory must not sit
inside the data directory
(`platform/hosted/node/bootstrap.sh:347-348`). The data
directory is created mode `0700` and must be empty unless
`--force` (`platform/hosted/node/bootstrap.sh:343`;
`platform/hosted/node/bootstrap.sh:352-354`). The run directory
is mode `0750` with group `--lni-gid`
(`platform/hosted/node/bootstrap.sh:355-357`).

Outputs under the data directory
(`platform/hosted/node/bootstrap.sh:73-78`):

| Path | Contents |
| --- | --- |
| `genesis/genesis.manifest` | Signed genesis manifest from `layerx-genesis-build` |
| `genesis/00000000000000000000.lxs` | Genesis snapshot |
| `genesis/paxeer-registration-request.lxrr` | LXRR v1, length `73` (`platform/hosted/node/bootstrap.sh:416`) |
| `genesis/paxeer-deployment-descriptor.lxgd` | LXGD descriptor |
| `genesis/genesis.registration` | LXGR v1 self-registration, length `82`, written only when `--custody-profile` is absent (`platform/hosted/node/bootstrap.sh:420-434`) |
| `identities.txt` | One line `{treasury-did-hex}:{treasury-public-key}:0` (`platform/hosted/node/bootstrap.sh:438`) |
| `checkpoints/` | Empty checkpoint directory |
| `logs/` | `program-feed.log`, `canonical.log`, `receipt-authority.log`, `batch.log`, `evidence.log` |
| `replica/receipt-authority.log` | Replica log |
| `secrets/program-token` | Program-listener bearer |
| `secrets/replica-token` | Replica-listener bearer |
| `secrets/treasury-key.hex` | Treasury seed hex |
| `secrets/guarantor-key.pem` | secp256k1 guarantor private key |
| `sequencer.conf` / `replica.conf` | Eight-line `layerxd` configurations; both pass `layerxd --check-config` (`platform/hosted/node/bootstrap.sh:449-455`) |
| `sequencer.env` / `replica.env` | Daemon environments |
| `node.env` | Published node facts, mode `0644` |
| `treasury.json` | Treasury DID, public key, account, asset, genesis balance, network id (`platform/hosted/node/bootstrap.sh:543-546`) |

The run directory receives `core.env` with
`LAYERX_CORE_SEQUENCER_ID` and `LAYERX_CORE_TREASURY_ASSET`
(`platform/hosted/node/bootstrap.sh:538-541`). Cluster bring-up
waits until `node.env`, the LXGD descriptor, and the LXRR
request are present, then fetches those three files
(`platform/hosted/tests/beta-cluster.sh:885-902`).

A non-zero `--treasury-balance` is refused:
`treasury_balance_unsupported`. The genesis manifest admits
only the three system accounts at balance zero
(`platform/hosted/node/bootstrap.sh:217-219`). Default balance
is `0`. `--asset` must be 64 lowercase hex and not zero;
default is
`b5a32b12029f8ddfb905f90f280f664b46390de0fc62770fc197dd87b18cd898`,
`sha256("layerx-beta-asset:LXT")`
(`platform/hosted/node/bootstrap.sh:29-30`;
`platform/hosted/node/bootstrap.sh:146`;
`platform/hosted/node/bootstrap.sh:214-216`). ConfigMap
`layerx-node-config` pins that same asset id, network id
`402`, network name `layerx-testnet`, replica id
`6c61796572782d626574612d726563656970742d617574686f726974792d3031`,
and Paxeer relay port `18545`
(`platform/hosted/node/deployment.yaml:2-9`).

`--custody-profile` must name a readable regular file that is
not a symlink and is exactly 207 bytes
(`platform/hosted/node/bootstrap.sh:196-201`). When set,
`layerx-genesis-build` receives `--custody-profile` and
bootstrap does not write `genesis.registration`
(`platform/hosted/node/bootstrap.sh:405-407`;
`platform/hosted/node/bootstrap.sh:423-434`).

---

## Genesis request and guarantor keys

Bootstrap generates a secp256k1 key at
`secrets/guarantor-key.pem`, derives the compressed public key
(33 bytes, prefix `02` or `03`), and sets
`GUARANTOR_ID = sha256("layerx-beta-guarantor:" || public-key-hex)`
(`platform/hosted/node/bootstrap.sh:364-368`). Cluster bring-up
requires `LAYERX_NODE_GENESIS_GUARANTOR_ID` as 64 hex and
`LAYERX_NODE_GENESIS_GUARANTOR_PUBLIC_KEY` as that compressed
form (`platform/hosted/tests/beta-cluster.sh:903-908`). The
same public key is the Paxeer guarantor signer input
(`platform/hosted/tests/beta-cluster.sh:917-923`).

The genesis request is LXGB v2. Its existing fixed body is 395 bytes;
`--genesis-metadata FILE` supplies the required canonical metadata suffix:

- magic `LXGB`, version `2`, protocol `3`
- network id, genesis timestamp milliseconds
- one parameter `parameter-version` = `1`
- one guarantor (id, compressed public key, bond `0`)
- the genesis asset, fee coefficients, and demand coefficients
- Asset record count u16, then each record's u16 byte length and canonical
  version-3 bytes, followed by the named fee schedule's u16 byte length and
  canonical version-2 bytes (all integers big-endian)

Supply the authoritative Asset records, including issuer DID id32, original
salt, cap, pause and supply. The fresh empty genesis builder requires zero
circulating supply and custody issuer kind; it validates the records and
schedule before signing. Do not invent salts for an existing asset. The
metadata file must be kept outside a data directory discarded with `--force`.
The same suffix is required by `prepare-beta.py --genesis-metadata FILE` and
`tests/bridge/custody_genesis.py --genesis-metadata FILE`. Version-1 decoding
remains available for existing signed artifacts; it cannot restore omitted
metadata.

`layerx-genesis-build` signs that request with the sequencer
seed (`platform/hosted/node/bootstrap.sh:400-408`). LXRR bytes
`[9,41)` are `LAYERX_NODE_GENESIS_STATE_ROOT`; the last 32
bytes are `LAYERX_NODE_GENESIS_RECEIPT_STATE_ROOT`
(`platform/hosted/node/bootstrap.sh:417-418`). Without a
custody profile, LXGR anchors both registration roots to the
receipt state root (`platform/hosted/node/bootstrap.sh:420-432`).

To migrate a post-genesis checkpoint containing the retired
`asset:<asset-id>:issuance` account name, stop the node and run:

```sh
layerx-genesis-build --migrate-snapshot-issuance \
  SOURCE.lxs genesis/genesis.manifest SEQUENCER.key MIGRATED-CHECKPOINTS
```

`MIGRATED-CHECKPOINTS` must not exist. The command verifies the signed genesis
manifest and requires its Ed25519 signer key, verifies the source snapshot and
its legacy state root, renames only retired issuance accounts to the
`module:asset:value:<account-id>` form while preserving their account ids and
all other state, and writes a same-sequence LXS3
checkpoint. Its authorization binds both snapshot digests, both canonical
roots, the prior receipt root, the newly derived receipt root, network id and
rename count. On restore, `layerxd` accepts LXS3 only when that authorization
verifies against the signer in `genesis.manifest`. Preserve and deploy the
matching `.lxi` identity sidecar beside the migrated `.lxs`; the migration
command does not rewrite identity history.

Sequencer and treasury seeds are 32 raw bytes or 64 hex
characters and must yield distinct public keys
(`platform/hosted/node/bootstrap.sh:283-315`). Sequencer id is
`sha256("layerx-sequencer:" || public-key-hex)`
(`platform/hosted/node/bootstrap.sh:316`). Replica id defaults
to `sha256("layerx-authority-replica:" || public-key-hex)`
(`platform/hosted/node/bootstrap.sh:317-321`). The hosted
manifest overrides replica id from ConfigMap
(`platform/hosted/node/deployment.yaml:45-46`;
`platform/hosted/node/deployment.yaml:68`).

---

## Settlement environment

Five keys bind Paxeer settlement. They are either in the
bootstrap process environment or deferred to
`--settlement-env FILE`
(`platform/hosted/node/bootstrap.sh:55-64`;
`platform/hosted/node/bootstrap.sh:221-232`). The file form
excludes those five from the environment
(`platform/hosted/node/bootstrap.sh:228-229`). Path must be
absolute (`platform/hosted/node/bootstrap.sh:227`).

`bootstrap.sh --check-settlement FILE` is the validator
(`platform/hosted/node/bootstrap.sh:67-72`;
`platform/hosted/node/bootstrap.sh:135-138`). The file must be
a readable regular file of at most 4096 bytes, holding exactly
these `KEY=VALUE` lines with no repeats and no other keys
(`platform/hosted/node/bootstrap.sh:110-132`):

| Key | Rule |
| --- | --- |
| `LAYERX_NODE_PAXEER_CHAIN_ID` | Positive decimal `uint64` (`platform/hosted/node/bootstrap.sh:94-97`) |
| `LAYERX_NODE_SETTLEMENT_CONTRACT` | `0x` plus 40 hex, not zero (`platform/hosted/node/bootstrap.sh:98-105`) |
| `LAYERX_NODE_CHECKPOINT_REGISTRY` | same address rule |
| `LAYERX_NODE_PAXEER_RPC_ADDRESS` | exactly `127.0.0.1` (`platform/hosted/node/bootstrap.sh:106`) |
| `LAYERX_NODE_PAXEER_RPC_PORT` | `1..=65535` (`platform/hosted/node/bootstrap.sh:107`) |

The hosted sequencer args pass `--settlement-env`
`/run/layerx/settlement/settlement.env`
(`platform/hosted/node/deployment.yaml:63-64`). That path is
ConfigMap `layerx-node-settlement`, optional at apply
(`platform/hosted/node/deployment.yaml:229`). Cluster bring-up
writes the five lines after contract deploy, runs
`--check-settlement`, then applies that ConfigMap
(`platform/hosted/tests/beta-cluster.sh:953-957`).

When `sequencer.env` names `LAYERX_NODE_SETTLEMENT_ENV`, the
sequencer supervisor waits for the file (log every 30 seconds,
cap `LAYERX_NODE_SETTLEMENT_WAIT_SECONDS`, default `3600`),
validates it with `--check-settlement`, and exports the five
values before `layerxd --serve`
(`platform/hosted/node/supervisor.sh:30-34`;
`platform/hosted/node/supervisor.sh:161-181`;
`platform/hosted/node/supervisor.sh:188-198`).

---

## `node.env` keys

`node.env` is mode `0644`
(`platform/hosted/node/bootstrap.sh:509-537`):

| Key | Role |
| --- | --- |
| `LAYERX_NODE_NETWORK_ID` | Decimal network id |
| `LAYERX_NODE_ASSET_ID` | Genesis asset hex |
| `LAYERX_NODE_LNI_SOCKET` | Absolute LNI Unix path |
| `LAYERX_NODE_SUPERVISOR_SOCKET` | Absolute supervisor Unix path |
| `LAYERX_NODE_PROGRAM_URL` | `http://127.0.0.1:{program-port}` |
| `LAYERX_NODE_REPLICA_URL` | `http://127.0.0.1:{replica-port}` |
| `LAYERX_NODE_PROGRAM_BEARER_TOKEN_FILE` | `{data}/secrets/program-token` |
| `LAYERX_NODE_REPLICA_BEARER_TOKEN_FILE` | `{data}/secrets/replica-token` |
| `LAYERX_NODE_SEQUENCER_ID` | Sequencer id hex |
| `LAYERX_NODE_SEQUENCER_PUBLIC_KEY` | Sequencer public key hex |
| `LAYERX_NODE_REPLICA_ID` | Replica id hex |
| `LAYERX_NODE_GENESIS_STATE_ROOT` | From LXRR |
| `LAYERX_NODE_GENESIS_RECEIPT_STATE_ROOT` | From LXRR |
| `LAYERX_NODE_GENESIS_GUARANTOR_ID` | Guarantor id hex |
| `LAYERX_NODE_GENESIS_GUARANTOR_PUBLIC_KEY` | Compressed guarantor public key hex |
| `LAYERX_NODE_GENESIS_GUARANTOR_KEY_FILE` | `{data}/secrets/guarantor-key.pem` |
| `LAYERX_PAXEER_GENESIS_DIR` | `{data}/genesis` |
| `LAYERX_NODE_TREASURY_DID` | `did:layerx:{treasury-public-key}` |
| `LAYERX_NODE_TREASURY_PUBLIC_KEY` | Treasury public key hex |
| `LAYERX_NODE_TREASURY_ACCOUNT` | `agent:{did}:main` |
| `LAYERX_NODE_TREASURY_BALANCE` | Genesis treasury balance (`0`) |
| `LAYERX_NODE_TREASURY_KEY_FILE` | `{data}/secrets/treasury-key.hex` |
| `LAYERX_NODE_SEQUENCER_CONFIG` | `{data}/sequencer.conf` |
| `LAYERX_NODE_REPLICA_CONFIG` | `{data}/replica.conf` |
| `LAYERX_NODE_SEQUENCER_ENV` | `{data}/sequencer.env` |
| `LAYERX_NODE_REPLICA_ENV` | `{data}/replica.env` |

`sequencer.env` additionally carries snapshot, manifest,
registration, identities, logs, history database, sequencer
keys, batch range `1`..`18446744073709551615`, replica
address/port/id/token, program address/port/token, LNI socket,
allowed uid/gid, `LAYERX_NODE_LNI_FRAME_BYTES=1212416`, and
`LAYERX_NODE_LNI_DEADLINE_MS=2000`, or
`LAYERX_NODE_SETTLEMENT_ENV` in place of the five settlement
keys (`platform/hosted/node/bootstrap.sh:457-494`).
`replica.env` carries replica log, replica id, sequencer id
and public key, batch range, bearer token, and
`LAYERX_AUTHORITY_ADDRESS=127.0.0.1` plus port
(`platform/hosted/node/bootstrap.sh:496-506`).

`layerxd --serve` requires
`LAYERX_NODE_LNI_FRAME_BYTES` equal to
`LXP_DAEMON_LNI_MAX_FRAME_BYTES`
(`cmd/layerxd/lxp_daemon_process.c:3804-3810`). Bootstrap
writes `1212416` (`platform/hosted/node/bootstrap.sh:492`).
`cmd/layerxd/lni.env.example:23` writes `1146902`. Those two
values differ.

---

## LNI socket

The production LNI is the sole activity-ingress boundary
(`cmd/layerxd/lni.env.example:1-5`). Path is
`RUN_DIR/layerxd.lni.sock`, length less than 108
(`platform/hosted/node/bootstrap.sh:358-360`;
`include/layerx/lxp_daemon.h:36`). The parent is mode `0750`,
owned by the daemon uid with the configured client gid. The
socket is mode `0660`, owned by `layerxd` and that gid
(`cmd/layerxd/lxp_daemon_process.c:3790`;
`cmd/layerxd/lxp_daemon_lni.c:3321-3324`;
`cmd/layerxd/lni.env.example:2-5`). `SO_PEERCRED` requires the
exact configured uid and gid; a mismatch is `LXP_ERR_AUTH_SCOPE`
(`cmd/layerxd/lxp_daemon_lni.c:2975-2991`). The client uid must
differ from the daemon uid
(`platform/hosted/node/bootstrap.sh:238`;
`cmd/layerxd/lni.env.example:6`). Hosted values: daemon
`4020:4020`, `--lni-uid 4021`, `--lni-gid 4020`
(`platform/hosted/node/deployment.yaml:26`;
`platform/hosted/node/deployment.yaml:59-62`). Colocated
`core-boundary`, `receipt-authority`, and `agent-boundary`
run as `4021:4020` so the LNI admits them
(`platform/hosted/node/deployment.yaml:119`;
`platform/hosted/node/deployment.yaml:155`;
`platform/hosted/node/deployment.yaml:196`).

Interface is LNI 1.4 (`cmd/layerxd/lxp_daemon_lni.c:38-39`).
The first frame must be NodeInfo request tag `1` with
correlation `0` and empty payload; otherwise refusal class `2`
`LXP_ERR_AUTH_SCOPE` (`cmd/layerxd/lxp_daemon_lni.c:40-41`;
`cmd/layerxd/lxp_daemon_lni.c:3126-3132`). Later tags
(`cmd/layerxd/lxp_daemon_lni.c:40-60`;
`cmd/layerxd/lxp_daemon_lni.c:3138-3167`):

| Tag | Name |
| --- | --- |
| `1` / `2` | NodeInfo |
| `3` / `4` | Submit |
| `5` / `6` | ReceiptLookup |
| `7` / `8` | AccountRead |
| `12` / `13` | BatchHeader |
| `14` / `15` | Checkpoint |
| `16` / `17` | ProofBundle |
| `25` | Error |
| `26` / `27` | PreparationState |
| `28` / `29` | FinalityEvidenceRegister |
| `30` / `31` | Simulate |

An unknown tag is refusal class `3` `LXP_ERR_MODULE_DISABLED`.
A second NodeInfo is class `1` `LXP_ERR_NON_CANONICAL`.
Sequencer NodeInfo advertises at least
`authenticated_durable_submit`, `batch_header`, `node_info`,
`preparation_state`, `receipt_lookup`, `submit`; evidence,
finalizer, and `simulate` bits depend on the daemon
(`cmd/layerxd/lxp_daemon_lni.c:1311-1334`). [Agentd](Agentd.md)
consumes receipts and batch evidence from this plane; it does
not mint balances.

---

## Supervisor socket

The sequencer supervisor listens on
`RUN_DIR/supervisor.sock` via `socat` `UNIX-LISTEN`, mode
`660`, group `LAYERX_NODE_LNI_ALLOWED_GID`
(`platform/hosted/node/supervisor.sh:358-360`). One line per
connection, 10 second read
(`platform/hosted/node/supervisor.sh:77-78`):

| Request | Success | Refusal |
| --- | --- | --- |
| `reset` | `{"state":"reset","reset_id":"<16 hex>"}` after stop, discard, bootstrap `--force`, restart (`platform/hosted/node/supervisor.sh:80-102`; `platform/hosted/node/supervisor.sh:363-387`) | `supervisor_unavailable` retry 5; `reset_failed` retry 30; `reset_timeout` retry 60 |
| `status` | `{"state":"running","generation":N}` or `{"state":"stopped","generation":N}` (`platform/hosted/node/supervisor.sh:104-111`) | none |
| any other line | | `unknown_request` retry never (`platform/hosted/node/supervisor.sh:113-114`) |

Reset coordinates the replica through
`reset.<id>.stop-replica` / `reset.<id>.replica-stopped`
(`platform/hosted/node/supervisor.sh:24-28`;
`platform/hosted/node/supervisor.sh:367-373`). A daemon that
exits on its own ends the supervisor with status `1`
(`platform/hosted/node/supervisor.sh:36-37`).

---

## Program listener

`layerxd --serve` binds the program listener only on
`127.0.0.1` (`cmd/layerxd/lxp_daemon_listener.c:397-408`).
Hosted port `9401`. `Authorization: Bearer` must match
`LAYERX_NODE_PROGRAM_BEARER_TOKEN` in length and constant-time
compare (`cmd/layerxd/lxp_daemon_protocol.c:134-139`;
`cmd/layerxd/lxp_daemon_protocol.c:888-890`). Duplicate
`Authorization` or a non-`HTTP/1.1` request line is
`LXP_ERR_NON_CANONICAL` (`cmd/layerxd/lxp_daemon_listener.c:154-166`).
GET only (`cmd/layerxd/lxp_daemon_protocol.c:496`).

| Path | Success body |
| --- | --- |
| `GET /v1/protocol/account-state/head` | `current`, `receipt_hex`, `receipt_digest`, `state_root`, `observed_sequence`, `observed_at`, `batch_evidence` (`cmd/layerxd/lxp_daemon_protocol.c:267-282`; `cmd/layerxd/lxp_daemon_protocol.c:300-306`) |
| `GET /v1/receipts/{64 hex}/account-state` | same document for that receipt digest (`cmd/layerxd/lxp_daemon_protocol.c:499-510`) |
| `GET /v1/programs/account-state/changes?after_sequence={decimal}` | `records[]` of `sequence`, `ordinal`, `program_id`, `activity_type`, `event_type`, `receipt_digest`; `complete_through`; `scanned_through_sequence`; `caught_up` (`cmd/layerxd/lxp_daemon_protocol.c:366-389`; `cmd/layerxd/lxp_daemon_protocol.c:546-551`) |
| `GET /v1/programs/{64 hex}/account-state?at={decimal}` | `program_id`, `record_hex`, `record_digest`, `receipt_digest`; `at` must equal the feed head (`cmd/layerxd/lxp_daemon_protocol.c:318-342`; `cmd/layerxd/lxp_daemon_protocol.c:553-564`) |
| `GET /v1/programs/activities/{64 hex}/artifacts?receipt_digest={64 hex}` | `activity_id`, `receipt_digest`, `terminal_payload`, `call_graph` (`cmd/layerxd/lxp_daemon_protocol.c:419-427`; `cmd/layerxd/lxp_daemon_protocol.c:530-544`) |
| `GET /v1/programs/receipts/by-idempotency/{64 lowercase hex}` | `activity_id`, `receipt` (`cmd/layerxd/lxp_daemon_protocol.c:462-466`; `cmd/layerxd/lxp_daemon_protocol.c:514-528`) |
| `GET /v1/batches/{64 hex}/receipt-authority?receipt_digest={64 hex}` | `sequencer_public_key`, `batch_evidence` (`cmd/layerxd/lxp_daemon_protocol.c:481-485`; `cmd/layerxd/lxp_daemon_protocol.c:567-580`) |

HTTP mapping after a valid parse
(`cmd/layerxd/lxp_daemon_protocol.c:888-901`;
`cmd/layerxd/lxp_daemon_listener.c:214-251`):

| Status | Body | Condition |
| --- | --- | --- |
| `200` | route JSON | `LXP_OK` |
| `401` | `{"error":<LXP_ERR_BAD_SIGNATURE>}` | bearer mismatch |
| `404` | `{"error":<LXP_ERR_UNKNOWN_ACTIVITY>}` | unknown path or non-GET |
| `503` | `{"error":<code>}` | any other `lxp_result` |
| `400` | `{"error":"refused"}` | framing failure before `protocol_route` |

Those two error envelopes differ: routed failures are a
numeric `error`; parse failures are the string `refused`.
Missing `changes` query is `LXP_ERR_NON_CANONICAL`
(`cmd/layerxd/lxp_daemon_protocol.c:546-547`). Uppercase hex
in an idempotency key is `LXP_ERR_NON_CANONICAL`
(`cmd/layerxd/lxp_daemon_protocol.c:519-523`). At most four
program-listener connections
(`include/layerx/lxp_daemon.h:30`).

The supervisor treats the sequencer as ready when
`GET /v1/programs/account-state/changes?after_sequence=0`
returns `HTTP/1.1 200` and a page matching that records
schema, and the LNI socket exists
(`platform/hosted/node/supervisor.sh:237-268`).

---

## Replica listener

`layerxd --authority-replica` binds only `127.0.0.1`
(`cmd/layerxd/lxp_daemon_authority_replica.c:773-778`). Hosted
port `9402`. Bearer is `Authorization: Bearer` plus
`LAYERX_AUTHORITY_BEARER_TOKEN`, compared constant-time
(`cmd/layerxd/lxp_daemon_authority_replica.c:430-439`).

| Method and path | Success | Refusal |
| --- | --- | --- |
| `POST /v1/receipt-authority/ingest` | `201` `{"authority_replica_id":"<64 hex>"}` (`cmd/layerxd/lxp_daemon_authority_replica.c:501-536`) | `401` without bearer; `503` on append failure |
| `GET /v1/batches/{64 hex}/receipt-authority?receipt_digest={64 hex}` | `200` evidence JSON (`cmd/layerxd/lxp_daemon_authority_replica.c:537-557`) | `401`; `404` on miss or malformed selector |

The supervisor treats the replica as ready on `HTTP/1.1 404`
for the all-zero batch and digest
(`platform/hosted/node/supervisor.sh:247-249`). The sequencer
posts ingest to that replica
(`cmd/layerxd/lxp_daemon_authority_replica.c:916`). Program
and replica tokens must differ; program address/port must
differ from replica address/port
(`platform/hosted/node/bootstrap.sh:339`;
`cmd/layerxd/lxp_daemon_process.c:3770-3777`).

---

## Mounted paths and env

Sequencer container (`platform/hosted/node/deployment.yaml:32-76`):

| Path | Source |
| --- | --- |
| `/var/lib/layerx` | PVC `data` (50Gi) |
| `/run/layerx` | emptyDir Memory 16Mi |
| `/run/layerx/keys/sequencer.key` | Secret `layerx-node-keys` |
| `/run/layerx/keys/treasury.key` | Secret `layerx-node-keys` |
| `/run/layerx/tokens/program-token` | Secret `layerx-node-tokens` |
| `/run/layerx/tokens/replica-token` | Secret `layerx-node-tokens` |
| `/run/layerx/settlement/settlement.env` | ConfigMap `layerx-node-settlement` |

Env from ConfigMap `layerx-node-config`:
`LAYERX_NODE_NETWORK_ID`, `LAYERX_NODE_ASSET_ID`,
`LAYERX_NODE_REPLICA_ID`
(`platform/hosted/node/deployment.yaml:65-68`).
`paxeer-relay` reads `LAYERX_NODE_PAXEER_RELAY_PORT` from that
ConfigMap and `LAYERX_NODE_PAXEER_BOUNDARY`
`paxeer-boundary.layerx-testnet.svc.cluster.local`, with CA
`/run/layerx/trust/ca.crt`
(`platform/hosted/node/deployment.yaml:84-90`;
`platform/hosted/node/deployment.yaml:94`).

TLS Secrets for colocated boundaries:
`layerx-pending-core-tls`, `layerx-pending-core-admin-tls`,
`layerx-receipt-authority-tls`, `layerx-agent-boundary-tls`,
`layerx-internal-ca` (`platform/hosted/node/deployment.yaml:231-235`).

---

## NetworkPolicy

Ingress `layerx-node-ingress`
(`platform/hosted/node/deployment.yaml:266-284`):

| From | Ports |
| --- | --- |
| `app=layerx-testnet-control` | `9443`, `9444`, `9445` |
| `app=layerx-gateway` | `9443`, `9445`, `9446` |
| `app=layerx-program-registry` | `9445`, `9446` |
| `app=layerx-faucet` | `9443` |
| namespace `layerx-developer`, `app=layerx-webhooks` | `9445`, `9446` |
| namespace `layerx-developer`, `app=layerx-dashboard-api` | `9445`, `9446` |

Egress `layerx-node-egress`: DNS `53` and `app=paxeer` TCP
`9443` (`platform/hosted/node/deployment.yaml:286-296`).
Loopback program/replica/LNI/supervisor sockets do not cross
the NetworkPolicy.

---

## Tests

`make platform-test-node` syntax-checks `bootstrap.sh`,
`supervisor.sh`, and `node-test.sh`, then runs
`platform/hosted/node/tests/node-test.sh`
(`platform/Makefile.inc:140-142`). That script requires root
so the LNI client can present a uid other than the daemon
(`platform/hosted/node/tests/node-test.sh:7-9`;
`platform/hosted/node/tests/node-test.sh:29-32`). It proves:

- `node.env` network id, run directory mode `0750` and LNI
  gid, LXGR length `82`, treasury identity line
  (`platform/hosted/node/tests/node-test.sh:125-130`)
- LNI handshake as the client uid: network id, role
  `Sequencer`, sequencer public key, `AccountRead`
  (`platform/hosted/node/tests/node-test.sh:133-139`)
- `layerxctl read-state` `evidence=authenticated_node_snapshot`
  (`platform/hosted/node/tests/node-test.sh:141-146`)
- treasury balance equals `LAYERX_NODE_TREASURY_BALANCE`
  (`platform/hosted/node/tests/node-test.sh:148-153`)
- a signed SEND is `acknowledged` and a repeated canonical
  submit is byte-identical
  (`platform/hosted/node/tests/node-test.sh:155-171`)
- supervisor `status` is `running` generation `1`; `reset`
  rebuilds genesis (new manifest inode, empty checkpoints)
  and generation `2`
  (`platform/hosted/node/tests/node-test.sh:173-195`)
- stopping the supervisors removes `supervisor.sock` and
  leaves no `layerxd` processes
  (`platform/hosted/node/tests/node-test.sh:197-206`)

[Home](Home.md)

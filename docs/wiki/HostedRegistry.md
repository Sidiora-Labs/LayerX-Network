# Hosted program registry

`layerx-program-registry` answers the developer CLI's registry routes from durable local evidence only: reads are re-verified against the canonical deployment journal, and verified-source status is produced by rebuilding published source in a pinned toolchain environment (`platform/hosted/registry/src/lib.rs:1-4`; `platform/hosted/registry/Cargo.toml:12-14`; `platform/hosted/registry/src/main.rs:1`). The crate is `layerx-platform-registry` (`platform/hosted/registry/Cargo.toml:1-2`). The graph anchor is `receipt-verified-program-registry-v1` (`platform/hosted/registry/src/lib.rs:70-74`). The image is `ghcr.io/sidiora-labs/layerx-program-registry:0.1.0`, UID/GID `4030`, command `layerx-program-registry` (`platform/hosted/registry/Dockerfile:5-12`; `platform/hosted/registry/deployment.yaml:12, 14-17`). A second binary, `layerx-cgroup-exec`, supervises one isolated build process tree (`platform/hosted/registry/Cargo.toml:16-18`; `platform/hosted/registry/src/bin/layerx-cgroup-exec.rs:1`).

It registers protocol-verified program deployments, upgrades, lifecycle, value-account bindings, published interfaces, operator-mirrored source, and rebuild verdicts. `POST /__registry/deployments` accepts a canonical signed Programs deploy or upgrade activity as an octet-stream and publishes only after native proof verification; caller-supplied `record_hex` or `proof_hex` JSON is not a supported body (`platform/hosted/registry/src/routes.rs:272, 729-792`; [RegistryDeploymentJournal.md](RegistryDeploymentJournal.md)).

The in-cluster Service is `layerx-program-registry` in `layerx-testnet` on TCP `9420` (`platform/hosted/registry/deployment.yaml:76-79`). Callers that name that URL are the gateway (`platform/hosted/gateway/deployment.yaml:84-85`; `platform/hosted/gateway/src/main.rs:553-557, 1670-1690, 393-411, 2559-2565`) and testnet-control (`platform/hosted/testnet/deployment.yaml:95`; `platform/hosted/testnet/src/main.rs:843-846, 1102`). Ingress NetworkPolicy admits `app: layerx-gateway`, `app: layerx-testnet-control`, and `layerx-role: source-publication-operator` on `9420` (`platform/hosted/registry/deployment.yaml:87-91`). Egress NetworkPolicy admits `layerx-plane: trusted-boundary` on TCP `9445` and `9446`, plus DNS `53` (`platform/hosted/registry/deployment.yaml:92-96`). The node ingress policy admits the registry pod on those same container ports (`platform/hosted/node/deployment.yaml:274-275`). The registry env URLs use Service port `9443` on `layerx-agent-boundary` and `layerx-receipt-authority` (`platform/hosted/registry/deployment.yaml:43-45`; `platform/hosted/node/deployment.yaml:255-261`). Those Services map `9443` to container ports `9446` and `9445` (`platform/hosted/node/deployment.yaml:181, 210, 255-261`).

`platform-hosted-topology-check` loads `platform/hosted/registry/deployment.yaml` among its default manifests (`platform/hosted/tests/topology-check.sh:17-24, 81-88`; `platform/Makefile.inc:177-178`). Beta-cluster apply order is testnet, then gateway, then registry, then developer (`platform/hosted/tests/beta-cluster.sh:825, 859-863`). The StatefulSet selects nodes labelled `layerx.io/program-registry-boundary=v2` (`platform/hosted/registry/deployment.yaml:11`; `platform/hosted/tests/beta-cluster.sh:87, 323`).

---

## Identity and credentials

TLS is mandatory mTLS. The server loads a DER certificate, a process-group-private DER private key, and a client CA, then builds `WebPkiClientVerifier` (`platform/hosted/registry/src/main.rs:199-226`). HTTP without a client certificate does not complete the handshake.

Two bearer planes are distinct (`platform/hosted/registry/src/auth.rs:1, 7-12`; `platform/hosted/registry/src/main.rs:236-242`):

| Plane | Config | HTTP use |
| --- | --- | --- |
| Request | `LAYERX_REGISTRY_REQUEST_TOKEN_FILE` | Every path except `GET /healthz` and `/__registry/sources` (`platform/hosted/registry/src/main.rs:726-732`; `platform/hosted/registry/src/routes.rs:177-202`) |
| Publication | `LAYERX_REGISTRY_PUBLICATION_TOKEN_FILE` | `/__registry/sources` (`platform/hosted/registry/src/main.rs:729-730`; `platform/hosted/registry/src/routes.rs:179-190, 792-794`) |

`RegistryAuthority::new` hashes a 1–4096 byte secret with no ASCII controls and retains only SHA-256 (`platform/hosted/registry/src/auth.rs:44-58`). `verifies` accepts only the exact prefix `Bearer ` (capital B), hashes the remainder, and compares in constant time (`platform/hosted/registry/src/auth.rs:60-76`). Absent, wrong, lowercase `bearer`, and extra-token headers fail (`platform/hosted/registry/src/auth.rs:20-28`). Token files must be canonical absolute regular files, size ≤ 4098 bytes, Unix mode without world bits, and `nlink == 1` (`platform/hosted/registry/src/main.rs:133-162`). Request and publication authorities that hash equal are refused at process start (`platform/hosted/registry/src/main.rs:240-242`).

The gateway mounts Secret `layerx-program-registry-request-client` as `LAYERX_GATEWAY_PROGRAM_REGISTRY_TOKEN_FILE` (`platform/hosted/gateway/deployment.yaml:85, 107, 118`). Publication uses Secret `layerx-program-registry-publication-operator` (`platform/hosted/registry/deployment.yaml:23, 55, 65`).

Node and receipt-authority credentials are environment strings, not token files: `LAYERX_REGISTRY_NODE_AUTHORIZATION` and `LAYERX_REGISTRY_RECEIPT_AUTHORITY_AUTHORIZATION` (`platform/hosted/registry/src/main.rs:293-302`; `platform/hosted/registry/deployment.yaml:43-46`). `NodeProgramStateSource::connect` refuses empty authorizations, a zero replica id, non-HTTPS non-loopback URLs, and identical node and authority endpoints (`platform/hosted/registry/src/node_state.rs:64-80`). Outbound GETs send `Authorization: Bearer {authorization}` (`platform/hosted/registry/src/node_state.rs:278-298`). The node mounts those same secrets as `LAYERX_AGENT_BOUNDARY_REGISTRY_TOKEN_FILE` and `LAYERX_AUTHORITY_TOKEN_FILES` (`platform/hosted/node/deployment.yaml:174, 191, 205, 221, 235, 238`).

The HTTP listener checks bearer before the worker (`platform/hosted/registry/src/main.rs:726-735`). `Registrar::route` checks again (`platform/hosted/registry/src/routes.rs:177-202`). For `/__registry/sources` the listener refuses a missing publication bearer as `401` `authentication_required` (`platform/hosted/registry/src/main.rs:729-735`); `Registrar::route` would refuse the same case as `403` `publication_authority_required` (`platform/hosted/registry/src/routes.rs:179-188`). Clients of the binary observe the listener code.

---

## Routes

HTTP/1.1 only. Query strings are stripped. `Transfer-Encoding` is refused. Duplicate headers are refused. POST/PUT/PATCH require `Content-Length`. Bound is 32 MiB (`platform/hosted/registry/src/http.rs:8, 16-112`). Responses are `Content-Type: application/json`, `Cache-Control: no-store`, `Connection: close` (`platform/hosted/registry/src/http.rs:119-140`).

Each accepted connection is served on a worker that re-opens `Registrar` (`platform/hosted/registry/src/main.rs:471-489, 536-547, 763`). The parent serializes workers on `registrar_gate` (`platform/hosted/registry/src/main.rs:46, 749-763`).

| Method | Path | Auth | Result |
| --- | --- | --- | --- |
| GET | `/healthz` | mTLS only | `200` `{"status":"ready","service":"program-registry"}` (`platform/hosted/registry/src/routes.rs:223-226`) |
| POST | `/__registry/deployments` | request | Submit canonical signed activity bytes; on success `200` with `activity_id`, `receipt_digest`, `state: deployed` after journal append and `pairs/` export (`platform/hosted/registry/src/routes.rs:272, 729-792, 1163-1172`) |
| POST | `/__registry/head` | request | Refresh journal observed head from the independently verified current protocol head (`platform/hosted/registry/src/routes.rs:228, 720-743`) |
| POST | `/__registry/sources` | publication | Publish archive + plan + URI into the source mirror (`platform/hosted/registry/src/routes.rs:229, 745-789`) |
| GET | `/v1/programs/registry/{program_id}` | request | Receipt-verified registry read (`platform/hosted/registry/src/routes.rs:350-355, 371-397`) |
| GET | `/v1/programs/registry/{program_id}/interface` | request | Published interface bound to the current verified version (`platform/hosted/registry/src/routes.rs:356-357, 476-559`) |
| POST | `/v1/programs/registry/{program_id}/source` | request | Rebuild mirrored source and compare to the registered code hash (`platform/hosted/registry/src/routes.rs:359-360, 561-610`) |

`program_id` is thirty-two hexadecimal-encoded bytes (`platform/hosted/registry/src/routes.rs:997-1001, 372-377`). Other methods on the fixed paths are `405` `method_not_allowed`. Unknown paths are `404` `not_found` (`platform/hosted/registry/src/routes.rs:230-237, 351-367`).

Gateway production routes that proxy these reads are `GET /v1/programs/registry/{program_id}` and `GET /v1/programs/registry/{program_id}/interface` (`platform/hosted/gateway/src/lib.rs:793-794, 849-851`). The gateway requires `receipt/verification` equal to `receipt-verified` (`platform/hosted/gateway/src/main.rs:421-427`).

---

## Record contracts

| Record | Where stored / served | Fields |
| --- | --- | --- |
| Deployment envelope | `{journal}/{receipt_digest}.envelope` | Domain `LayerX/deployment-envelope/v1\0`, framed canonical `DeploymentRecord`, framed canonical `DeploymentProof`, SHA-256 seal over both frames (`platform/hosted/registry/src/journal.rs:17-27, 89-97, 115-127`). Filed under the proof's claimed receipt digest (`platform/hosted/registry/src/journal.rs:53-66, 68-72`). |
| Legacy two-file unit | `{digest}.deployment` + `{digest}.admission` | Loaded when both files exist and no envelope is present (`platform/hosted/registry/src/journal.rs:672-695, 729-740`). |
| Observed head | `{journal}/head` | `{sequence}\t{observed_at}\n`. Sequence and observed_at must be non-zero (`platform/hosted/registry/src/journal.rs:599-615, 749-764`). |
| Program-state cache | `{journal}/program-state/{program}.{digest}.program-state` | Canonical `ProtocolProgramStateRead` bytes. Filename digest must match SHA-256 of the bytes (`platform/hosted/registry/src/program_state.rs:11-12, 31-42, 134-141, 48-83`). |
| Program-state cursor | `{journal}/program-state/canonical.cursor` | `{sequence}\t{ordinal}\n`. Advance refuses non-zero ordinal and regression (`platform/hosted/registry/src/program_state.rs:12, 86-132`). |
| Mirrored source | `{mirror}/{digest}.archive`, `.plan`, `.uri` | Archive bytes, `BuildPlan` encoding, URI. Digest is SHA-256 of the encoded archive. URI length 1–512 with no control or whitespace (`platform/hosted/registry/src/mirror.rs:17-20, 87-107, 116-151`). |
| Completed rebuild | `{verified}/{program}-{version}.verified` | JSON `program`, `version`, `source_uri`, `source_digest`, `artifact_digest`, `plan` (`platform/hosted/registry/src/verified.rs:19-28, 53-69`). Replay never asserts a verdict by itself; the digest is compared again to registered protocol state (`platform/hosted/registry/src/verified.rs:1-7`; `platform/hosted/registry/src/routes.rs:696-710`). |
| Registry read body | GET program | `program_id`, `upgrade_policy` (`immutable` or `upgradeable`+`authority`), `lifecycle` (`active`/`deprecated`/`tombstoned`), `state_root`, `observed_sequence`, `observed_at`, `valid_through`, `latest_version`, `versions[]` (`version`, `code_hash`, `abi_version`, `deployment_receipt_digest`, `source`), `lifecycle_history[]`, `exit_routes[]`, `value_accounts`, `receipt` (`deployment_receipt_digest`, `observed_sequence`, `observed_at`, `verification`: `receipt-verified`) (`platform/hosted/registry/src/routes.rs:853-927, 929-995`). ABI 1 with empty value accounts sets `value_accounts.status` to `account-incapable-abi1`. ABI 2 attaches receipt-proven balances with `verification`: `account-primary-and-state-proof-verified`. |
| Interface body | GET interface | `program_id`, `version`, `code_hash`, `abi_version`, `interface` (hex of canonical encoding), `interface_digest`, `deployment_receipt_digest`, `state_root`, `observed_sequence`, `observed_at`, `valid_through`, `source`, `verification`: `deployment-interface-and-current-head-verified` (`platform/hosted/registry/src/routes.rs:540-558`). |
| Source publish body | POST `/__registry/sources` | Request JSON `source_uri`, `plan`, `archive_hex`. Response `mirrored`, `source_uri`, `source_digest` (`platform/hosted/registry/src/routes.rs:745-786`). |
| Source verify request | POST `…/source` | JSON `source_uri`, `source_digest` (32-byte hex). Header `Idempotency-Key` 16–128 ASCII alnum, `-`, `_` (`platform/hosted/registry/src/routes.rs:569-588, 1023-1028, 1003-1008`). |
| Source verify success | `200` | `program_id`, `version`, `source_uri`, `source_digest`, `environment_digest`, `reproduced_artifact_digest`, `source`, `pipeline` (`platform/hosted/registry/src/routes.rs:816-837`). |
| Source mismatch | `409` | Refusal envelope plus `verification` object (`platform/hosted/registry/src/routes.rs:838-850`). |
| Head ingest body | POST `/__registry/head` | `observed`, `sequence`, `observed_at`, `receipt_digest`, `state_root` (`platform/hosted/registry/src/routes.rs:730-739`). |
| Source status | nested `source` | `unpublished`; `verified` with `source_digest`/`environment_digest`/`pipeline`; `mismatch` with `expected_code_hash`/`reproduced_artifact_digest` (`platform/hosted/registry/src/routes.rs:939-960`). |

Refusal envelope (except source-mismatch, which adds `verification`): `{"error":{"code","retry":"never","detail"}}` (`platform/hosted/registry/src/routes.rs:796-803`).

---

## Storage and durability

`LAYERX_REGISTRY_STATE` defaults to `/var/lib/layerx-program-registry` (`platform/hosted/registry/src/main.rs:26, 233`). Under that root: `journal`, `sources`, `verified`, `builds` unless overridden (`platform/hosted/registry/src/main.rs:249-252`). The StatefulSet mounts a 100Gi RWO PVC at `/var/lib/layerx-program-registry` (`platform/hosted/registry/deployment.yaml:21, 53, 72-74`).

`write_atomic` writes `{name}.tmp`, `sync_all`, then `rename` (`platform/hosted/registry/src/lib.rs:76-94`). Envelope publication also `sync_all`s the journal directory after rename (`platform/hosted/registry/src/journal.rs:506-538`). The seven write steps are create-temporary, write-record, write-proof, write-seal, sync-temporary, commit, sync-directory (`platform/hosted/registry/src/journal.rs:284-307`). A crash before commit leaves an incomplete unit; `load` quarantines it with a typed `UnitDefect` and still loads complete units (`platform/hosted/registry/src/journal.rs:378-380, 650-721`). `proofs()` refuses a journal whose committed projections and admission proofs are not the same set (`platform/hosted/registry/src/journal.rs:459-463`).

Program-state `audit` hash-checks cache filenames. It does not construct a verified read; restart publication requires a fresh node receipt/head resolution (`platform/hosted/registry/src/program_state.rs:45-47, 48-83`). `Registrar::open` rebuilds the projection from the journal and stored rebuilds, then `synchronize_protocol_state` (`platform/hosted/registry/src/routes.rs:85-146, 680-717`). Synchronization pages node change notices (max 1024 pages, 4096 records per page), independently verifies the current head against the receipt authority, restores ABI-2 program state through `ProtocolProgramStateRead::restore_verified`, persists, then advances the cursor (`platform/hosted/registry/src/routes.rs:251-348`; `platform/hosted/registry/src/node_state.rs:15, 177-264, 314-392`). ABI 1 with empty value accounts is skipped (`platform/hosted/registry/src/routes.rs:313-315`). Other ABI values are refused (`platform/hosted/registry/src/routes.rs:316-318`).

Build workspaces remain hostPath quota slots at `/var/lib/layerx-program-registry-builds`. The node oneshot provisions and checks ext4 quota slots through systemd-mount, with no cgroup delegation. The registry mounts `/sys/fs/cgroup/kubelet.slice` read-write at `/run/layerx/host-cgroup` and requires `LAYERX_REGISTRY_HOST_CGROUP_MOUNT`. Startup requires UID 0 with only CHOWN, SETUID and SETGID, finds its namespace root C by device/inode equality with `/sys/fs/cgroup` in a depth-eight walk, refuses duplicate matches, and confirms its PID in C. Discovery descends only into root-owned directories, logs each excluded non-root owner UID once at debug level, treats unreadable root-owned directories as fatal, and never descends into the matched C. It requires cpu, memory, pids and io, moves to `C/main`, enables controllers in C and `C/workers`, and delegates C's cgroup.procs plus the workers directory and its procs, threads and subtree-control files to 4030:4030. Before any secret, token, journal or listener, it disables keepcaps, sets supplementary groups and all saved/real/effective GIDs and UIDs to 4030, checks all four identity columns and zero CapEff/CapPrm/CapInh, and requires root restoration to fail with EPERM. State files are created after this drop. Worker and build cgroups live inside `C/workers/request-<pid>-<ts>/{worker,builds}`. Enabling controllers in C makes the kernel refuse new direct member processes: `kubectl exec` is refused by design, and readiness remains a `tcpSocket` probe. `HermeticBuilder` refuses a cgroup not owned by 4030:4030, missing controllers, or zero mounted quota slots (`platform/hosted/registry/src/builder.rs:380-421`). Each rebuild runs `layerx-cgroup-exec` then `bwrap --unshare-all --disable-userns --cap-drop ALL --ro-bind` of the pinned environment, no network, `CARGO_NET_OFFLINE=true` (`platform/hosted/registry/src/builder.rs:274-338`). Artifact open uses `openat2` with `ResolveFlags::BENEATH` (`platform/hosted/registry/src/builder.rs:698-713`). Artifact bound is 32 MiB (`platform/hosted/registry/src/builder.rs:25`).

The in-memory idempotency map is constructed empty at each `Registrar::open` (`platform/hosted/registry/src/routes.rs:80, 140, 652-677`). Workers are one process per request (`platform/hosted/registry/src/main.rs:536-547, 471-489`). `503` responses are not stored even inside one worker (`platform/hosted/registry/src/routes.rs:605-608`).

---

## Typed refusals

| HTTP | Code | When |
| --- | --- | --- |
| 400 | `invalid_request` | HTTP framing failed (`platform/hosted/registry/src/main.rs:723-724`) |
| 400 | `invalid_argument` | Bad program id, idempotency key charset/length, source body, source publish body, or head sequence/time (`platform/hosted/registry/src/routes.rs:372-377, 576-588, 741, 746-768, 787`) |
| 400 | `idempotency_key_required` | POST source without `Idempotency-Key` (`platform/hosted/registry/src/routes.rs:569-574`) |
| 401 | `authentication_required` | Listener: missing/wrong bearer (`platform/hosted/registry/src/main.rs:734-735`). `Registrar::route`: missing/wrong request bearer (`platform/hosted/registry/src/routes.rs:192-200`) |
| 403 | `publication_authority_required` | `Registrar::route` on `/__registry/sources` without publication bearer (`platform/hosted/registry/src/routes.rs:179-188`) |
| 404 | `not_found` | Unknown route or unregistered program (`platform/hosted/registry/src/routes.rs:351-352, 379-380, 613-616`) |
| 404 | `interface_absent` | No published interface for the current version (`platform/hosted/registry/src/routes.rs:507-512`) |
| 404 | `source_not_mirrored` | Mirror has no archive for that digest (`platform/hosted/registry/src/routes.rs:619-621`; `platform/hosted/registry/src/mirror.rs:32-33, 118-120`) |
| 405 | `method_not_allowed` | Wrong method on a known path (`platform/hosted/registry/src/routes.rs:230-237, 362-366`) |
| 409 | `idempotency_conflict` | Same scoped key, different request digest (`platform/hosted/registry/src/routes.rs:592-598`) |
| 409 | `source_mismatch` | Rebuilt artifact does not hash to the registered code hash (`platform/hosted/registry/src/routes.rs:838-850`) |
| 422 | `source_unverifiable` | Mirror URI/digest/plan refusal, or other `BuildRefusal` (`platform/hosted/registry/src/routes.rs:622, 805-813`; `platform/hosted/registry/src/mirror.rs:32-38`) |
| 422 | `build_failed` | `BuildRefusal::BuilderFailed` (`platform/hosted/registry/src/routes.rs:808`) |
| 422 | `build_not_reproducible` | `BuildRefusal::NondeterministicBuild` (`platform/hosted/registry/src/routes.rs:809-811`) |
| 502 | `unverified_read` | `Registry::read` error other than unknown/stale (`platform/hosted/registry/src/routes.rs:395, 502`) |
| 502 | `balance_protocol_unsupported` | Value accounts without ABI 2 (`platform/hosted/registry/src/routes.rs:428-433`) |
| 502 | `balance_registry_mismatch` | Balance proof program/lifecycle/bindings disagree with the registry record (`platform/hosted/registry/src/routes.rs:460-468`) |
| 502 | `interface_registry_mismatch` | Interface code hash or ABI disagrees with the current version (`platform/hosted/registry/src/routes.rs:514-521`) |
| 422 | `deployment_proof_refused` | Native proof failed cryptographic replay (`platform/hosted/registry/src/routes.rs:741, 772`) |
| 422 | `deployment_projection_refused` | Verified evidence could not be applied to the in-memory registry (`platform/hosted/registry/src/routes.rs:778-779`) |
| 503 | `deployment_proof_unavailable` | Empty body, or native admission/proof production failed (`platform/hosted/registry/src/routes.rs:748-757, 1126-1131`) |
| 503 | `journal_unavailable` | Journal load or append failed (`platform/hosted/registry/src/routes.rs:732, 781-782`) |
| 503 | `journal_export_unavailable` | Pair export under `pairs/` failed (`platform/hosted/registry/src/routes.rs:743-744, 789-790`) |
| 503 | `clock_unavailable` | Deployment verification could not read a UNIX-epoch clock (`platform/hosted/registry/src/routes.rs:759-768`) |
| 503 | `receipt_authority_unavailable` | Independent receipt-authority check failed (`platform/hosted/registry/src/routes.rs:774-775`) |
| 503 | `request_deadline_exceeded` | Absolute request timeout (`platform/hosted/registry/src/main.rs:497-501, 766-771`; `platform/hosted/registry/src/routes.rs:157-162, 205-210`) |
| 503 | `worker_unavailable` | Isolated worker IPC/cgroup/start/attach/status (`platform/hosted/registry/src/main.rs:509-661`) |
| 503 | `build_queue_full` | Concurrent POST `…/source` at `max_builds` (`platform/hosted/registry/src/main.rs:737-743`) |
| 503 | `registry_unavailable` | Poisoned `registrar_gate` (`platform/hosted/registry/src/main.rs:752-753`) |
| 503 | `builder_unavailable` | Deadline lock or `BuildRefusal::SandboxUnavailable` (`platform/hosted/registry/src/routes.rs:166-175, 807`) |
| 503 | `protocol_state_unavailable` | Node/authority sync or current-head fetch failed (`platform/hosted/registry/src/routes.rs:382-383, 720-723`) |
| 503 | `read_unverifiable` | `JournalReadAuthority` refused (`platform/hosted/registry/src/routes.rs:385-387`) |
| 503 | `stale_read` | `RegistryError::StaleRead` or head outside freshness (`platform/hosted/registry/src/routes.rs:394, 407-415`) |
| 503 | `protocol_head_unavailable` | No current independently verified head (`platform/hosted/registry/src/routes.rs:400-405, 523-528`) |
| 503 | `balance_read_unavailable` | ABI 2 without a current balance proof (`platform/hosted/registry/src/routes.rs:435-440`) |
| 503 | `stale_balance_read` | Balance proof not current at the observed registry head (`platform/hosted/registry/src/routes.rs:443-451`) |
| 503 | `persistence_unavailable` | Verified-source store write failed (`platform/hosted/registry/src/routes.rs:646-647`) |

`write_response` also names reason `413 Content Too Large` (`platform/hosted/registry/src/http.rs:128`). No registry path sets status `413`. Connections beyond `max_connections` are shut down with no HTTP response (`platform/hosted/registry/src/main.rs:690-693`).

---

## Readiness

The StatefulSet probe is `tcpSocket` on container port `registry` (9420), period 5s, failureThreshold 3 (`platform/hosted/registry/deployment.yaml:18, 49`). The crate test requires that deployment YAML not contain `httpGet: {path: /healthz` (`platform/hosted/registry/src/main.rs:848-849`).

`GET /healthz` returns `ready` / `program-registry` without consulting node, authority, journal, or builder (`platform/hosted/registry/src/routes.rs:223-226`). The gateway treats that JSON plus HTTP 200 as `program_registry: ready` (`platform/hosted/gateway/src/main.rs:1670-1690, 2793-2807`). Testnet-control probes the registry with TCP only; the success detail is `TCP connection accepted; the registry requires client-certificate TLS beyond this probe` (`platform/hosted/testnet/src/main.rs:1047-1053, 1102`).

---

## Configuration keys

Process keys from `config()` and `tls_config()` (`platform/hosted/registry/src/main.rs:199-321`). Deployment values from `platform/hosted/registry/deployment.yaml:19-48`.

| Key | Bound / default | Deployment |
| --- | --- | --- |
| `LAYERX_REGISTRY_LISTEN` | default `127.0.0.1:9420` (`platform/hosted/registry/src/main.rs:25, 248`) | `0.0.0.0:9420` |
| `LAYERX_REGISTRY_STATE` | default `/var/lib/layerx-program-registry` (`platform/hosted/registry/src/main.rs:26, 233`) | `/var/lib/layerx-program-registry` |
| `LAYERX_REGISTRY_JOURNAL` | default `$STATE/journal` (`platform/hosted/registry/src/main.rs:249`) | unset |
| `LAYERX_REGISTRY_SOURCE_MIRROR` | default `$STATE/sources` (`platform/hosted/registry/src/main.rs:250`) | unset |
| `LAYERX_REGISTRY_VERIFIED` | default `$STATE/verified` (`platform/hosted/registry/src/main.rs:251`) | unset |
| `LAYERX_REGISTRY_BUILD_ROOT` | default `$STATE/builds` (`platform/hosted/registry/src/main.rs:252`) | `/run/layerx/quota` |
| `LAYERX_REGISTRY_REQUEST_TOKEN_FILE` | required secret file (`platform/hosted/registry/src/main.rs:237`) | `/run/layerx/request/token` |
| `LAYERX_REGISTRY_PUBLICATION_TOKEN_FILE` | required secret file, must differ (`platform/hosted/registry/src/main.rs:238-242`) | `/run/layerx/publication/token` |
| `LAYERX_REGISTRY_TLS_CERT_DER` | required, 1–65536 bytes (`platform/hosted/registry/src/main.rs:203-206`) | `/run/layerx/server/tls.crt.der` |
| `LAYERX_REGISTRY_TLS_KEY_DER` | required private canonical file (`platform/hosted/registry/src/main.rs:207-209`) | `/run/layerx/server/tls.key.der` |
| `LAYERX_REGISTRY_CLIENT_CA_DER` | required, 1–65536 bytes (`platform/hosted/registry/src/main.rs:210-213`) | `/run/layerx/client-ca/ca.crt.der` |
| `LAYERX_REGISTRY_BUILDER_IMAGE_DIGEST` | required 32-byte hex (`platform/hosted/registry/src/main.rs:234-254`) | ConfigMap `layerx-program-builder-release` key `environment-tree-digest` |
| `LAYERX_REGISTRY_BUILDER_ENVIRONMENT_ROOT` | required (`platform/hosted/registry/src/main.rs:255-258`) | `/opt/layerx-builder/rootfs` |
| `LAYERX_REGISTRY_BUILDER_ENTRYPOINT` | required, absolute, no `..` (`platform/hosted/registry/src/main.rs:259-260`; `platform/hosted/registry/src/builder.rs:92-94`) | `/bin/layerx-build` |
| `LAYERX_REGISTRY_BUILDER_ISOLATION_RUNTIME` | required (`platform/hosted/registry/src/main.rs:261-264`) | `/usr/bin/bwrap` |
| `LAYERX_REGISTRY_BUILDER_ISOLATION_RUNTIME_DIGEST` | required 32-byte hex (`platform/hosted/registry/src/main.rs:265-270`) | ConfigMap key `bwrap-digest` |
| `LAYERX_REGISTRY_BUILDER_JOB_SUPERVISOR` | required (`platform/hosted/registry/src/main.rs:271-274`) | `/usr/bin/layerx-cgroup-exec` |
| `LAYERX_REGISTRY_BUILDER_JOB_SUPERVISOR_DIGEST` | required 32-byte hex (`platform/hosted/registry/src/main.rs:275-280`) | ConfigMap key `cgroup-exec-digest` |
| `LAYERX_REGISTRY_HOST_CGROUP_MOUNT` | required before configuration and privilege drop | `/run/layerx/host-cgroup` |
| `LAYERX_REGISTRY_BUILD_TIMEOUT_SECONDS` | default 1800; builder admits 1..=3600 (`platform/hosted/registry/src/main.rs:285`; `platform/hosted/registry/src/builder.rs:88`) | `1800` |
| `LAYERX_REGISTRY_BUILD_MEMORY_BYTES` | default 2147483648; builder admits 67108864..=8589934592 (`platform/hosted/registry/src/main.rs:286`; `platform/hosted/registry/src/builder.rs:89`) | `2147483648` |
| `LAYERX_REGISTRY_BUILD_PROCESS_LIMIT` | default 64; builder admits 1..=256 (`platform/hosted/registry/src/main.rs:287`; `platform/hosted/registry/src/builder.rs:90`) | `64` |
| `LAYERX_REGISTRY_BUILD_FILE_SIZE_BYTES` | default 67108864; builder admits 33554432..=134217728 (`platform/hosted/registry/src/main.rs:288`; `platform/hosted/registry/src/builder.rs:91`) | `67108864` |
| `LAYERX_REGISTRY_ATTEMPTS` | default 2 (`platform/hosted/registry/src/main.rs:289`) | unset |
| `LAYERX_REGISTRY_MAX_STALENESS_SECONDS` | default 300; converted to ms; `0` refused (`platform/hosted/registry/src/main.rs:290-292`; `platform/hosted/registry/src/routes.rs:111-113`) | unset |
| `LAYERX_REGISTRY_REQUEST_TIMEOUT_SECONDS` | default 1800; admits 1..=3600 (`platform/hosted/registry/src/main.rs:243-246`) | `1800` |
| `LAYERX_REGISTRY_MAX_CONNECTIONS` | default 128; admits 1..=1024 (`platform/hosted/registry/src/main.rs:318`) | `128` |
| `LAYERX_REGISTRY_MAX_BUILDS` | default 4; admits 1..=64 (`platform/hosted/registry/src/main.rs:319`) | `4` |
| `LAYERX_REGISTRY_NODE_ENDPOINT` | required (`platform/hosted/registry/src/main.rs:293-294`) | `https://layerx-agent-boundary.layerx-testnet.svc.cluster.local:9443` |
| `LAYERX_REGISTRY_NODE_AUTHORIZATION` | required env secret (`platform/hosted/registry/src/main.rs:295-296`) | Secret `layerx-program-registry-node-client` key `token` |
| `LAYERX_REGISTRY_OUTBOUND_CA_DER` | required DER CA that the registry trusts for its outbound node-state and receipt-authority HTTPS calls (`platform/hosted/registry/src/main.rs`) | `/run/layerx/client-ca/ca.crt.der` (Secret `layerx-internal-ca`) |
| `LAYERX_REGISTRY_RECEIPT_AUTHORITY_ENDPOINT` | required (`platform/hosted/registry/src/main.rs:297-298`) | `https://layerx-receipt-authority.layerx-testnet.svc.cluster.local:9443` |
| `LAYERX_REGISTRY_RECEIPT_AUTHORITY_AUTHORIZATION` | required env secret (`platform/hosted/registry/src/main.rs:299-302`) | Secret `layerx-program-registry-authority-client` key `token` |
| `LAYERX_REGISTRY_RECEIPT_AUTHORITY_REPLICA_ID` | required 32-byte hex (`platform/hosted/registry/src/main.rs:303-310`) | ConfigMap `layerx-receipt-authority` key `replica-id` |
| `LAYERX_REGISTRY_SEQUENCER_TRUST_HISTORY` | required path (`platform/hosted/registry/src/main.rs:311-314`) | `/run/layerx/trust/history` |

Node provisioner keys (not read by `config()`): `LAYERX_REGISTRY_NODE_QUOTA_ROOT`, `LAYERX_REGISTRY_BUILD_QUOTA_BYTES`, `LAYERX_REGISTRY_BUILD_QUOTA_INODES`, `LAYERX_REGISTRY_NODE_LOCK` (`platform/hosted/registry/node-provision-build-boundary.sh:4-12`; `platform/hosted/registry/layerx-program-registry-boundary.service:10-12`).

`LAYERX_REGISTRY_RECEIPT_AUTHORITY_REPLICA_ID` names ConfigMap `layerx-receipt-authority`. That ConfigMap is not in `platform/hosted/registry/deployment.yaml`. Beta-cluster creates it (`platform/hosted/tests/beta-cluster.sh:604`) and also writes the same generated replica id into ConfigMap `layerx-node-config` (`platform/hosted/node/deployment.yaml:8`; `platform/hosted/tests/beta-cluster.sh:798-800`).

---

## Make targets

`make platform-test-registry` runs `cargo test --offline --manifest-path platform/Cargo.toml --locked -p layerx-platform-registry` (`platform/Makefile.inc:8, 137-138`). `make platform-hosted-topology-check` runs `topology-check.sh` (`platform/Makefile.inc:177-178`).

---

## What the tests prove

| Test | Proves |
| --- | --- |
| `auth::tests::bearer_authority_refuses_absent_wrong_and_ambiguous_credentials` | Exact `Bearer` match; refuses absent, one-bit-wrong, lowercase scheme, extra token (`platform/hosted/registry/src/auth.rs:19-28`) |
| `http::tests::refuses_duplicate_content_length_and_transfer_encoding` | Duplicate `Content-Length` and `Transfer-Encoding: chunked` fail parse (`platform/hosted/registry/src/http.rs:168-172`) |
| `http::tests::accepts_one_canonical_bounded_request` | One `POST /__registry/head` with a single `Content-Length` parses (`platform/hosted/registry/src/http.rs:174-179`) |
| `routes::tests::deployment_ingress_stays_blocked_for_forgeable_record_shape` | The unavailable helper still returns `503` for `{"record_hex":…}` (`platform/hosted/registry/src/routes.rs:1179-1183`) |
| `routes::tests::deployment_ingress_does_not_accept_caller_proof_bytes` | The unavailable helper still returns `503` for `{"proof_hex":…}` (`platform/hosted/registry/src/routes.rs:1185-1189`) |
| `main::tests::deployment_contract_keeps_https_mtls_and_bearer_roles_aligned` | Gateway URL `https://layerx-program-registry.layerx-testnet.svc.cluster.local:9420`; deployment contains TLS/token/builder keys; readiness is not HTTP `/healthz`; builder source contains `--unshare-all`, `--disable-userns`, `openat2(`, `ResolveFlags::BENEATH`; main contains deadline/worker/`MAX_WORKER_IPC_BYTES` and does not contain `std::process::abort()`; cgroup supervisor contains `memory.max`, `cgroup.kill`, `Signal::STOP`/`CONT`; provisioner contains `mkfs.ext4`, `e2fsck -p` (`platform/hosted/registry/src/main.rs:827-899`) |
| `main::tests::request_deadline_cancels_only_the_bounded_worker_and_preserves_listener_liveness` | Source contains `registrar_gate`, `--stopped-request-worker`, `worker_group.kill()`, `cgroup.kill`, `child.wait()` (`platform/hosted/registry/src/main.rs:901-913`) |
| `main::tests::delegation_quota_and_open_inode_execution_fail_closed` | Provisioner and unit contain no cgroup reference; quota and capability checks remain; container delegation and privilege-drop contracts are asserted; deployment contains `layerx.io/program-registry-boundary: "v2"`; builder contains `NonBlockingLockExclusive` and `/proc/self/fd/` (`platform/hosted/registry/src/main.rs:915-946`) |
| `tests/journal.rs::interruption_at_every_write_step_recovers_on_restart_without_repair` | Each of seven write steps either commits or quarantines; restart plus retry reproduces the projection (`platform/hosted/registry/tests/journal.rs:211-299`) |
| `tests/journal.rs::replaying_the_journal_reproduces_the_projection_of_its_evidence` | Digest-order replay equals the in-memory projection of the same evidence (`platform/hosted/registry/tests/journal.rs:301-337`) |
| `tests/journal.rs::envelope_encoding_round_trips_and_names_the_missing_part` | Truncation names Record/Proof/Seal; trailing bytes and bad seal are `Corrupt`; seven distinct `WriteStep` strings (`platform/hosted/registry/tests/journal.rs:339-421`) |
| `tests/journal.rs::startup_quarantines_defective_units_and_loads_the_rest` | Truncated, corrupt, misfiled, unreadable units are quarantined; complete units still load; `proofs()` fails on unequal sets (`platform/hosted/registry/tests/journal.rs:423-544`) |
| `tests/journal.rs::legacy_two_file_units_load_when_complete_and_quarantine_when_not` | Split `.deployment`/`.admission` load when paired; lone halves quarantine (`platform/hosted/registry/tests/journal.rs:546-662`) |
| `tests/journal.rs::observed_head_round_trips_and_refuses_absent_observations` | Missing head is `JournalUnavailable`; sequence `0` is refused; a non-zero head round-trips (`platform/hosted/registry/tests/journal.rs:664-687`) |

There are no `#[test]` functions in `builder.rs`, `mirror.rs`, `verified.rs`, `node_state.rs`, or `program_state.rs`.

---

## Facts the sources disagree on or leave unset

`POST /__registry/deployments` writes a sealed journal envelope and exports Human-consumed `{digest}.admission` / `{digest}.deployment` files under `pairs/` after native proof verification (`platform/hosted/registry/src/routes.rs:729-792`; `platform/hosted/registry/src/journal.rs:501-538`). An admission acknowledgement alone never produces a record. Empty bodies and failed native proof production remain `503` `deployment_proof_unavailable`. The helper tests in `routes.rs` pin that unavailable envelope; they do not exercise the live ingest path (`platform/hosted/registry/src/routes.rs:1179-1189`). Human evidence assembly requires a pairs-only directory; copying the journal root (envelopes, `head`, `pairs/`) is refused (`platform/hosted/human/provision.py:364-390`). The cluster materializer currently copies the journal root (`platform/hosted/human/provision.sh:111-113`).

NetworkPolicy ingress names `layerx-role: source-publication-operator` (`platform/hosted/registry/deployment.yaml:90-91`). No workload with that label is in the default topology-check manifests (`platform/hosted/tests/topology-check.sh:81-88`).

`GET /healthz` does not inspect protocol state (`platform/hosted/registry/src/routes.rs:223-226`). The kubelet probe does not call it (`platform/hosted/registry/deployment.yaml:49`; `platform/hosted/registry/src/main.rs:848-849`).

[Home](Home.md)

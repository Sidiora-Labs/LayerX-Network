# Disposable beta cluster

The beta cluster is a disposable in-cluster Paxeer chain with EVM chain id `125` plus the LayerX daemon, boundary, and authority images (`platform/hosted/tests/beta-cluster.sh:10`, `platform/hosted/tests/beta-cluster.sh:89-90`, `platform/hosted/tests/beta-cluster.sh:96`). Unset `LAYERX_BETA_KUBECONFIG` selects a local kind cluster named `layerx-beta`; a readable kubeconfig targets an owner cluster (`platform/hosted/tests/beta-cluster.sh:10`, `platform/hosted/tests/beta-cluster.sh:73`, `platform/hosted/tests/beta-cluster.sh:146-148`). The image set includes `layerx-node`, `layerx-core-boundary`, `layerx-receipt-authority`, `layerx-agent-boundary`, `layerx-paxeer-boundary`, `paxd-node`, and `paxd` (`platform/hosted/tests/beta-cluster.sh:89-90`). The Paxeer chain id the script binds is `125` (`platform/hosted/tests/beta-cluster.sh:96`, `platform/hosted/paxeer/deployment.yaml:41`).

Trusted-boundary Services applied with the node are `layerx-pending-core`, `layerx-pending-core-admin`, `paxeer-boundary`, `layerx-identity`, `layerx-receipt-authority`, and `layerx-agent-boundary` (`platform/hosted/tests/beta-cluster.sh:91`, `platform/hosted/tests/beta-cluster.sh:838-845`).

Work products land under `build/beta-cluster` (`platform/hosted/tests/beta-cluster.sh:55`).

## Make goals

Root `Makefile` includes the platform recipes (`Makefile:2802`). The operator chain is:

1. `make platform-test-tooling` — crate tests, then `sh -n` / `bash -n` of `platform/hosted/paxeer/init-chain.sh`, `platform/hosted/tests/beta-cluster.sh`, and `platform/hosted/paxeer/deploy-contracts.sh`, plus `python3 -m py_compile` of `platform/hosted/paxeer/prepare-beta.py` (`platform/Makefile.inc:115-129`).
2. `make platform-beta-cluster-up` — `bash platform/hosted/tests/beta-cluster.sh up $(PLATFORM_BETA_CLUSTER_FLAGS)` (`platform/Makefile.inc:184-185`).
3. `make platform-hosted-smoke` — sources `PLATFORM_BETA_CLUSTER_ENV` (default `build/beta-cluster/env`) and refuses any empty required smoke input, then runs `platform/hosted/testnet/tests/hosted-smoke.sh` (`platform/Makefile.inc:3`, `platform/Makefile.inc:161-173`).
4. `make platform-beta-cluster-down` — `bash platform/hosted/tests/beta-cluster.sh down` (`platform/Makefile.inc:187-188`).

When `platform-beta-cluster-up` or `platform-beta-cluster-down` is among the make goals, make sets `.NOTPARALLEL` (`platform/Makefile.inc:180-182`). Smoke consumes the env file that `up` writes (`platform/Makefile.inc:3`, `platform/hosted/tests/beta-cluster.sh:61`, `platform/hosted/tests/beta-cluster.sh:1090-1113`, `platform/hosted/tests/beta-cluster.sh:1264`).

## Bring-up order

`beta-cluster.sh up` runs `beta_cluster_up` (`platform/hosted/tests/beta-cluster.sh:5`, `platform/hosted/tests/beta-cluster.sh:1331-1338`). After host-tool, Foundry, custody-profile, disk, image, cluster, CA, and secret steps, it renders manifests, publishes the builder release, and applies trusted-boundary manifests before the testnet, gateway, registry, and developer manifests (`platform/hosted/tests/beta-cluster.sh:41-47`, `platform/hosted/tests/beta-cluster.sh:1216-1257`).

Inside that binding: the Paxeer chain starts with the generated deployer address, the node bootstraps its genesis, `deploy-contracts.sh` deploys settlement contracts from the node's genesis artifacts through the Paxeer boundary, and the resulting GuarantorBond and CheckpointRegistry addresses are published as ConfigMap `layerx-node-settlement` (`platform/hosted/tests/beta-cluster.sh:41-47`, `platform/hosted/tests/beta-cluster.sh:1247-1253`, `platform/hosted/tests/beta-cluster.sh:928-938`). `trusted_boundary_apply` applies `paxeer.yaml`, then `identity.yaml`, then `node.yaml` (`platform/hosted/tests/beta-cluster.sh:838-842`).

## Ordinary-genesis bootstrap

The Paxeer StatefulSet init container `genesis` runs `bash /opt/layerx/init-chain.sh` with home `/var/lib/paxeer` (`platform/hosted/paxeer/deployment.yaml:12-22`). `init-chain.sh` initialises a single-validator Paxeer chain (`platform/hosted/paxeer/init-chain.sh:2`). It refuses any `LAYERX_PAXEER_CHAIN_ID` other than `125`, then sets the CometBFT chain id to `hyperpax_125-1` (`platform/hosted/paxeer/init-chain.sh:16`, `platform/hosted/paxeer/init-chain.sh:39-42`). It runs `paxd init` for that chain id, funds the validator and deployer, writes a one-validator genesis, collects gentxs, and validates genesis (`platform/hosted/paxeer/init-chain.sh:84-113`). Tendermint RPC and P2P, gRPC, and EVM HTTP bind on loopback; the EVM listener is reached through the boundary container (`platform/hosted/paxeer/init-chain.sh:9-10`, `platform/hosted/paxeer/init-chain.sh:117-128`). A completed home is marked and skipped; a partial `genesis.json` without the marker is refused (`platform/hosted/paxeer/init-chain.sh:59-65`).

The primary `paxd` container is `paxd start --home /var/lib/paxeer` (`platform/hosted/paxeer/deployment.yaml:26-29`). The boundary container listens on `0.0.0.0:9443` and talks to `http://127.0.0.1:8545` with chain id `125` (`platform/hosted/paxeer/deployment.yaml:35-41`).

Bring-up then waits until the LayerX node has `node.env`, `genesis/paxeer-deployment-descriptor.lxgd`, and `genesis/paxeer-registration-request.lxrr` (`platform/hosted/tests/beta-cluster.sh:865-878`). Settlement deploy is `deploy-contracts.sh bootstrap` with those artifacts (`platform/hosted/tests/beta-cluster.sh:910-917`). Bootstrap requires the `immediate-beta` timelock profile and runs deploy, permissions, activate, bond, and finalize in that order (`platform/hosted/paxeer/deploy-contracts.sh:9`, `platform/hosted/paxeer/deploy-contracts.sh:417-425`). The beta input pins `timelock_profile` `immediate-beta` and `timelock_delay` `0` (`platform/hosted/paxeer/deployment-input.beta.json:6`, `platform/hosted/paxeer/deployment-input.beta.json:26`). `immediate-beta` refuses a chain id other than `125` and a delay other than zero (`platform/hosted/paxeer/deploy-contracts.sh:147-149`).

`prepare-beta.py` writes a fresh directory from `deployment-input.beta.json` with `protocol_version` `3`, a `deployment-input.json`, `guarantors.json`, and `genesis-request.lxgb` (`platform/hosted/paxeer/prepare-beta.py:12`, `platform/hosted/paxeer/prepare-beta.py:81-96`). The cluster `up` path overlays `deployment-input.beta.json` itself and does not invoke `prepare-beta.py` (`platform/hosted/tests/beta-cluster.sh:905-908`).

## Custody-profile input

`LAYERX_BETA_CUSTODY_PROFILE` defaults to empty (`platform/hosted/tests/beta-cluster.sh:94`). Empty skips validation (`platform/hosted/tests/beta-cluster.sh:1203-1204`). A set value is refused unless it names a readable regular file that is not a symlink (`platform/hosted/tests/beta-cluster.sh:1205-1206`). It is refused unless the file is exactly 207 bytes (`platform/hosted/tests/beta-cluster.sh:1207-1208`). Validation runs on both `up` and `render` (`platform/hosted/tests/beta-cluster.sh:1220`, `platform/hosted/tests/beta-cluster.sh:1303`).

An accepted profile is applied as ConfigMap `layerx-node-custody-profile` and mounted read-only at `/run/layerx/custody.profile` with `layerxd --custody-profile` (`platform/hosted/tests/beta-cluster.sh:603-605`, `platform/hosted/tests/beta-cluster.sh:791-805`).

The 207-byte profile identity used by `tests/bridge/custody_genesis.py` is magic `LXBC1`, protocol `3` at the last two bytes, EVM chain id at bytes `5:13`, and genesis document sha256 at bytes `169:201` (`tests/bridge/custody_genesis.py:22-24`, `tests/bridge/custody_genesis.py:30`). Chain id `125` (or `--disposable-identity`) refuses unless `--rpc`, `--ca-bundle`, and `--disposable-identity` are all supplied (`tests/bridge/custody_genesis.py:25-27`). It refuses unless `eth_chainId` matches the profile chain id and the RPC genesis sha256 matches bytes `169:201` (`tests/bridge/custody_genesis.py:29-30`).

## Primary node and observer node

The Paxeer StatefulSet has one replica and a `data` PVC of 40Gi mounted at `/var/lib/paxeer` (`platform/hosted/paxeer/deployment.yaml:6`, `platform/hosted/paxeer/deployment.yaml:22`, `platform/hosted/paxeer/deployment.yaml:55-57`). After render, `paxeer_observer_render` appends an independent observer (`platform/hosted/tests/beta-cluster.sh:811-812`, `platform/hosted/tests/beta-cluster.sh:699-757`).

The observer init container `observer-genesis` uses its own home `/var/lib/paxeer-observer` (`platform/hosted/tests/beta-cluster.sh:710-714`). On first run it refuses a pre-existing observer `genesis.json`, runs `paxd init paxeer-observer` (the observer's own node id) with the primary CometBFT `chain_id`, copies the primary `genesis.json`, and records the primary Tendermint node id (`platform/hosted/tests/beta-cluster.sh:715-721`). It sets observer mode `full`, moves observer RPC/P2P to `127.0.0.1:26667` / `127.0.0.1:26666`, and sets `persistent-peers` to that primary node id at `127.0.0.1:26656` (`platform/hosted/tests/beta-cluster.sh:722-723`). EVM HTTP/WS and gRPC ports on the observer home are distinct (`platform/hosted/tests/beta-cluster.sh:724`). It validates observer genesis and then `cmp`s the two `genesis.json` files (`platform/hosted/tests/beta-cluster.sh:725-728`).

Container `paxd-observer` is a separate `paxd` process: `start --home /var/lib/paxeer-observer` (`platform/hosted/tests/beta-cluster.sh:736-738`). Claim template `observer-data` is a second PVC (`platform/hosted/tests/beta-cluster.sh:755-757`). `observer-boundary` listens on `0.0.0.0:9444` against `http://127.0.0.1:8555` (`platform/hosted/tests/beta-cluster.sh:744-751`). Service `paxeer-observer-boundary` targets that observer HTTPS port (`platform/hosted/tests/beta-cluster.sh:758-763`).

## TLS origins, CA bundle, signer key, `rpc-origins.json`

After the Paxeer pod is ready, bring-up port-forwards `paxeer-boundary` `19449:9443` and `paxeer-observer-boundary` `19452:9443` (`platform/hosted/tests/beta-cluster.sh:1247-1249`). The two TLS origins are `https://localhost:19449` and `https://localhost:19452` (`platform/hosted/tests/beta-cluster.sh:1244-1245`).

`paxeer_origins_write` converts the internal CA DER to PEM and writes `build/beta-cluster/paxeer/rpc-origins.json` (`platform/hosted/tests/beta-cluster.sh:55`, `platform/hosted/tests/beta-cluster.sh:773-781`). That document lists `rpc_origins` as those two URLs, `ca_bundle` as `build/beta-cluster/ca/ca.pem`, and `key_file` as `build/beta-cluster/secrets/paxeer-deployer.key` (`platform/hosted/tests/beta-cluster.sh:57-58`, `platform/hosted/tests/beta-cluster.sh:776-781`). Backends named in the same file are container `paxd` home `/var/lib/paxeer` and container `paxd-observer` home `/var/lib/paxeer-observer` (`platform/hosted/tests/beta-cluster.sh:778-780`).

## Builder environment and sealed upload

`LAYERX_BETA_BUILDER_ENVIRONMENT_DIR` is the owner hermetic builder root (regular files and directories only, entrypoint `bin/layerx-build`) published into the registry builder release PVC (`platform/hosted/tests/beta-cluster.sh:14-15`). Unset, it is recorded as a missing owner input and `builder_release_publish` returns without uploading (`platform/hosted/tests/beta-cluster.sh:645-647`). Set, it is refused unless it is a directory that contains `bin/layerx-build` (`platform/hosted/tests/beta-cluster.sh:649-650`).

The upload applies PVC `layerx-program-builder-release`, starts loader pod `layerx-program-builder-loader` waiting for `/opt/layerx-builder/.sealed`, copies the tree with `tar --mode=u+w`, then `chmod -R a-w` on the rootfs and `touch /opt/layerx-builder/.sealed` (`platform/hosted/tests/beta-cluster.sh:655-681`). The loader must reach `Succeeded` and is then deleted (`platform/hosted/tests/beta-cluster.sh:682-683`).

## Teardown

`beta-cluster.sh down` stops port-forwards, deletes the kind cluster named `CLUSTER_NAME` in kind mode, or deletes namespaces `layerx-testnet` and `layerx-developer` in owner mode (`platform/hosted/tests/beta-cluster.sh:6`, `platform/hosted/tests/beta-cluster.sh:1272-1283`). It removes labeled images, deletes `build/beta-cluster`, and unless `LAYERX_BETA_KEEP_TOOLS=1` removes the pinned kind/kubectl/calico downloads (`platform/hosted/tests/beta-cluster.sh:1285-1297`).

## Chain-identity triple

The disposable chain identity is the triple:

| Field | Value | Source |
| --- | --- | --- |
| CometBFT `chain_id` | `hyperpax_125-1` | `platform/hosted/paxeer/init-chain.sh:42` (`hyperpax_${CHAIN_ID}-1` with `CHAIN_ID` `125` at `platform/hosted/paxeer/init-chain.sh:16`) |
| Genesis document sha256 | 32 bytes at custody-profile offset `169:201` | `tests/bridge/custody_genesis.py:30` |
| EVM chain id | `125` | `platform/hosted/tests/beta-cluster.sh:96`, `platform/hosted/paxeer/init-chain.sh:16`, `platform/hosted/paxeer/deployment.yaml:41`, `tests/bridge/custody_genesis.py:24` |

`init-chain.sh` records `cosmos_chain_id` and `evm_chain_id` in the initialisation marker (`platform/hosted/paxeer/init-chain.sh:140-142`). For profile chain id `125`, `custody_genesis.py` requires the RPC `eth_chainId` to equal that id and the RPC genesis sha256 to equal the profile binding (`tests/bridge/custody_genesis.py:24-30`).

## Not covered

- Custody-first genesis is not performed. Bring-up waits for node genesis artifacts and then runs `deploy-contracts.sh bootstrap` (`platform/hosted/tests/beta-cluster.sh:1251-1252`). `deploy-contracts.sh` consumes LXGD/LXRR before prediction and deployment and does not expose a pre-genesis custody deploy followed by a signed native genesis that pins that vault, nor a post-genesis phase that preserves those vault and bond identities (`platform/hosted/paxeer/deploy-contracts.sh:19-28`).
- The `/genesis` boundary route is not present. The Paxeer boundary probes `/readyz` and `/livez` only (`platform/hosted/paxeer/deployment.yaml:45-46`).

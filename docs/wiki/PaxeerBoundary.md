# Paxeer boundary

`layerx-paxeer-boundary` is the TLS HTTP surface in front of the in-cluster Paxeer node (`platform/hosted/paxeer/Cargo.toml:2`, `platform/hosted/paxeer/Cargo.toml:8-10`, `platform/hosted/paxeer/src/main.rs:128-144`, `platform/hosted/paxeer/src/main.rs:477-493`). It is the only path the hosted manifests expose between other services and that chain: `paxd` binds EVM HTTP on loopback, and the StatefulSet Services expose only the boundary container port (`platform/hosted/paxeer/init-chain.sh:9-10`, `platform/hosted/paxeer/init-chain.sh:126-128`, `platform/hosted/paxeer/deployment.yaml:35-45`, `platform/hosted/paxeer/deployment.yaml:60-68`). The crate is `layerx-platform-paxeer-boundary`. The binary path is `src/main.rs`. The image is `ghcr.io/sidiora-labs/layerx-paxeer-boundary:0.1.0`, user `4020:4020`, entrypoint `/usr/local/bin/layerx-paxeer-boundary` (`platform/hosted/paxeer/Dockerfile:5-11`, `platform/hosted/paxeer/deployment.yaml:36`).

The beta cluster names that image `layerx-paxeer-boundary` and the Service `paxeer-boundary`, and describes the Service as the JSON-RPC relay to chain `125` (`platform/hosted/tests/beta-cluster.sh:89-91`, `platform/hosted/tests/beta-cluster.sh:1140-1141`). `layerx-testnet-control` sets `LAYERX_TESTNET_PAXEER_URL` to `https://paxeer-boundary.layerx-testnet.svc.cluster.local:9443` (`platform/hosted/testnet/deployment.yaml:91`). The node `paxeer-relay` container `socat`s loopback TCP to `OPENSSL:${LAYERX_NODE_PAXEER_BOUNDARY}:9443` with `LAYERX_NODE_PAXEER_BOUNDARY` `paxeer-boundary.layerx-testnet.svc.cluster.local` (`platform/hosted/node/deployment.yaml:78-90`). `deploy-contracts.sh` sends every `cast`/`forge` RPC through `LAYERX_PAXEER_BOUNDARY_URL` (`platform/hosted/paxeer/deploy-contracts.sh:2`, `platform/hosted/paxeer/deploy-contracts.sh:33`, `platform/hosted/paxeer/deploy-contracts.sh:108-119`, `platform/hosted/paxeer/deploy-contracts.sh:218-226`, `platform/hosted/paxeer/deploy-contracts.sh:265-270`).

`layerx-paxeer-client` is the custody-boundary client that talks JSON-RPC to that surface (`human/crates/layerx-paxeer-client/src/lib.rs:46-47`, `human/crates/layerx-paxeer-client/src/rpc.rs:165-186`).

---

## Chain identity

The process requires `LAYERX_PAXEER_CHAIN_ID` as a positive `u64` and stores it on `Config` (`platform/hosted/paxeer/src/main.rs:85-90`, `platform/hosted/paxeer/src/main.rs:121-126`, `platform/hosted/paxeer/src/main.rs:152-155`). Startup does not pin a specific chain id. The hosted manifest sets `LAYERX_PAXEER_CHAIN_ID` to `125` (`platform/hosted/paxeer/deployment.yaml:42`). `beta-cluster.sh` binds `PAXEER_CHAIN_ID=125` (`platform/hosted/tests/beta-cluster.sh:96`).

`GET /readyz` calls the loopback node `eth_chainId` and compares the hex quantity to that config value. A different id is `503` `chain_id_mismatch` (`platform/hosted/paxeer/src/main.rs:448-474`). `POST /` does not repeat that comparison (`platform/hosted/paxeer/src/main.rs:396-446`, `platform/hosted/paxeer/src/main.rs:490-491`).

`init-chain.sh` refuses any `LAYERX_PAXEER_CHAIN_ID` other than `125` and sets the CometBFT chain id to `hyperpax_125-1` (`platform/hosted/paxeer/init-chain.sh:16`, `platform/hosted/paxeer/init-chain.sh:39-42`). `deploy-contracts.sh` default `LAYERX_PAXEER_CHAIN_ID` is `125`; `require_endpoint` calls `cast chain-id --rpc-url` on the boundary and refuses a mismatch (`platform/hosted/paxeer/deploy-contracts.sh:35`, `platform/hosted/paxeer/deploy-contracts.sh:61`, `platform/hosted/paxeer/deploy-contracts.sh:108-119`). Profile `immediate-beta` additionally requires chain `125` (`platform/hosted/paxeer/deploy-contracts.sh:147-149`). `PaxeerBetaDeploymentValidator` constant `PAXEER_EVM_CHAIN_ID` is `125`; `block.chainid` other than `125` reverts `WrongPaxeerChain` (`contracts/deployment/PaxeerBetaDeploymentValidator.sol:9`, `contracts/deployment/PaxeerBetaDeploymentValidator.sol:130`).

`layerx-paxeer-client` `raw_call` issues `eth_chainId` first and returns `EndpointFault::ChainMismatch` when the quantity is not `expected_chain_id` (`human/crates/layerx-paxeer-client/src/rpc.rs:170-197`). `expected_chain_id` `0` is `UnexpectedValue` (`human/crates/layerx-paxeer-client/src/rpc.rs:82-86`). The hosted manifest, `init-chain.sh`, `deploy-contracts.sh`, and `PaxeerBetaDeploymentValidator` pin `125` (`platform/hosted/paxeer/deployment.yaml:42`, `platform/hosted/paxeer/init-chain.sh:39-42`, `platform/hosted/paxeer/deploy-contracts.sh:108-119`, `contracts/deployment/PaxeerBetaDeploymentValidator.sol:9`, `contracts/deployment/PaxeerBetaDeploymentValidator.sol:130`). The boundary binary itself accepts any positive integer at process start (`platform/hosted/paxeer/src/main.rs:152-155`).

`GET /genesis`, when `LAYERX_PAXEER_COMET_URL` is set, returns the Comet genesis JSON bytes and sets `X-LayerX-Genesis-SHA256` to the SHA-256 of those bytes (`platform/hosted/paxeer/src/main.rs:164-172`, `platform/hosted/paxeer/src/main.rs:484-488`, `platform/hosted/paxeer/src/genesis.rs:241-256`, `platform/hosted/paxeer/src/main.rs:535-540`). The body must decode as JSON with a non-empty `chain_id` string (`platform/hosted/paxeer/src/genesis.rs:230-237`). `real_chain.py` asserts that genesis `chain_id` equals `hyperpax_125-1` and that EVM `eth_chainId` is `125` (`platform/hosted/paxeer/tests/real_chain.py:99-105`).

---

## Routes

TLS HTTP/1.1 only. Query strings fail the request line (`platform/hosted/paxeer/src/main.rs:318-320`). Responses are `Content-Type: application/json`, `Cache-Control: no-store`, `Connection: close` (`platform/hosted/paxeer/src/main.rs:541-546`).

| Method | Path | Result |
| --- | --- | --- |
| `GET` | `/livez` | `200` `{"status":"live","service":"paxeer-boundary"}` (`platform/hosted/paxeer/src/main.rs:478-479`) |
| `GET` | `/readyz` | readiness document below (`platform/hosted/paxeer/src/main.rs:481-482`, `platform/hosted/paxeer/src/main.rs:466-474`) |
| `GET` | `/genesis` | Comet genesis JSON when `LAYERX_PAXEER_COMET_URL` is set; otherwise `404` `not_found` (`platform/hosted/paxeer/src/main.rs:164-166`, `platform/hosted/paxeer/src/main.rs:484-488`) |
| `POST` | `/` | JSON-RPC relay to `LAYERX_PAXEER_NODE_URL` (`platform/hosted/paxeer/src/main.rs:490-491`, `platform/hosted/paxeer/src/main.rs:396-445`) |

Any other method or path is `404` `not_found` (`platform/hosted/paxeer/src/main.rs:493`). `POST /genesis`, `GET /genesis/`, and `GET /genesis_chunked` are `404` (`platform/hosted/paxeer/tests/genesis.py:190-192`).

`POST /` requires `Content-Type: application/json` (`platform/hosted/paxeer/src/main.rs:397-398`). The body must be a JSON object, not an array (`platform/hosted/paxeer/src/main.rs:403-407`). Members other than `jsonrpc`, `id`, `method`, `params` fail as JSON-RPC `-32600` `invalid request`, as does `jsonrpc` other than `"2.0"`, a non-null/number/string `id`, or `params` that is neither array nor object (`platform/hosted/paxeer/src/main.rs:363-368`, `platform/hosted/paxeer/src/main.rs:409-419`). Relayed methods are `net_version`, `web3_clientVersion`, and names that start with `eth_`, except the denied set `eth_accounts`, `eth_coinbase`, `eth_sendTransaction`, `eth_sign`, `eth_signTransaction`, `eth_signTypedData`, `eth_signTypedData_v4`, `eth_mining` (`platform/hosted/paxeer/src/main.rs:28-39`, `platform/hosted/paxeer/src/main.rs:371-384`). A denied or unknown method is JSON-RPC `-32601` `method is not relayed by the boundary` with HTTP `200` (`platform/hosted/paxeer/src/main.rs:424-425`, `platform/hosted/paxeer/src/main.rs:386-393`). Method names longer than 64 bytes or containing bytes other than ASCII alphanumeric and `_` are also `-32601` (`platform/hosted/paxeer/src/main.rs:21`, `platform/hosted/paxeer/src/main.rs:371-378`). `eth_sendRawTransaction` starts with `eth_` and is not in the denied set (`platform/hosted/paxeer/src/main.rs:28-39`, `platform/hosted/paxeer/tests/real_chain.py:145-147`).

A successful node reply must be HTTP `200` with a JSON object `jsonrpc` `"2.0"`, matching `id`, and exactly one of `result` or `error` (`platform/hosted/paxeer/src/main.rs:427-440`). Request body bound is 2 MiB (`platform/hosted/paxeer/src/main.rs:17`, `platform/hosted/paxeer/src/main.rs:400-401`). Node response bound is 8 MiB (`platform/hosted/paxeer/src/main.rs:20`, `platform/hosted/paxeer/src/main.rs:348`). At most 128 connections are live; further accepts are dropped with no HTTP response (`platform/hosted/paxeer/src/main.rs:25-26`, `platform/hosted/paxeer/src/main.rs:597-601`).

`GET /genesis` fetches Comet `GET /genesis`. The Comet error `code` `-32603`, `message` `Internal error`, `data` `genesis response is large, please use the genesis_chunked API instead` switches to `GET /genesis_chunked?chunk=N` (`platform/hosted/paxeer/src/genesis.rs:15`, `platform/hosted/paxeer/src/genesis.rs:213-226`). Chunked genesis is at most 32 chunks and 32 MiB (`platform/hosted/paxeer/src/genesis.rs:12-14`, `platform/hosted/paxeer/src/genesis.rs:182-210`).

---

## Credentials

Inbound TLS is rustls with no client authentication. The certificate is `LAYERX_PAXEER_BOUNDARY_TLS_CERT_DER`; the PKCS#8 key is `LAYERX_PAXEER_BOUNDARY_TLS_KEY_DER` (`platform/hosted/paxeer/src/main.rs:128-144`). The StatefulSet mounts Secret `paxeer-boundary-tls` at `/run/layerx/tls` (`platform/hosted/paxeer/deployment.yaml:43-44`, `platform/hosted/paxeer/deployment.yaml:50-53`). `beta-cluster.sh` issues that certificate for `paxeer-boundary` with `serverAuth` (`platform/hosted/tests/beta-cluster.sh:399-400`, `platform/hosted/tests/beta-cluster.sh:627`). The binary has no bearer, API-key, or client-certificate check (`platform/hosted/paxeer/src/main.rs:140-142`, `platform/hosted/paxeer/src/main.rs:477-493`).

`layerx-paxeer-client` production transport is `EndpointTransport::PinnedTls` with an explicit trust-anchor DER; plaintext is `LocalEmulator` and only on loopback (`human/crates/layerx-paxeer-client/src/rpc.rs:18-22`, `human/crates/layerx-paxeer-client/src/rpc.rs:75-81`). Mixed TLS and emulator endpoints are `ClientConfigError::MixedTransportModes` (`human/crates/layerx-paxeer-client/src/client.rs:245-250`). The node relay verifies the boundary server certificate against `/run/layerx/trust/ca.crt` and `commonname=${LAYERX_NODE_PAXEER_BOUNDARY}` (`platform/hosted/node/deployment.yaml:86-87`). `deploy-contracts.sh` requires an `https://` `LAYERX_PAXEER_BOUNDARY_URL` and a readable `LAYERX_PAXEER_BOUNDARY_CA_DER`, converts that DER to PEM, and sets `SSL_CERT_FILE` (`platform/hosted/paxeer/deploy-contracts.sh:108-116`).

`LAYERX_PAXEER_DEPLOYER_KEY_FILE` and `LAYERX_PAXEER_GUARANTOR_KEYS_DIR` are names of files consumed by `deploy-contracts.sh` for on-chain sends; they are not inputs to the boundary binary (`platform/hosted/paxeer/deploy-contracts.sh:36`, `platform/hosted/paxeer/deploy-contracts.sh:42-43`, `platform/hosted/paxeer/deploy-contracts.sh:122-130`).

---

## Typed refusals

HTTP envelope `{ "error": { "code": <name>, "retry": "never"|"after", ... } }` (`platform/hosted/paxeer/src/main.rs:505-520`):

| Status | Code | When |
| --- | --- | --- |
| `400` | `invalid_request` | Request line, Host, or framing fails (`platform/hosted/paxeer/src/main.rs:303-324`, `platform/hosted/paxeer/src/main.rs:561-563`) |
| `400` | `content_type_required` | `POST /` without `Content-Type: application/json` (`platform/hosted/paxeer/src/main.rs:397-398`) |
| `400` | `invalid_json` | Body is not JSON (`platform/hosted/paxeer/src/main.rs:403-404`) |
| `400` | `batch_not_supported` | Body is not a JSON object (`platform/hosted/paxeer/src/main.rs:406-407`) |
| `413` | `body_too_large` | Body exceeds 2 MiB (`platform/hosted/paxeer/src/main.rs:400-401`) |
| `404` | `not_found` | Unknown route, or `GET /genesis` with Comet unset (`platform/hosted/paxeer/src/main.rs:484-488`, `platform/hosted/paxeer/src/main.rs:493`) |
| `502` | `node_response_invalid` | Node HTTP or JSON-RPC envelope is not a matching single reply; `retry_after_seconds` `5` (`platform/hosted/paxeer/src/main.rs:441-443`) |
| `503` | `node_unavailable` | Node TCP connect or I/O fails; `retry_after_seconds` `5` (`platform/hosted/paxeer/src/main.rs:444`, `platform/hosted/paxeer/src/main.rs:472`) |
| `503` | `chain_id_mismatch` | `/readyz` `eth_chainId` ≠ `LAYERX_PAXEER_CHAIN_ID`; `retry_after_seconds` `10` (`platform/hosted/paxeer/src/main.rs:471`) |
| `502` | `comet_response_invalid` | Genesis fetch body or framing is invalid; `retry_after_seconds` `5` (`platform/hosted/paxeer/src/genesis.rs:260`) |
| `503` | `comet_unavailable` | Comet TCP connect or I/O fails; `retry_after_seconds` `5` (`platform/hosted/paxeer/src/genesis.rs:259`) |

JSON-RPC errors on HTTP `200` for `POST /`: `-32600` `invalid request`, `-32601` `method is not relayed by the boundary` (`platform/hosted/paxeer/src/main.rs:419-425`). Config or listen failure prints to stderr and exits `2` (`platform/hosted/paxeer/src/main.rs:617-620`). `LAYERX_PAXEER_COMET_URL` that is not loopback `http://` with path `/` exits `2` (`platform/hosted/paxeer/src/genesis.rs:17-23`, `platform/hosted/paxeer/tests/genesis.py:273-282`).

---

## Readiness

`GET /livez` does not contact the node (`platform/hosted/paxeer/src/main.rs:478-479`). `GET /readyz` is ready only when `eth_chainId` equals `LAYERX_PAXEER_CHAIN_ID`; the `200` body is `{"status":"ready","service":"paxeer-boundary","chain_id":<id>}` (`platform/hosted/paxeer/src/main.rs:466-470`). The StatefulSet HTTPS probes are `/readyz` period `5` and `/livez` period `10` (`platform/hosted/paxeer/deployment.yaml:46-47`). After `paxd` exit, `real_chain.py` observes `/readyz` `503` and `/livez` `200` (`platform/hosted/paxeer/tests/real_chain.py:204-207`). A process started with `LAYERX_PAXEER_CHAIN_ID` `126` against chain `125` serves `/readyz` `503` `chain_id_mismatch` (`platform/hosted/paxeer/tests/real_chain.py:189-202`).

`layerx-paxeer-client` `BoundaryHealth` is derived from a `FinalityReport`, not from `/readyz` (`human/crates/layerx-paxeer-client/src/status.rs:38-52`, `human/crates/layerx-paxeer-client/src/status.rs:54-107`).

---

## Configuration keys

Boundary process (`platform/hosted/paxeer/src/main.rs:147-173`):

| Variable | Role |
| --- | --- |
| `LAYERX_PAXEER_BOUNDARY_LISTEN` | TLS bind; default `0.0.0.0:9443` (`platform/hosted/paxeer/src/main.rs:148-151`) |
| `LAYERX_PAXEER_CHAIN_ID` | Required positive `u64`; compared on `/readyz` (`platform/hosted/paxeer/src/main.rs:152-155`, `platform/hosted/paxeer/src/main.rs:466-471`) |
| `LAYERX_PAXEER_NODE_URL` | Required loopback `http://` literal IP and port (`platform/hosted/paxeer/src/main.rs:48-81`, `platform/hosted/paxeer/src/main.rs:159-162`) |
| `LAYERX_PAXEER_COMET_URL` | Optional; same loopback HTTP origin with path `/` (`platform/hosted/paxeer/src/main.rs:164-172`, `platform/hosted/paxeer/src/genesis.rs:17-23`) |
| `LAYERX_PAXEER_BOUNDARY_TLS_CERT_DER` | Server certificate DER (`platform/hosted/paxeer/src/main.rs:132-136`) |
| `LAYERX_PAXEER_BOUNDARY_TLS_KEY_DER` | PKCS#8 key DER (`platform/hosted/paxeer/src/main.rs:134-139`) |

Hosted manifest values: listen `0.0.0.0:9443`, node `http://127.0.0.1:8545`, Comet `http://127.0.0.1:26657`, chain `125` (`platform/hosted/paxeer/deployment.yaml:39-42`).

`deploy-contracts.sh` additionally reads `LAYERX_PAXEER_BOUNDARY_URL`, `LAYERX_PAXEER_BOUNDARY_CA_DER`, `LAYERX_PAXEER_DEPLOYER_KEY_FILE`, `LAYERX_PAXEER_GENESIS_DIR`, `LAYERX_PAXEER_DEPLOYMENT_INPUT`, `LAYERX_PAXEER_GUARANTORS`, `LAYERX_PAXEER_GUARANTOR_KEYS_DIR`, `LAYERX_PAXEER_DEPLOYMENT_RECORD`, `LAYERX_PAXEER_SETTLEMENT_JSON`, `LAYERX_PAXEER_SETTLEMENT_DOMAIN`, `LAYERX_PAXEER_FOUNDRY_BIN`, `LAYERX_PAXEER_CONTROLLER_GAS_WEI` (`platform/hosted/paxeer/deploy-contracts.sh:32-47`, `platform/hosted/paxeer/deploy-contracts.sh:53-71`). `init-chain.sh` reads `LAYERX_PAXEER_HOME`, `LAYERX_PAXEER_CHAIN_ID`, `LAYERX_PAXEER_MONIKER`, `LAYERX_PAXEER_VALIDATOR_KEY_NAME`, `LAYERX_PAXEER_VALIDATOR_FUNDING`, `LAYERX_PAXEER_VALIDATOR_STAKE`, `LAYERX_PAXEER_VALIDATOR_POWER`, `LAYERX_PAXEER_DEPLOYER_FUNDING`, `LAYERX_PAXEER_DEPLOYER_ADDRESS_FILE`, `LAYERX_PAXEER_DEPLOYER_ADDRESS`, `LAYERX_PAXEER_USDL_RUNTIME`, `LAYERX_PAXEER_EVM_PORT`, `LAYERX_PAXEER_EVM_WS_PORT`, `LAYERX_PAXEER_RPC_PORT`, `LAYERX_PAXEER_P2P_PORT`, `LAYERX_PAXEER_GRPC_PORT`, `LAYERX_PAXEER_GRPC_WEB_PORT` (`platform/hosted/paxeer/init-chain.sh:15-31`, `platform/hosted/paxeer/init-chain.sh:44-48`).

Client tests may set `LAYERX_PAXEER_TEST_CLIENT_URL` and `LAYERX_PAXEER_TEST_CLIENT_CA` (`platform/hosted/paxeer/tests/real_chain.rs:7-17`).

---

## Egress and admitted edges

NetworkPolicy `paxeer-boundary` selects `app: paxeer` with `Ingress` and `Egress` (`platform/hosted/paxeer/deployment.yaml:70-75`). Ingress TCP `9443` is admitted from `app: layerx-testnet-control`, `app: layerx-node`, and any-namespace `layerx-plane: human` (`platform/hosted/paxeer/deployment.yaml:76-82`). No workload in the default topology manifests carries `layerx-plane: human` (`platform/hosted/paxeer/deployment.yaml:81`; the label appears only on that peer selector). Egress is kube-dns in `kube-system` UDP/TCP `53` (`platform/hosted/paxeer/deployment.yaml:83-87`). `LAYERX_PAXEER_NODE_URL` and `LAYERX_PAXEER_COMET_URL` are loopback (`platform/hosted/paxeer/deployment.yaml:40-41`).

`layerx-testnet-control` egress admits `layerx-plane: trusted-boundary` TCP `9443`/`9444`/`9445` plus DNS (`platform/hosted/testnet/deployment.yaml:266-284`). The Paxeer pod label set includes `layerx-plane: trusted-boundary` (`platform/hosted/paxeer/deployment.yaml:9`). `layerx-node` egress admits `app: paxeer` TCP `9443` plus DNS (`platform/hosted/node/deployment.yaml:286-296`).

`platform-hosted-topology-check` loads `platform/hosted/paxeer/deployment.yaml` among its default manifests and resolves only `http(s)|redis(s)` URLs to Services (`platform/hosted/tests/topology-check.sh:17-26`, `platform/hosted/tests/topology-check.sh:81-92`, `platform/hosted/tests/topology-check.sh:117`, `platform/hosted/tests/topology-check.sh:588-606`). `LAYERX_NODE_PAXEER_BOUNDARY` is a hostname, not a URL (`platform/hosted/node/deployment.yaml:90`). Loopback `http://127.0.0.1:8545` and `http://127.0.0.1:26657` are classified `offcluster` (`platform/hosted/tests/topology-check.sh:594-597`, `platform/hosted/paxeer/deployment.yaml:40-41`).

---

## Settlement suite

`deploy-contracts.sh` deploys the LayerX settlement suite through the boundary using `scripts/PaxeerBetaDeploy.s.sol:PaxeerBetaDeploy` (`platform/hosted/paxeer/deploy-contracts.sh:2-7`, `platform/hosted/paxeer/deploy-contracts.sh:265-270`). Phases: `deploy`, `permissions`, `activate`, `bond`, `finalize`; `bootstrap` runs those five in order and requires `timelock_profile` `immediate-beta` (`platform/hosted/paxeer/deploy-contracts.sh:9-17`, `platform/hosted/paxeer/deploy-contracts.sh:417-425`). Recorded addresses: `blueprint`, `timelock`, `asset_registry`, `vault`, `guarantor_bond`, `checkpoint_registry`, `challenge_manager`, `nullifier_registry`, `withdrawal_claims`, `emergency_exit`, `reserve_reconciler`, `manager_container`, `manager_migrator`, `custody_topology` (`platform/hosted/paxeer/deploy-contracts.sh:342-347`, `scripts/PaxeerBetaDeploy.s.sol:39-54`). The Solidity runner imports `Blueprint`, `LayerXTimelock` / `LayerXBetaTimelock`, `AssetRegistry`, `LayerXVault`, `GuarantorBond`, `CheckpointRegistry`, `CheckpointChallengeManager`, `WithdrawalNullifierRegistry`, `WithdrawalClaims`, `EmergencyExit`, `ReserveReconciler`, `ManagerContainer`, `ManagerMigrator`, and custody topology (`scripts/PaxeerBetaDeploy.s.sol:4-23`).

USDL is the 6-decimal token at `0x85FcD13735F4309833A503EE804ea32395851479` (`contracts/libraries/Constants.sol:14-16`, `platform/hosted/paxeer/init-chain.sh:23`, `platform/hosted/paxeer/deploy-contracts.sh:70`, `platform/hosted/paxeer/deploy-contracts.sh:301`). `init-chain.sh` seeds that address in genesis with `BetaUsdl` runtime and the deployer as slot-0 owner (`platform/hosted/paxeer/init-chain.sh:99-106`, `platform/hosted/paxeer/contracts/BetaUsdl.sol:5-9`). `GuarantorBond` requires that USDL token; the runner comment states a WETH custody deployment is not a replacement suite (`platform/hosted/paxeer/deploy-contracts.sh:29-30`).

`PaxeerBetaDeploymentValidator.decodeAndCrossCheckGenesis` requires LXGD v1 105 bytes and LXRR v1 73 bytes, matching network id, and distinct non-zero manifest / canonical / receipt roots (`contracts/deployment/PaxeerBetaDeploymentValidator.sol:10-11`, `contracts/deployment/PaxeerBetaDeploymentValidator.sol:64-94`). `_validateInput` requires protocol `Constants.PROTOCOL_VERSION` or `3`, chain id `125`, live USDL with decimals `6`, a parseable release, distinct non-zero bootstrap/proposer/executor/council, economic bounds, and (unless `immediateBeta`) `timelockDelay >= 1 days`; `immediateBeta` requires `timelockDelay == 0` (`contracts/deployment/PaxeerBetaDeploymentValidator.sol:121-148`, `contracts/libraries/Constants.sol:5`). `validateGuarantors` requires a non-empty set, strictly increasing ids, unique non-zero signers and controllers, and USDL balance/allowance covering each `bondAmount` (`contracts/deployment/PaxeerBetaDeploymentValidator.sol:180-203`). Errors: `InvalidBetaDeploymentInput`, `WrongPaxeerChain`, `InvalidUsdl`, `InvalidGenesisArtifacts`, `InvalidGuarantor`, `UnfundedGuarantor` (`contracts/deployment/PaxeerBetaDeploymentValidator.sol:13-18`).

`Constants.PROTOCOL_VERSION` is `2` (`contracts/libraries/Constants.sol:5`). `prepare-beta.py` writes `protocol_version` `3` (`platform/hosted/paxeer/prepare-beta.py:12`, `platform/hosted/paxeer/prepare-beta.py:81-82`). `beta-cluster.sh` overlays `protocol_version: 3` onto `deployment-input.beta.json` and does not invoke `prepare-beta.py` (`platform/hosted/tests/beta-cluster.sh:925-928`). `deploy-contracts.sh` accepts protocol `2` or `3` (`platform/hosted/paxeer/deploy-contracts.sh:174-176`). Those protocol pins differ: the library default is `2`; the beta overlay is `3`.

`deployment-input.beta.json` sets `timelock_profile` `immediate-beta` and `timelock_delay` `0` (`platform/hosted/paxeer/deployment-input.beta.json:6`, `platform/hosted/paxeer/deployment-input.beta.json:26`). Cluster bring-up runs `deploy-contracts.sh bootstrap` through the boundary and publishes `GuarantorBond` and `CheckpointRegistry` as ConfigMap `layerx-node-settlement` (`platform/hosted/tests/beta-cluster.sh:41-47`, `platform/hosted/tests/beta-cluster.sh:913-957`).

`deploy-contracts.sh` consumes LXGD/LXRR before prediction and deployment. It does not expose a pre-genesis custody deploy followed by a signed native genesis that pins that vault, nor a post-genesis phase that preserves those vault and bond identities (`platform/hosted/paxeer/deploy-contracts.sh:19-28`).

---

## In-cluster chain

StatefulSet `paxeer` in `layerx-testnet` has one replica, init container `genesis` (`bash /opt/layerx/init-chain.sh`, home `/var/lib/paxeer`), container `paxd` (`start --home /var/lib/paxeer`), and container `boundary` (`platform/hosted/paxeer/deployment.yaml:2-45`). `paxd` image `ghcr.io/sidiora-labs/paxd:0.1.0` is built from `Dockerfile.paxd` on `paxd-node` and copies `init-chain.sh` and `BetaUsdl.runtime.hex` (`platform/hosted/paxeer/Dockerfile.paxd:1-8`, `platform/hosted/paxeer/Dockerfile.paxd-node:29-41`). PVC `data` is 40Gi at `/var/lib/paxeer` (`platform/hosted/paxeer/deployment.yaml:56-58`).

`init-chain.sh` comment states Tendermint, gRPC, and API listeners bind on loopback (`platform/hosted/paxeer/init-chain.sh:8-10`). The script sets Tendermint RPC/P2P and gRPC to `127.0.0.1`, sets `api` `enable = false`, and sets `grpc-web` `enable = false` (`platform/hosted/paxeer/init-chain.sh:117-125`). Those two statements differ on whether the API listener is bound. EVM HTTP is `127.0.0.1:${LAYERX_PAXEER_EVM_PORT}` with `http_enabled = true` and `enable_test_api = false` (`platform/hosted/paxeer/init-chain.sh:126-131`). A completed home is marked and skipped; a partial `genesis.json` without the marker is refused (`platform/hosted/paxeer/init-chain.sh:59-65`).

`beta-cluster.sh` `paxeer_observer_render` appends container `paxd-observer` and `observer-boundary` listen `0.0.0.0:9444` against `http://127.0.0.1:8555` with `LAYERX_PAXEER_COMET_URL` `http://127.0.0.1:26667`, and Service `paxeer-observer-boundary` (`platform/hosted/tests/beta-cluster.sh:713-778`). That observer is not in `platform/hosted/paxeer/deployment.yaml`.

---

## Client contract

`PaxeerClient` takes one or more `EndpointConfig` values that share `expected_chain_id` and transport mode (`human/crates/layerx-paxeer-client/src/client.rs:210-269`). Production calls are `POST` JSON-RPC over TLS with the pinned trust anchor (`human/crates/layerx-paxeer-client/src/rpc.rs:329-336`, `human/crates/layerx-paxeer-client/src/rpc.rs:297-314`). `raw_call` always verifies `eth_chainId` before any other method (`human/crates/layerx-paxeer-client/src/rpc.rs:175-181`). Client HTTP faults include `Http { status }`, `Rpc { code, message }`, `ChainMismatch`, `InsecureTransport`, `InvalidTrustAnchor` (`human/crates/layerx-paxeer-client/src/rpc.rs:36-51`). Client response body bound is 2 MiB (`human/crates/layerx-paxeer-client/src/rpc.rs:12`, `human/crates/layerx-paxeer-client/src/rpc.rs:417-419`). The client does not filter JSON-RPC method names; the boundary does (`human/crates/layerx-paxeer-client/src/rpc.rs:170-181`, `platform/hosted/paxeer/src/main.rs:371-384`).

---

## Tests

`make platform-test-paxeer-boundary` runs `cargo test --offline --manifest-path platform/Cargo.toml --locked -p layerx-platform-paxeer-boundary` (`platform/Makefile.inc:156-157`). `platform-test-trusted-boundary` depends on that target (`platform/Makefile.inc:159`). `platform-test-tooling` runs `sh -n` on `init-chain.sh`, `bash -n` on `deploy-contracts.sh` and `beta-cluster.sh` / `topology-check.sh`, and `python3 -m py_compile` on `settlement-domain.py` and `prepare-beta.py` (`platform/Makefile.inc:120-129`).

`tests` in `main.rs` assert chunked node framing is bounded and rejects `Transfer-Encoding` plus `Content-Length` (`platform/hosted/paxeer/src/main.rs:628-649`). `tests/genesis.rs` runs `tests/genesis.py` against the real binary (`platform/hosted/paxeer/tests/genesis.rs:4-10`). That script proves verbatim and HTTP-chunked `/genesis`, ordered Comet chunk reassembly, `X-LayerX-Genesis-SHA256` equal to SHA-256 of the body, `404` `not_found` when Comet is unset, JSON-RPC `-32601` for denied methods including `eth_sendTransaction` and `eth_sign`, `502` `comet_response_invalid` without falling through to chunked genesis, chunk metadata/base64 refusals, 32 MiB bound, deadline, `503` `comet_unavailable`, and startup exit `2` for non-loopback Comet URLs (`platform/hosted/paxeer/tests/genesis.py:171-282`).

`tests/real_chain.rs` `real_paxd_boundary` either calls `raw_call` `eth_chainId` against `LAYERX_PAXEER_TEST_CLIENT_URL` expecting `0x7d`, or runs `tests/real_chain.py` (`platform/hosted/paxeer/tests/real_chain.rs:6-33`). `real_chain.py` initialises chain `125` with `init-chain.sh`, starts `paxd` and the boundary, waits for `/readyz` `200`, relays `eth_chainId` `0x7d` with ids `1` / `"preserved"` / `null`, compares `/genesis` to local `genesis.json`, preserves RPC errors, accepts a 2 MiB-padded `eth_chainId` and refuses one extra byte with `413`, `eth_call`s USDL `decimals()` `6` and `owner()`, asserts the EVM socket is loopback, sends `eth_sendRawTransaction` to inclusion `0x1`, optionally runs `deploy-contracts.sh`, then proves `chain_id_mismatch` and node-loss readiness (`platform/hosted/paxeer/tests/real_chain.py:42-208`). `settlement_domains` runs `tests/settlement_domains.py` (`platform/hosted/paxeer/tests/real_chain.rs:36-45`), which proves the domain writer keeps `vectors`, refuses rewriting `vectors`, binds protocol `3` headers distinctly from protocol `2`, and refuses protocol `4` (`platform/hosted/paxeer/tests/settlement_domains.py:15-55`). `tests/deployment_profile.py` proves `immediate-beta` is explicit, `standard` is the default, nonzero immediate delay is refused, unknown profiles are refused, an existing standard record cannot be reinterpreted, and immediate-beta refuses chain `1` (`platform/hosted/paxeer/tests/deployment_profile.py:32-65`).

`test/PaxeerBetaDeploymentValidator.t.sol` proves LXGD/LXRR cross-check, `WrongPaxeerChain` on `126`, `InvalidUsdl` on 18-decimal code, protocol-2 config, protocol-3 Blueprint/ManagerContainer binding, authority and economic input refusals, explicit immediate-beta delay `0`, sorted unique funded guarantors, empty guarantor refusal, and runner `InvalidDeploymentState` before stateful phases (`test/PaxeerBetaDeploymentValidator.t.sol:105-241`).

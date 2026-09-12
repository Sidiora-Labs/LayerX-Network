# Testnet quickstart

At the end of this path a developer has a disposable beta cluster from this
repository, the env file `up` writes, a local Ed25519 key and a stored hosted
session token, a faucet claim against that cluster, one Programs deploy
submitted through the CLI command that exists (and the hosted
`POST /v1/programs/deploy` route the gateway actually serves), one payment
submitted through the CLI command that exists (and the hosted activity HTTP
route the gateway actually serves), fetched receipt bytes, a local `layerx
receipt verify` result, and a Paxeer-boundary observation of chain id `125`.

This page covers that path. Commands, flags, printed fields, and HTTP routes are
cited to the tree. Related pages: [CLI](Cli.md), [Beta cluster](BetaCluster.md),
[Hosted gateway](HostedGateway.md), [Hosted identity](HostedIdentity.md),
[Programs](Programs.md), [Finality](Finality.md).

Faucet, send, Asset, program, and 402 surfaces are on
[Payments developer path](PaymentsQuickstart.md). The public endpoint
checklist is [Getting started on testnet](Getting-Started-Testnet.md).
Asset encodings: [Assets](Assets.md). Public `POST /rpc`: [Public JSON-RPC](PublicRpc.md).
`executed` / `batched` / `finalised`:
[Commitment levels](CommitmentLevels.md).

---

## 1. Bring the beta cluster up

The script is `platform/hosted/tests/beta-cluster.sh`. Its usage names `up`,
`down`, and `render` (`platform/hosted/tests/beta-cluster.sh:4-7`). `main`
dispatches `up`, `down`, `render`, and `images`; any other first argument
prints that usage block to stderr and exits `64`
(`platform/hosted/tests/beta-cluster.sh:1341-1362`). There is no `export`
subcommand.

Make:

```sh
make platform-beta-cluster-up
```

That runs `bash platform/hosted/tests/beta-cluster.sh up $(PLATFORM_BETA_CLUSTER_FLAGS)`
(`platform/Makefile.inc:184-185`). The only `up` flag the script accepts is
`--boundary-checks` (`platform/hosted/tests/beta-cluster.sh:1351-1358`).

`up` calls `beta_cluster_up` (`platform/hosted/tests/beta-cluster.sh:1236-1290,
1358`). After images, cluster, CA, secrets, render, trusted-boundary apply,
Paxeer contract deploy, identity provisioning, and port-forwards, it writes
the env file, writes cluster identity, and waits until testnet `GET /readyz`
has `state == "ready"`, every journey `ready == true`, every dependency
`ready == true`, four journeys, and the developer-plane deployments are
ready (`platform/hosted/tests/beta-cluster.sh:1065-1076, 1284-1287`). On
success it prints, on stderr via `log`:

- `every journey ready:` followed by the comma-joined `.journeys[].journey`
  values from `build/beta-cluster/readyz.json`
  (`platform/hosted/tests/beta-cluster.sh:102, 1287`)
- `environment exported to` plus `build/beta-cluster/env`
  (`platform/hosted/tests/beta-cluster.sh:55, 61, 1288`)

`identity_write` prints `beta-cluster: cluster identity` and then each
identity line prefixed `beta-cluster:   `
(`platform/hosted/tests/beta-cluster.sh:1177-1178`). Those lines include
`cluster_mode`, `cluster_name`, `revision`, `sequencer_public_key`,
`sequencer_id`, `node_network_id`, `node_asset_id`, `paxeer_chain_id`,
`paxeer_guarantor_bond`, `paxeer_checkpoint_registry`, `test_source_did`,
`test_destination_did`, `faucet_host`, `developer_host`
(`platform/hosted/tests/beta-cluster.sh:1149-1172`).

Render without applying:

```sh
bash platform/hosted/tests/beta-cluster.sh render
```

prints `rendered manifests under` `$MANIFESTS_DIR` `and beta CA under`
`$CA_DIR` `(nothing applied)` (`platform/hosted/tests/beta-cluster.sh:1320-1338`).

Default host ports are testnet `19443`, gateway `19444`, faucet `19445`
(`platform/hosted/tests/beta-cluster.sh:81-83`). `up` binds
`TESTNET_URL=https://localhost:$TESTNET_PORT`,
`GATEWAY_URL=https://localhost:$GATEWAY_PORT`,
`FAUCET_URL=https://localhost:$FAUCET_PORT`,
`DEVELOPER_URL=https://localhost:19450`,
`NODE_URL=https://localhost:19446`,
`AGENT_URL=https://localhost:19447`,
`PAXEER_URL=https://localhost:19449`,
`PAXEER_OBSERVER_URL=https://localhost:19452`,
`IDENTITY_URL=https://localhost:19451`,
`HUMAN_URL=https://localhost:19453`
(`platform/hosted/tests/beta-cluster.sh:1258-1266`). Port-forwards are
Paxeer-boundary `19449`, Paxeer-observer-boundary `19452`, identity
`19451`, testnet, gateway, faucet, developer `19450`, pending-core
`19446`, and agent-boundary `19447`
(`platform/hosted/tests/beta-cluster.sh:1268-1283`). Human is forwarded on `19453` to port `9443`. There is no agentd port-forward.

The kind cluster name defaults to `layerx-beta`
(`platform/hosted/tests/beta-cluster.sh:11, 73`). Paxeer EVM chain id is
`125` (`platform/hosted/tests/beta-cluster.sh:96`).

---

## 2. Exported endpoints

`up` writes `build/beta-cluster/env` from `env_write`
(`platform/hosted/tests/beta-cluster.sh:61, 1110-1142, 1284`). There is no
separate export step. Source it:

```sh
. build/beta-cluster/env
```

`make platform-hosted-smoke` sources the same file when readable
(`platform/Makefile.inc:3, 161-173`). The file contains `export` lines for:

| Variable | Value written |
| --- | --- |
| `LAYERX_TESTNET_URL` | `$TESTNET_URL` |
| `LAYERX_GATEWAY_URL` | `$GATEWAY_URL` |
| `LAYERX_FAUCET_URL` | `$FAUCET_URL` |
| `LAYERX_TEST_AUTH_TOKEN_FILE` | `build/beta-cluster/secrets/test-auth.token` |
| `LAYERX_TEST_CA_FILE` | `build/beta-cluster/ca/ca.crt` |
| `LAYERX_TEST_SOURCE_DID` | smoke source DID |
| `LAYERX_TEST_SOURCE_PUBLIC_KEY` | 64-hex Ed25519 public key |
| `LAYERX_TEST_SOURCE_KEY_FILE` | PEM signer path |
| `LAYERX_TEST_DESTINATION_DID` | smoke destination DID |
| `LAYERX_TEST_ASSET` | node ConfigMap `asset-id` |
| `LAYERX_TEST_AMOUNT` | default `1` |
| `LAYERX_GATEWAY_CA_FILE` | same CA as `LAYERX_TEST_CA_FILE` |
| `LAYERX_IDENTITY_URL` | identity port-forward |
| `LAYERX_PAXEER_BOUNDARY_URL` | `$PAXEER_URL` |
| `LAYERX_PAXEER_SETTLEMENT_CONTRACT` | GuarantorBond address |
| `LAYERX_PAXEER_CHECKPOINT_REGISTRY` | CheckpointRegistry address |
| `LAYERX_PAXEER_DEPLOYMENT_RECORD` | `build/beta-cluster/paxeer/deployment.json` |
| `KUBECONFIG` | cluster kubeconfig |
| `WEBHOOKS_URL` | developer port-forward |
| `LAYERX_QUALIFICATION_NODE_URL` | `$NODE_URL` (`https://localhost:19446`, pending-core) unless `LAYERX_BETA_QUALIFICATION_NODE_URL` is set. Role: qualification node (`tools/qualification/release_runner.py:24`; `tools/qualification/beta_driver.py:41`). |
| `LAYERX_AGENT_BOUNDARY_URL` | `$AGENT_URL` (`https://localhost:19447`, agent boundary). |
| `LAYERX_QUALIFICATION_AGENT_URL` | `LAYERX_BETA_QUALIFICATION_AGENT_URL` when supplied for a real deployed agentd; otherwise explicitly unset. Bring-up does not start agentd. |
| `LAYERX_QUALIFICATION_HUMAN_URL` | `LAYERX_BETA_QUALIFICATION_HUMAN_URL` when set; otherwise `$HUMAN_URL` (`https://localhost:19453`, Human HTTPS API). |
| `LAYERX_QUALIFICATION_PAXEER_URL` | `$PAXEER_URL` unless `LAYERX_BETA_QUALIFICATION_PAXEER_URL` is set. Role: qualification Paxeer testnet (`tools/qualification/release_runner.py:27`; `tools/qualification/beta_driver.py:44`). |

`env_write` in `platform/hosted/tests/beta-cluster.sh` exports the node,
Human and Paxeer qualification origins, using their non-empty overrides
when provided. It exports the agent boundary separately and unsets the
agentd qualification variable unless a real agentd override is supplied.
The missing agentd endpoint is recorded in the bring-up missing-input list.
`LAYERX_TEST_AMOUNT` defaults to `1`.

HTTP against those origins uses the cluster CA. Hosted smoke passes
`--cacert "$LAYERX_TEST_CA_FILE"` (`platform/hosted/testnet/tests/hosted-smoke.sh:7,
32-33`). The CLI HTTP client builds a ureq agent with a 30s timeout and no CA
flag (`platform/cli/src/http.rs:41-49`).

Node network id is `402` (`platform/hosted/node/deployment.yaml:5`;
`platform/hosted/testnet/src/lib.rs:4`). Node asset id is
`b5a32b12029f8ddfb905f90f280f664b46390de0fc62770fc197dd87b18cd898`
(`platform/hosted/node/deployment.yaml:7`). Testnet control
`GET /v1/parameters` returns `network` `layerx-testnet`, `network_id`
`TESTNET_NETWORK_ID` (`402`), `package_semver`, `lxp_wire_protocol_version`,
and `reset_schedule` (`platform/hosted/testnet/src/main.rs:1155-1163`;
`platform/hosted/testnet/src/lib.rs:4`).

---

## 2.1. Run a 402LXP payment through public RPC

The maintained 402LXP client path uses JSON-RPC 2.0 at gateway `POST /rpc` and
WebSocket subscriptions at `GET /rpc/ws`. The Node and Python examples accept
the gateway and faucet origins explicitly:

```sh
node platform/middleware/examples/public-rpc.mjs --help
PYTHONPATH=agent/sdk/python python3 platform/middleware/examples/public_rpc.py --help
```

The payment sequence is challenge, payer grant, settlement receipt, then
renewal. A metered or subscription `PAYMENT-REQUIRED` challenge fixes the
registered asset, amount, recipient, payer, purpose and required commitment.
The payer signs and issues the canonical ordinal-7 grant. The receiver submits
the canonical signed ordinal-6 receive activity with
`lx_sendActivity(canonical_hex, commitment)`. A subscription renewal is a new
receive activity under the same recurring grant, with current sequences and a
new period idempotency key.

If submission is pending or its outcome is unknown, retain the exact signed
activity and query `lx_getActivityStatus(activity_id)` and
`lx_getReceipt(activity_id)`. Verify the returned sequencer signature, successful
result and exact activity binding. For a `batched` challenge, also obtain
`lx_getProof("receipt", activity_id)`, match its canonical value to that receipt,
and verify inclusion in the authorized signed batch header. Do not issue a
replacement draw with a new key. Only evidence that verifies at the challenged
`executed`, `batched` or `finalised` level authorizes fulfillment. The settlement reference remains
`lxp:<receipt_digest>`; an HTTP success, admission acknowledgement, queue state
or faucet response is not payment evidence.

The maintained disposable-network qualification exercises the same gateway
path end to end: a canonical ordinal-7 grant, 20 ordinal-6 metered draws at
`executed`, a recurring grant and first draw at `executed`, then a new-period
renewal at `batched`. The verified draw submit-to-receipt measurements were
p50 990,202 microseconds and p99 11,801,435 microseconds. The batched renewal's
verified settlement reference was
`lxp:21d0e81da67a7dac4d669d45b07f67b241c47285b120121e3d5700c8978233fd`.
These measurements describe that qualification run, not a service-level target.

Faucet funding is separate. Confirm the funded account with `lx_getAccount`,
`lx_getBalance` or `lx_getBalances(did)` before preparing a draw, and obtain the
current identity sequence through `lx_getSequence`. The examples read
`LAYERX_RPC_URL`, `LAYERX_FAUCET_URL` and `LAYERX_DID`; authentication uses
`LAYERX_RPC_TOKEN` and `LAYERX_FAUCET_TOKEN`. See
[402LXP transport](X402Transport.md) for the offer, grant and commitment
contracts.

The public hosted equivalents are `https://api.testnet.layerx.network/rpc` and
`wss://api.testnet.layerx.network/rpc/ws`; the faucet origin is
`https://faucet.testnet.layerx.network`. See
[Getting started on testnet](Getting-Started-Testnet.md) for the public
checklist and [Public JSON-RPC](PublicRpc.md) for every method and typed error.

---

## 3. Create a credential

For hosts without an OS Secret Service, first follow the
[headless credential setup](../../platform/cli/README.md#headless-credential-storage).
Keep the store selection and passphrase environment available for all commands.

The developer CLI binary is `layerx` (`platform/cli/Cargo.toml:11-13`;
`platform/cli/src/main.rs:31-32`). Global `--json` emits one JSON object
`{ok, kind, message, data}` (`platform/cli/src/main.rs:34-39`;
`platform/cli/src/output.rs:18-30`).

```sh
layerx key create quickstart
```

`name` is 1–128 ASCII alnum/`-`/`_` (`platform/cli/src/credential.rs:294-303`).
`--did` is optional (`platform/cli/src/main.rs:131-134`). Without it the DID
is `did:layerx:` plus the 64-hex public key
(`platform/cli/src/credential.rs:122`). Human output is `Created key {name} in
credential storage` plus JSON `{name, did, public_key}`
(`platform/cli/src/main.rs:831-834`). `--json` sets `kind` to `key.created`.

The seed is 32 OS-random bytes stored in the selected credential backend.
The default OS backend uses keyring service `dev.layerx.cli`
(`platform/cli/src/credential.rs:11, 83-96`). Key metadata in config is `did`
and `public_key` (`platform/cli/src/config.rs:19-23`).

Store the cluster session token (do not print the file contents):

```sh
tr -d '\r\n' < "$LAYERX_TEST_AUTH_TOKEN_FILE" | layerx auth set --environment testnet
```

`auth set` reads stdin and saves it (`platform/cli/src/main.rs:153-158,
858-865`). Human output is `Saved {environment} API token in credential storage` with data `{environment, secret_storage:
operating-system-credential-store}` by default, or
`secret_storage: encrypted-file-credential-store` for the file backend.
`kind` is `auth.saved`.
`--environment` optional; else current (`platform/cli/src/main.rs:1421-1449`).
Bring-up mints that token as `ses_` plus 32 hex, `.`, 64 hex when the source
is identity-provisioning (`platform/hosted/tests/beta-cluster.sh:1020-1026`).

Bind the hosted profile. `name` must be `emulator`, `testnet`, or `production`
(`platform/cli/src/config.rs:121-126`). `--endpoint`, `--network-id`, and one
of `--sequencer-trust-anchor` / `--sequencer-trust-anchor-file` must be
supplied together or omitted together (`platform/cli/src/emulator.rs:751-791`).
The anchor is 32-byte hex Ed25519 (`platform/cli/src/emulator.rs:821-824`).
`up` writes `sequencer_public_key` into `build/beta-cluster/identity`
(`platform/hosted/tests/beta-cluster.sh:1157`). For `testnet`, sequencer
identity is not probed (`platform/cli/src/main.rs:771-805`).

```sh
layerx environment use testnet \
  --endpoint "$LAYERX_GATEWAY_URL" \
  --network-id 402 \
  --sequencer-trust-anchor "$(sed -n 's/^sequencer_public_key=//p' build/beta-cluster/identity)"
```

Human output is `Using LayerX {name}` with data `{name, endpoint, network_id,
sequencer_trust_anchor}` (`platform/cli/src/main.rs:813-820`). `kind` is
`environment.selected`.

`layerx auth status --environment testnet` prints whether a token exists
without printing it (`platform/cli/src/main.rs:159-163, 908-919`).

Hosted account create is a different command: `--email`, `--display-name`,
`--idempotency-key`; `--initial-amount` must be `0`
(`platform/cli/src/account.rs:42-54`). It POSTs `/v1/accounts`
(`platform/cli/src/account.rs:56-63`). Hosted gateway `production_route` does
not include `/v1/accounts` (`platform/hosted/gateway/src/lib.rs:809-881`).

---

## 4. Request funds from the faucet

There is no `layerx faucet` command (`platform/cli/src/main.rs:53-99`). The
faucet claim route is `POST /v1/faucet/claims`
(`platform/hosted/faucet/src/main.rs:1025`).

Required headers: `Content-Type: application/json`
(`platform/hosted/faucet/src/main.rs:924-925`), `Idempotency-Key` 1–128
alnum/`-`/`_`/`.`/`:` (`platform/hosted/faucet/src/main.rs:946-950, 180-186`),
`Authorization: Bearer` session (`platform/hosted/faucet/src/main.rs:470-504`).
Body fields are `did` and `public_key` only
(`platform/hosted/faucet/src/main.rs:100-105`). `did` must start with `did:`
(`platform/hosted/faucet/src/main.rs:197-199`); `public_key` is 64 hex
(`platform/hosted/faucet/src/main.rs:201-203, 953-954`).

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

(`platform/hosted/testnet/tests/hosted-smoke.sh:101-109`).

A 200 body has `funded` `true`, `funding_id`, optional `transaction_id`,
`amount` as a decimal string of `LAYERX_FAUCET_CLAIM_AMOUNT` (default
`1000000`), and `network` `layerx-testnet`
(`platform/hosted/faucet/src/main.rs:255, 892-898`). Smoke asserts
`.funded == true and .funding_id != null`
(`platform/hosted/testnet/tests/hosted-smoke.sh:110`). A 202 body is
`state` `still_checking`, `retry` `after`, `retry_after_seconds` `10`
(`platform/hosted/faucet/src/main.rs:1125-1131`).

For a complete request and response copied from a real faucet, hosted gateway,
and native-node run, continue with
[Public payment API](PublicAPI.md). The transcript covers asset registration,
account opening, mint, SEND, balances, asset metadata, and receipt calls without
shortening canonical bytes.

Testnet control admits the funding journey at `GET /v1/journeys/funding`
(`platform/hosted/testnet/src/main.rs:229, 1165-1171`). Smoke requires
`.admitted == true and .ready == true and (.failing | length) == 0`
(`platform/hosted/testnet/tests/hosted-smoke.sh:38-49, 101`).

---

## 5. Deploy a program

There is no `layerx programs` command (`platform/cli/src/main.rs:53-99`).
The command is `layerx program deploy <artifact>`
(`platform/cli/src/main.rs:263-272, 1101-1117`;
`platform/cli/src/programs.rs:245-279`).

From the faucet-funded credential, scaffold and compile a WASM artifact:

```sh
layerx new quickstart-program
layerx --json program build --manifest-path quickstart-program/Cargo.toml
```

`name` is a lowercase Cargo package name, 1–64, digits/`-`, not starting
with `-` (`platform/cli/src/scaffold.rs:52-62`). `--directory` default `.`
(`platform/cli/src/main.rs:101-106`). `--json` `kind` is `project.created`
(`platform/cli/src/main.rs:543-547`). `program build` `--manifest-path`
default `Cargo.toml`; `--artifact` optional
(`platform/cli/src/main.rs:241-247, 1083-1090`). Without `--artifact` the
Rust toolchain must produce exactly one `.wasm` under
`target/wasm32-unknown-unknown/release`
(`platform/cli/src/programs.rs:176-213, 1976-1995`). `--json` `kind` is
`program.built`. Data includes `artifact`, `code_hash`, `byte_size`,
`function_count`, `abi_version`, `deterministic_validation`
(`platform/cli/src/programs.rs:216-236`).

The deploy path is the `artifact` field from `program.built`.

```sh
layerx --json program deploy \
  quickstart-program/target/wasm32-unknown-unknown/release/quickstart_program.wasm \
  --program-id <program_id> \
  --idempotency-key <idempotency_key> \
  --key quickstart \
  --account-sequence 0 \
  --not-before-ms <not_before_ms> \
  --expires-at-ms <expires_at_ms> \
  --previous-state-root <previous_state_root>
```

Required lifecycle flags: `--program-id`, `--idempotency-key`,
`--account-sequence`, `--not-before-ms`, `--expires-at-ms`,
`--previous-state-root` (`platform/cli/src/main.rs:297-315, 1503-1569`).
`--key` optional (else the configured default)
(`platform/cli/src/main.rs:304, 697-711, 1034`). `--fee-limit` default `0`
(`platform/cli/src/main.rs:311-312`). `--upgrade-authority` and
`--interface` optional (`platform/cli/src/main.rs:266-269`). Without
`--upgrade-authority` the policy is immutable
(`platform/cli/src/programs.rs:263-269`). `--interface` is canonical
encoded interface bytes, not KVX source
(`platform/cli/src/programs.rs:349-357`).

`--program-id` and `--previous-state-root` are 32-byte hex
(`platform/cli/src/programs.rs:261, 429`;
`platform/cli/src/encoding.rs:29-33`). `--idempotency-key` is 32-byte hex
at submit time (`platform/cli/src/programs.rs:952`); clap and
`validate_idempotency_key` also admit 16–128 alnum/`-`/`_`
(`platform/cli/src/http.rs:388-399`). Hosted `POST /v1/programs/deploy`
requires `Idempotency-Key` of 64 lowercase hex
(`platform/hosted/gateway/src/main.rs:1535-1545, 829-833`). Validity
`expires_at_ms - not_before_ms` must be in `(0, 300000]`
(`platform/cli/src/programs.rs:944-950`). There is no CLI command that
reads the current state root (`platform/cli/src/main.rs:53-99`). Hosted
`GET /v1/state` is `503` `principal_state_proof_unavailable`
(`platform/hosted/gateway/src/lib.rs:816`;
`platform/hosted/gateway/src/main.rs:2275`).

The command POSTs canonical signed bytes to `/v1/programs/deploy`
(`platform/cli/src/programs.rs:436-442`). `--json` `kind` is
`program.lifecycle`; human text is `Programs lifecycle outcome on
{environment}` (`platform/cli/src/main.rs:1064-1068`). On a completed
receipt the data object has `activity_id`, `receipt` (hex), `result_code`,
`outcome.status` `completed` or `refused`,
`verified_previous_state_root`, `verified_resulting_state_root`,
`verification`, and `artifact`
(`platform/cli/src/programs.rs:519-526, 276-278`). Hosted gateway 200 is
`{ok: true, result: {activity_id, receipt, state, terminal_payload,
call_graph}, trace}` (`platform/hosted/gateway/src/main.rs:1883-1911`).
`state` is `completed` when `result_code == 0`, else `refused`.

`active_client` sends the stored session as `Authorization: Bearer`
(`platform/cli/src/main.rs:1415-1418`; `platform/cli/src/http.rs:240-246`).
Hosted `POST /v1/programs/deploy` authenticates `LayerX-Key` with scope
`program:call` (`platform/hosted/gateway/src/lib.rs:813`;
`platform/hosted/gateway/src/main.rs:1077-1080, 1144-1153, 2413-2415`).
Bearer on that route is `401 api_key_required`. Those sources disagree
on how deploy reaches the gateway. The CLI command that issues a gateway
key is `layerx install a2a`
(`platform/cli/src/install/mod.rs:603-632`); `layerx install mcp` binds
to the agent daemon and issues no key
(`platform/cli/src/install/mcp.rs:33-56`).

---

## 6. Submit one signed activity through the CLI

```sh
layerx --json payment test \
  --from "$LAYERX_TEST_SOURCE_DID" \
  --to "$LAYERX_TEST_DESTINATION_DID" \
  --currency "$LAYERX_TEST_ASSET" \
  --amount "$LAYERX_TEST_AMOUNT" \
  --idempotency-key paymentquickstart1
```

`--from`, `--to`, `--currency` (alias `--asset`), `--amount` (>0),
`--idempotency-key` (16–128 alnum/`-`/`_`) are required
(`platform/cli/src/main.rs:194-207`; `platform/cli/src/payment.rs:5-14`;
`platform/cli/src/http.rs:388-399`). The command POSTs `/v1/moves/quote` then
`/v1/moves` with `{quote_id}` (`platform/cli/src/payment.rs:23-32`). It
reads `quote_id` from `/result/quote_id` (`platform/cli/src/payment.rs:24-27`).
`--json` `kind` is `payment.started`; `data` has `quote`, `journey`,
`idempotency_key` (`platform/cli/src/main.rs:991-994`;
`platform/cli/src/payment.rs:33-37`). It does not run `verify_outcome`
(`platform/cli/src/payment.rs:5-37`). These two routes are Human-plane
operations (`human/schema/human-api/movement.kvx:116-131`). The emulator serves
that Human contract directly (`platform/emulator/src/main.rs:2667-2668`).

The hosted smoke harness also posts the Human-plane `/v1/moves/quote` and
`/v1/moves` contract with the session Bearer, then reads `.result.quote_id` and
`.result.receipt_id` (`platform/hosted/testnet/tests/hosted-smoke.sh:113-132`).
Those paths are not part of the hosted gateway's production route parser.
`production_route` accepts `POST /v1/activities`,
program call/deploy/upgrade/wind-down/simulate, `GET /v1/state`,
`GET /v1/receipts/{id}`, and program registry/interface/activity/receipt reads
(`platform/hosted/gateway/src/lib.rs:802-890`). Unknown production routes are
`404 not_found`
(`platform/hosted/gateway/src/main.rs:1760-1766`). Production routes
authenticate `LayerX-Key`, not Bearer (`platform/hosted/gateway/src/main.rs:1162-1173,
1748`). Bearer on those routes is `401 api_key_required`. Bearer is the
session scheme for `/v1/keys` (`platform/hosted/gateway/src/main.rs:972-981,
1081-1088`). Therefore `layerx payment test` is an emulator/Human-plane client
path, not a hosted production-gateway payment command. Use the
`lx_sendActivity` public RPC path for hosted canonical payment activities.

There is no `layerx activity` command (`platform/cli/src/main.rs:53-99`).
MCP/A2A `activity.submit` POSTs JSON `{"activity": <hex>}` to
`/v1/activities` (`platform/cli/src/toolset.rs:341-344`). That route
requires scope `activity:write` (`platform/hosted/gateway/src/main.rs:1072-1087`).

The CLI command that issues a gateway key is `layerx install a2a`,
which POSTs `/v1/keys` with the stored session
(`platform/cli/src/install/mod.rs:603-632`). Payment-capable install
requires `--source-account` and `--asset` as 64-hex
(`platform/cli/src/install/a2a.rs:357-376`). `layerx install mcp` takes
neither: it binds to the agent daemon and issues no key
(`platform/cli/src/install/mcp.rs:33-56`). There is no other CLI
key-issue command. The HTTP issue body is `{signer_public_key, scopes, quota_requests,
quota_window_seconds}` (`platform/hosted/gateway/src/main.rs:77-84`). Success
JSON includes `ok`, `key.id`, `key.secret`, `key.authorization_scheme`
`LayerX-Key`, `key.scopes` (`platform/hosted/gateway/src/main.rs:2354-2367`).
`signer_public_key` must be in the session allow-list or the gateway returns
`403 signer_not_owned` (`platform/hosted/gateway/src/main.rs:2308-2313`).
Bring-up puts `$LAYERX_TEST_SOURCE_PUBLIC_KEY` on the smoke principal
(`platform/hosted/tests/beta-cluster.sh:1012-1014`).

Hosted activity POST body used by the CLI toolset is `{"activity": <hex>}`
with `Idempotency-Key` (`platform/cli/src/toolset.rs:341-344`). Gateway
success for a completed non-program activity is `{ok: true, result, trace}`
(`platform/hosted/gateway/src/main.rs:3576-3578`).

---

## 7. Fetch the receipt

```sh
layerx --json receipt get <id>
```

`id` is path-safe: 1–256 alnum/`-`/`_`/`:`/`.` (`platform/cli/src/http.rs:372-383`;
`platform/cli/src/main.rs:212-213, 997-1005`). The command GETs
`/v1/receipts/{id}` on the active endpoint. Human output is `Read receipt {id}
from {environment}`. `--json` `kind` is `receipt.read`; `data` is the GET body.

Hosted gateway GET `/v1/receipts/{id}` requires `LayerX-Key` and scope
`receipt:read` (`platform/hosted/gateway/src/lib.rs:870-878`;
`platform/hosted/gateway/src/main.rs:1072-1087, 1748`). The 200 body is
`{ok: true, result: {activity_id, receipt}, trace}` where `receipt` is hex
(`platform/hosted/gateway/src/main.rs:2526-2532`). `activity_id` in the path
must be 64 hex (`platform/hosted/gateway/src/lib.rs:874-875`).

Hosted smoke GETs `$LAYERX_GATEWAY_URL/v1/receipts/$receipt_id` with the
session Bearer, writes `.result.receipt` as hex, and reads
`.result.authority.batch_id`, `.result.authority.asset`,
`.result.authority.previous_state_root`,
`.result.authority.resulting_state_root`,
`.result.authority.sequencer_public_key`
(`platform/hosted/testnet/tests/hosted-smoke.sh:134-143`). The gateway GET
body above has no `authority` object. Those two sources disagree.

Authorised-batch facts with those five names plus `activity_id`,
`network_id`, and `wire_version` are served by receipt-authority
`GET /v1/authorized-batches/by-activity/{activity_id}`
(`platform/hosted/authority/src/main.rs:46, 740-749, 656-667`). Routes other
than `/livez` and `/readyz` require `Authorization: Bearer`
(`platform/hosted/authority/src/main.rs:46-49, 747-749`).

---

## 8. Verify the receipt locally

```sh
layerx --json receipt verify \
  --receipt receipt.hex \
  --batch-id "$batch_id" \
  --asset "$asset" \
  --previous-state-root "$previous_root" \
  --resulting-state-root "$resulting_root" \
  --sequencer-public-key "$sequencer_key"
```

All six flags are required (`platform/cli/src/main.rs:214-232, 1007-1019`).
The check is local; it does not contact the endpoint
(`platform/cli/src/main.rs:1011-1023`). The file is hex text or raw bytes
(`platform/cli/src/receipt.rs:54-64`). The five hex flags become
`AuthorizedBatch`; the call is `layerx_proof::receipt::verify_outcome`
(`platform/cli/src/receipt.rs:25-34`).

On success the data object has `verified` `true`, `verification_level`
(wire rank), `receipt_digest`, `activity_id`, `batch_id`, `result_code`,
`canonical_bytes` (length) (`platform/cli/src/receipt.rs:43-51`). `--json`
wraps that as `ok` `true`, `kind` `receipt.verified`
(`platform/cli/src/main.rs:1011-1013`; `platform/cli/src/output.rs:20-25`).
Smoke asserts `.ok == true and .kind == "receipt.verified" and
.data.verified == true` (`platform/hosted/testnet/tests/hosted-smoke.sh:144-148`).

Failure text is `receipt verification failed at {:?}`
(`platform/cli/src/receipt.rs:33-34`).

---

## 9. Observe finality against the Paxeer boundary

The CLI talks HTTP to the active environment endpoint
(`platform/cli/src/main.rs:1420-1424`; `platform/cli/src/http.rs:24-50`).
`layerx --rpc <url> wallet|token …` POSTs JSON-RPC 2.0 to the gateway `/rpc`
surface (`platform/cli/src/rpc.rs:14-15, 29-36`). That is not a `layerxd` node
admin socket. `layerx receipt verify` is local `layerx_proof` against
caller-supplied batch facts (`platform/cli/src/receipt.rs:4, 25-34`).

The Paxeer boundary is `$LAYERX_PAXEER_BOUNDARY_URL` (default
`https://localhost:19449`). Routes (`platform/hosted/paxeer/src/main.rs:477-493`):

```sh
curl --fail --silent --show-error --max-time 10 --cacert "$LAYERX_TEST_CA_FILE" \
  "$LAYERX_PAXEER_BOUNDARY_URL/readyz"
```

When the node `eth_chainId` equals the configured chain id, the 200 body is
`status` `ready`, `service` `paxeer-boundary`, `chain_id` (decimal)
(`platform/hosted/paxeer/src/main.rs:466-469`). Bring-up sets that chain id
to `125` (`platform/hosted/tests/beta-cluster.sh:96`;
`platform/hosted/paxeer/deployment.yaml:42`).

```sh
curl --fail --silent --show-error --max-time 30 --cacert "$LAYERX_TEST_CA_FILE" \
  --dump-header - "$LAYERX_PAXEER_BOUNDARY_URL/genesis"
```

`GET /genesis` is served when `LAYERX_PAXEER_COMET_URL` is set: 200 body is
the Comet genesis JSON; `X-LayerX-Genesis-SHA256` is the SHA-256 of those
bytes (`platform/hosted/paxeer/src/main.rs:164-172, 484-488`;
`platform/hosted/paxeer/src/genesis.rs:241-256`). The genesis JSON must
carry a non-empty `chain_id` string (`platform/hosted/paxeer/src/genesis.rs:230-237`).
Bring-up sets Comet chain id `hyperpax_125-1`
(`platform/hosted/paxeer/init-chain.sh:39-42`). When the Comet URL is unset,
`GET /genesis` is `404 not_found` (`platform/hosted/paxeer/src/main.rs:164-166,
484-488`).

```sh
curl --fail --silent --show-error --max-time 15 --cacert "$LAYERX_TEST_CA_FILE" \
  --header 'Content-Type: application/json' \
  --request POST "$LAYERX_PAXEER_BOUNDARY_URL/" \
  --data '{"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}'
```

`POST /` relays JSON-RPC methods that pass `method_allowed`: `eth_*` except
the denied list, plus `net_version` and `web3_clientVersion`
(`platform/hosted/paxeer/src/main.rs:28-39, 371-384, 490-491`). Denied
methods include `eth_accounts`, `eth_sendTransaction`, `eth_sign`
(`platform/hosted/paxeer/src/main.rs:30-38`). Readiness itself calls
`eth_chainId` with `params` `[]` (`platform/hosted/paxeer/src/main.rs:448-449`).

Env also exports `LAYERX_PAXEER_CHECKPOINT_REGISTRY` and
`LAYERX_PAXEER_SETTLEMENT_CONTRACT` (`platform/hosted/tests/beta-cluster.sh:1129-1130`).
There is no CLI command that reads those contracts.

Testnet control journey routes are `/v1/journeys/funding`,
`/v1/journeys/payment`, `/v1/journeys/receipt-inspection`, and
`/v1/journeys/programs` (`platform/hosted/testnet/src/main.rs:227-234`).
`/v1/journeys/settlement` is not one of them
(`platform/hosted/testnet/src/main.rs:1498`).

---

## 10. Tear down

```sh
make platform-beta-cluster-down
```

runs `bash platform/hosted/tests/beta-cluster.sh down`
(`platform/Makefile.inc:187-188`). `down` stops port-forwards, deletes the
kind cluster when mode is kind or absent, removes labeled images, deletes
`build/beta-cluster`, and unless `LAYERX_BETA_KEEP_TOOLS=1` removes pinned
kind/kubectl/calico downloads (`platform/hosted/tests/beta-cluster.sh:1292-1317`).
It prints `teardown complete` (`platform/hosted/tests/beta-cluster.sh:1317`).

[Home](Home.md)

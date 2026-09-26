# Paxeer X Network bridge

The Paxeer X Network bridge is lock and release in both directions: a foreign chain locks a deposit in the bridge's vault and Paxeer X Network releases the bridged denom through the layerxBridge precompile at `0x0000000000000000000000000000000000001016` against a threshold of attestor signatures, and a burn of that denom on Paxeer X Network is attested so the foreign vault releases what it holds. The default pair on every chain is PAX against that chain's native coin, and SID - Sidiora, the second official coin - bridges between Paxeer X Network and Solana, its foreign home.

This page is the operator runbook: what to deploy, in what order, with which environment variables, and what must read back before the bridge is opened. The Paxeer side is finished and the bridge configures it; nothing in these steps changes the chain. The product site is [paxeer.app](https://paxeer.app) and the source is [the Paxeer X Network repository](https://github.com/Sidiora-Labs/Paxeer-X-Network).

---

## What is here

| Path | What it is |
| --- | --- |
| `bridge/evm` | the Foundry project of `PaxeerXVault`, one source deployed as identical bytecode to every EVM chain |
| `bridge/evm/chains/<name>` | one `config.json` and one page per EVM chain |
| `bridge/solana` | the Solana custody program, a raw `solana-program` crate |
| `bridge/solana/chains/solana` | the Solana `config.json` and its page |
| `bridge/deploy` | the configuration schema, the proposal generator, the attestor-set manifest and the deploy and verify scripts |
| `bridge/vectors` | the Solana handle derivation and the pinned digest vectors |
| `bridge/ATTESTATION-SOLANA.md` | how Solana keys, mints and transactions enter the attestation digests |
| `modules/layerxbridge/ATTESTATION.md` | the byte-exact specification of both preimages and the signature rules |

## The chains

| Chain | Chain id | Pair | Page |
| --- | --- | --- | --- |
| `ethereum` | `1` | PAX against ETH | `bridge/evm/chains/ethereum/README.md` |
| `base` | `8453` | PAX against ETH | `bridge/evm/chains/base/README.md` |
| `arbitrum` | `42161` | PAX against ETH | `bridge/evm/chains/arbitrum/README.md` |
| `optimism` | `10` | PAX against ETH | `bridge/evm/chains/optimism/README.md` |
| `bnb` | `56` | PAX against BNB | `bridge/evm/chains/bnb/README.md` |
| `polygon` | `137` | PAX against POL | `bridge/evm/chains/polygon/README.md` |
| `avalanche` | `43114` | PAX against AVAX | `bridge/evm/chains/avalanche/README.md` |
| `hyperevm` | `999` | PAX against HYPE | `bridge/evm/chains/hyperevm/README.md` |
| `solana` | `91600046870081` | PAX against SOL, and SID | `bridge/solana/chains/solana/README.md` |

The native coin is the first asset of every configuration and is always registered: `0x0000000000000000000000000000000000000000` with eighteen decimals on an EVM chain, deposited through `depositNative`, and the wrapped SOL mint `So11111111111111111111111111111111111111112` with nine decimals on Solana. Every chain differs from the others only in its configuration file.

## Tools

| Tool | Needed by |
| --- | --- |
| `git` | `bridge/evm/bootstrap-libs.sh` |
| `jq`, `forge`, `cast` | `bridge/deploy/deploy-evm-chain.sh`, `bridge/deploy/verify-evm-chain.sh` |
| `jq`, `python3`, `cast`, `sha256sum` and the pinned Solana toolchain | `bridge/deploy/deploy-solana-program.sh` |
| `go` | the proposal generator under `bridge/deploy/proposals` |

## Fill in the configuration

Every committed configuration carries values nobody has filled in, and every tool refuses them, naming the file and the field. Nothing defaults.

- `owner` is `PLACEHOLDER:owner` in every configuration: the vault owner address on an EVM chain, the program owner's base58 key on Solana.
- `attestors` are `PLACEHOLDER:attestor-1` to `PLACEHOLDER:attestor-5` in every configuration, at a threshold of `3`. There is one attestor set for the whole bridge: fill in the same five secp256k1 addresses, strictly ascending, everywhere.
- `bridge/deploy/attestors.json` is the attestor-set manifest, the one committed record of the attestor addresses and the threshold. It carries the repeated-byte addresses `0x1111111111111111111111111111111111111111` to `0x5555555555555555555555555555555555555555`, which the proposal generator refuses; it also refuses a configuration whose set or threshold differs from the manifest.
- `big_blocks.acknowledged` is `false` in `bridge/evm/chains/hyperevm/config.json`. The HyperEVM page says what to do before setting it to `true`.
- `solana.program_id` is `PLACEHOLDER:program-id` until the first Solana deployment prints the real id.

## Order of operations

### 1. Bootstrap the libraries

```sh
bash bridge/evm/bootstrap-libs.sh
```

Clones `forge-std` `v1.9.6` and `openzeppelin-contracts` `v5.3.0` into the `lib` directory beside `bridge/evm/foundry.toml`, which is never committed. A second run with both libraries already at their tag and unmodified does nothing; an edited checkout is replaced. The deploy script runs this itself before it builds.

### 2. Deploy each EVM chain

| Variable | Holds |
| --- | --- |
| the variable in `environment.rpc_url`, `PAXEER_BRIDGE_<CHAIN>_RPC_URL` | the chain's endpoint |
| the variable in `environment.deploy_key`, `PAXEER_BRIDGE_<CHAIN>_DEPLOY_KEY` | the deployer key |
| `PAXEER_BRIDGE_DEPLOYMENT_RECORD` | the path the deployment record is written to; its directory must exist and be writable |
| `PAXEER_BRIDGE_EVM_CHAINS_ROOT` | optional; a chains root in place of `bridge/evm/chains` |

```sh
bash bridge/deploy/deploy-evm-chain.sh --preflight <chain>
bash bridge/deploy/deploy-evm-chain.sh <chain>
```

`--preflight` runs every check that needs no endpoint - the configuration, the variables it names and the tools - and stops before the first call to the chain. The deployment then confirms the endpoint's chain id against the configuration, builds with the pinned libraries, deploys `PaxeerXVault` with the attestor set and threshold, sets the caps of every asset the configuration lists, and proposes ownership to the configured owner. It reads the deployed code, owner, threshold, attestors and caps back, stops if the code hash is not the built one or the threshold, the attestors or a cap differs from the configuration, and writes a record naming the chain, the vault address, the deployer, the code hash, the block, the owner, the ownership state, the threshold, the attestors and the caps.

Ownership moves in two steps. Until the configured owner calls `acceptOwnership()` on the vault, the deployer is still the owner and the record reads `"ownership": "proposed"`.

The deploy key reaches `forge script` as a command argument, so run a deployment only where another user cannot read process arguments.

A vault accepts deposits as soon as it exists: it starts unpaused with its caps set. Until the Paxeer side reads back as registered for the chain, the owner can hold it with `pause()`.

### 3. Verify the source

| Variable | Holds |
| --- | --- |
| the variable in `environment.explorer_key`, `PAXEER_BRIDGE_<CHAIN>_EXPLORER_KEY` | the explorer verification key |
| `PAXEER_BRIDGE_DEPLOYMENT_RECORD` | the record step 2 wrote |

```sh
bash bridge/deploy/verify-evm-chain.sh --preflight <chain>
bash bridge/deploy/verify-evm-chain.sh <chain>
```

The vault address and the constructor arguments come from the deployment record, so a source cannot be claimed for a deployment that was never made. The script submits through `forge verify-contract` and reports the explorer's own answer; anything other than a verified or already-verified answer stops it.

### 4. Deploy and initialise the Solana program

```sh
bash bridge/deploy/deploy-solana-program.sh --preflight
bash bridge/deploy/deploy-solana-program.sh
```

The Solana page, `bridge/solana/chains/solana/README.md`, lists the variables this step needs - the endpoint, the publisher keypair file, the pinned toolchain directory, the deployment record and the program's admin client - and what each must hold. The script builds the program with `cargo-build-sbf`, deploys it, checks the upgrade authority, the deployed ELF hash and that the deployment is rooted, initialises the program with the owner, the attestor set and the threshold, registers the wrapped SOL mint and then the Sidiora mint with their caps, and writes the deployment record.

Two things the Solana page covers in full: the admin client the script calls is not part of this repository, and the vault handle Paxeer registers for Solana comes from the seed `vault-authority`, not from the `vault_handle` field of the record.

### 5. Generate the proposals

```sh
go run ./bridge/deploy/proposals/cmd/paxeer-bridge-proposals -manifest bridge/deploy/attestors.json <proposal input> <output directory>
```

`-manifest` defaults to `bridge/deploy/attestors.json`. The generator reads one chain's proposal input and the manifest, and writes into an empty or absent output directory, in the order they are submitted:

| File | Body |
| --- | --- |
| `01-register-chain.json` | `MsgRegisterChain`: the chain id, the vault - on Solana the vault handle - and the finality depth, with the chain enabled |
| `02-set-attestors.json` | `MsgSetAttestors`: the shared attestor set and the threshold |
| `03-set-cap-<NN>-<asset id>.json` | one `MsgSetCap` per asset, the native coin first; for Solana, `SID` under `0x21f7b20a555199fa73A238B1a91FD0f549068fEe` |

Every body is marshalled from the message types in `modules/layerxbridge/types`, so it cannot drift from what the keeper accepts. The generator refuses a placeholder or zero authority, owner, vault or attestor, a zero threshold or one above the attestor count, a zero cap, and a set that differs from the manifest, and it writes nothing when it refuses.

The proposal input is a JSON file in the generator's own schema, decoded with unknown fields refused: `name`, `chain_id`, `native_symbol`, `native_decimals`, `rpc_endpoint_env`, `explorer_key_env`, `governance_authority`, `owner`, `vault`, `attestors`, `threshold`, `finality_depth`, `program_id`, `commitment`, `big_blocks_required` and `assets`, each asset carrying `address`, `asset_id`, `decimals`, `max_per_tx` and `max_total`. It is not the chain configuration file: the generator refuses the configuration's own fields. Write it from three sources - the chain configuration for the chain, the attestors, the finality depth and the assets with their caps; the deployment record for `vault`; and the bridge authority for `governance_authority`. `bridge/deploy/proposals/testdata/ethereum.json` and `bridge/deploy/proposals/testdata/solana.json` show the shape.

`governance_authority` is the bech32 account the bridge module's parameters name as its authority; the module's default genesis names the governance module account. The keeper applies each body only when its authority is that account.

### 6. Submit the proposals

Submit `01-register-chain.json`, then `02-set-attestors.json`, then the `03-set-cap-*` bodies in their numbered order, each executed as the bridge authority. For Solana, the Sidiora cap body waits for the check in the next section.

The bodies are the bridge module's message types field for field. The bridge module registers no message service and no transaction command, so this repository carries no command that broadcasts them; the holder of the bridge authority submits them through the path that authority executes with.

### 7. Read the deployment back

The bridge is opened only when every value below reads back as the configuration and the generated bodies say, with the native coin checked first on every chain. A read changes nothing anywhere.

On each EVM chain, against the vault address in the deployment record:

```sh
cast code <vault> --rpc-url "$PAXEER_BRIDGE_<CHAIN>_RPC_URL"
cast call <vault> 'owner()(address)' --rpc-url "$PAXEER_BRIDGE_<CHAIN>_RPC_URL"
cast call <vault> 'attestors()(address[])' --rpc-url "$PAXEER_BRIDGE_<CHAIN>_RPC_URL"
cast call <vault> 'threshold()(uint256)' --rpc-url "$PAXEER_BRIDGE_<CHAIN>_RPC_URL"
cast call <vault> 'caps(address)(uint256,uint256)' <asset> --rpc-url "$PAXEER_BRIDGE_<CHAIN>_RPC_URL"
cast call <vault> 'paused()(bool)' --rpc-url "$PAXEER_BRIDGE_<CHAIN>_RPC_URL"
```

`cast keccak` of the code must equal the record's `code_hash`; the owner must be the configured owner, not the deployer; the attestors and threshold must be the manifest's; every asset, the native coin first, must carry the configured per-transaction and total cap; and `paused` must be `false`.

On Solana, read the config account (seed `config`) and every asset account (seed `asset` and the mint) of the program with `solana account`; their layouts are in `bridge/solana/src/state.rs`. The owner, the attestors, the threshold and the paused flag must match the configuration, and each asset account must carry its mint, its asset id and its caps, wrapped SOL first.

On Paxeer X Network, through the layerxBridge precompile:

```sh
cast call 0x0000000000000000000000000000000000001016 'getChain(uint64)(bool,address,uint64,bool)' <chain id> --rpc-url <Paxeer X Network EVM endpoint>
cast call 0x0000000000000000000000000000000000001016 'getAttestors()(address[],uint256[],uint32)' --rpc-url <Paxeer X Network EVM endpoint>
cast call 0x0000000000000000000000000000000000001016 'getCap(uint64,address)(string,uint256,uint256,uint256)' <chain id> <asset id> --rpc-url <Paxeer X Network EVM endpoint>
cast call 0x0000000000000000000000000000000000001016 'isPaused()(bool)' --rpc-url <Paxeer X Network EVM endpoint>
```

`getChain` must report the chain registered and enabled with the vault - on Solana the vault handle - and the finality depth of `01-register-chain.json`. `getAttestors` must return the manifest's set and threshold. `getCap` must return a denom and the `max_in_flight` and `max_per_tx` of each cap body, the native coin first; an empty denom means the asset is not registered. `isPaused` must be `false`.

Any disagreement, any placeholder still in place, an unregistered or uncapped native coin, a paused vault, program or precompile, or a chain the Paxeer side does not report as registered keeps the bridge closed.

## The Sidiora pair on Solana

An inbound SID deposit from Solana resolves to the `usid` denom only if the Paxeer side records the pair (`91600046870081`, `0x21f7b20a555199fa73A238B1a91FD0f549068fEe`) against `usid` before any cap is set for it.

- `MsgSetCap` for a pair the Paxeer side has not registered registers it itself, under the bridge's generic denom `factory/<bridge module account>/lxb<hex>`, not under `usid`. Submitting Sidiora's cap body first binds Solana's SID to the wrong denom, and `EnsureSidioraDenom` then refuses to rebind it.
- No generated body registers the pair against `usid`. The chain does it: `EnsureSidioraDenom` in `modules/layerxbridge/keeper/sidiora.go` records it, and its production caller is the handler of the `v6.7` upgrade in `node/upgrades.go`, which calls it for the chain the `usid` denom is already recorded against and refuses to run when the denom is recorded against none. The bridge module's genesis state is the other place an asset record is written.

So, for Solana: submit `01-register-chain.json` and `02-set-attestors.json`, then the wrapped SOL cap. Before Sidiora's cap body, read the pair back:

```sh
cast call 0x0000000000000000000000000000000000001016 'getCap(uint64,address)(string,uint256,uint256,uint256)' 91600046870081 0x21f7b20a555199fa73A238B1a91FD0f549068fEe --rpc-url <Paxeer X Network EVM endpoint>
```

Submit Sidiora's cap body only when the denom it returns is `factory/pax1dzfx9mk4fl9kl2mysjmtvk2xp75ljumk6nynhf/usid`, the `usid` denom of the bridge module account. An empty denom means the pair is not recorded yet; a denom ending in `lxb` followed by hex means a cap was set first and the pair is bound to the wrong denom.

## The relayer

`interop/crates/layerx-bridge-relayer` observes the vaults, has the attestations signed through the remote signer and submits both directions. It runs as `layerx-bridge-relayer --config <path>` against one JSON file, decoded with unknown fields refused: `journal_path`, an optional `cosign_directory`, `poll_interval_ms`, `max_submissions`, `signer`, `attestor`, `paxeer` and `chains`. Each `chains` entry carries `chain_id`, `vault`, `finality_depth`, `start_block`, `max_block_range`, `rpc`, `submitter` and `gas`. The file carries key handles and public keys only, never a private key. Its `chains` entries are EVM chains; the configuration has no Solana entry.

# HyperEVM

PAX against HYPE. This page is the HyperEVM leg of the Paxeer X Network bridge: a HYPE deposit locked in the vault on HyperEVM is released as its bridged denom on Paxeer X Network, and a burn of that denom on Paxeer X Network releases the HYPE locked here. The operator runbook is `bridge/README.md`; this page lists what this chain needs.

| Field | Value |
| --- | --- |
| Chain name | `hyperevm` |
| Chain id | `999` |
| Native coin | `HYPE`, 18 decimals |
| Finality depth | `60` blocks |
| Attestors | 5, signing at a threshold of 3 |
| Vault | `PaxeerXVault`, the same bytecode every EVM chain of the bridge deploys |
| Configuration | `bridge/evm/chains/hyperevm/config.json` |

## Assets

The configuration registers one asset, the native coin. It is the first asset of the file and is always registered, so the default pair works the moment the chain is opened.

| Asset | Address and asset id | Decimals | Per-transaction cap | Total cap |
| --- | --- | --- | --- | --- |
| HYPE | `0x0000000000000000000000000000000000000000` | 18 | `20000000000000000000000` (20,000 HYPE) | `400000000000000000000000` (400,000 HYPE) |

HYPE is deposited through `depositNative` and paid out by the vault's native release path. The caps are in base units and apply to the vault on this chain; the same values go into the `MsgSetCap` body for this asset on the Paxeer side.

## Environment variables

The configuration names the variables; neither the configuration nor the scripts carry a value for any of them.

| Variable | Read by | Holds |
| --- | --- | --- |
| `PAXEER_BRIDGE_HYPEREVM_RPC_URL` | `bridge/deploy/deploy-evm-chain.sh` | the HyperEVM endpoint |
| `PAXEER_BRIDGE_HYPEREVM_DEPLOY_KEY` | `bridge/deploy/deploy-evm-chain.sh` | the deployer key |
| `PAXEER_BRIDGE_HYPEREVM_EXPLORER_KEY` | `bridge/deploy/verify-evm-chain.sh` | the explorer verification key |
| `PAXEER_BRIDGE_DEPLOYMENT_RECORD` | both scripts | the deployment record: written by the deploy, read by the verification |
| `PAXEER_BRIDGE_EVM_CHAINS_ROOT` | both scripts, optional | a chains root holding `hyperevm/config.json`, in place of `bridge/evm/chains` |

## Big blocks

HyperEVM needs one thing no other chain of the bridge needs. A contract deployment does not fit in a small block on this chain, so the deploying account must be switched to big blocks before the vault deployment will fit, and otherwise it runs out of block gas.

The configuration records this in its `big_blocks` section: `required` is `true` and `acknowledged` is `false` as committed. `bridge/deploy/deploy-evm-chain.sh` refuses to deploy to `hyperevm`, `--preflight` included, until `acknowledged` is `true`, and its refusal prints the requirement the configuration carries. Switch the deploying account to big blocks first, then set `acknowledged` to `true`.

## Before running the deploy script

- `owner` and the five `attestors` in `bridge/evm/chains/hyperevm/config.json` carry `PLACEHOLDER:` values, and every tool refuses them. Fill in the owner address and the attestor set, strictly ascending; the proposals generator later refuses a set or threshold that differs from `bridge/deploy/attestors.json`.
- The endpoint must answer `eth_chainId` with `999`; the script stops on any other chain id.
- `jq`, `forge`, `cast` and `git` are on the `PATH`.
- The deployer account holds enough HYPE to pay for the vault creation, the `setCap` call and the `transferOwnership` call the deployment makes.

```sh
bash bridge/deploy/deploy-evm-chain.sh --preflight hyperevm
bash bridge/deploy/deploy-evm-chain.sh hyperevm
bash bridge/deploy/verify-evm-chain.sh --preflight hyperevm
bash bridge/deploy/verify-evm-chain.sh hyperevm
```

The deployment hands the vault to the configured owner in two steps: the record reads `"ownership": "proposed"` until the owner calls `acceptOwnership()` on the vault.

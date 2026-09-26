# Avalanche C-Chain

PAX against AVAX. This page is the Avalanche C-Chain leg of the Paxeer X Network bridge: a AVAX deposit locked in the vault on Avalanche C-Chain is released as its bridged denom on Paxeer X Network, and a burn of that denom on Paxeer X Network releases the AVAX locked here. The operator runbook is `bridge/README.md`; this page lists what this chain needs.

| Field | Value |
| --- | --- |
| Chain name | `avalanche` |
| Chain id | `43114` |
| Native coin | `AVAX`, 18 decimals |
| Finality depth | `12` blocks |
| Attestors | 5, signing at a threshold of 3 |
| Vault | `PaxeerXVault`, the same bytecode every EVM chain of the bridge deploys |
| Configuration | `bridge/evm/chains/avalanche/config.json` |

## Assets

The configuration registers one asset, the native coin. It is the first asset of the file and is always registered, so the default pair works the moment the chain is opened.

| Asset | Address and asset id | Decimals | Per-transaction cap | Total cap |
| --- | --- | --- | --- | --- |
| AVAX | `0x0000000000000000000000000000000000000000` | 18 | `10000000000000000000000` (10,000 AVAX) | `200000000000000000000000` (200,000 AVAX) |

AVAX is deposited through `depositNative` and paid out by the vault's native release path. The caps are in base units and apply to the vault on this chain; the same values go into the `MsgSetCap` body for this asset on the Paxeer side.

## Environment variables

The configuration names the variables; neither the configuration nor the scripts carry a value for any of them.

| Variable | Read by | Holds |
| --- | --- | --- |
| `PAXEER_BRIDGE_AVALANCHE_RPC_URL` | `bridge/deploy/deploy-evm-chain.sh` | the Avalanche C-Chain endpoint |
| `PAXEER_BRIDGE_AVALANCHE_DEPLOY_KEY` | `bridge/deploy/deploy-evm-chain.sh` | the deployer key |
| `PAXEER_BRIDGE_AVALANCHE_EXPLORER_KEY` | `bridge/deploy/verify-evm-chain.sh` | the explorer verification key |
| `PAXEER_BRIDGE_DEPLOYMENT_RECORD` | both scripts | the deployment record: written by the deploy, read by the verification |
| `PAXEER_BRIDGE_EVM_CHAINS_ROOT` | both scripts, optional | a chains root holding `avalanche/config.json`, in place of `bridge/evm/chains` |

## Before running the deploy script

- `owner` and the five `attestors` in `bridge/evm/chains/avalanche/config.json` carry `PLACEHOLDER:` values, and every tool refuses them. Fill in the owner address and the attestor set, strictly ascending; the proposals generator later refuses a set or threshold that differs from `bridge/deploy/attestors.json`.
- The endpoint must answer `eth_chainId` with `43114`; the script stops on any other chain id.
- `jq`, `forge`, `cast` and `git` are on the `PATH`.
- The deployer account holds enough AVAX to pay for the vault creation, the `setCap` call and the `transferOwnership` call the deployment makes.

```sh
bash bridge/deploy/deploy-evm-chain.sh --preflight avalanche
bash bridge/deploy/deploy-evm-chain.sh avalanche
bash bridge/deploy/verify-evm-chain.sh --preflight avalanche
bash bridge/deploy/verify-evm-chain.sh avalanche
```

The deployment hands the vault to the configured owner in two steps: the record reads `"ownership": "proposed"` until the owner calls `acceptOwnership()` on the vault.

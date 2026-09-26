# BNB Smart Chain

PAX against BNB. This page is the BNB Smart Chain leg of the Paxeer X Network bridge: a BNB deposit locked in the vault on BNB Smart Chain is released as its bridged denom on Paxeer X Network, and a burn of that denom on Paxeer X Network releases the BNB locked here. The operator runbook is `bridge/README.md`; this page lists what this chain needs.

| Field | Value |
| --- | --- |
| Chain name | `bnb` |
| Chain id | `56` |
| Native coin | `BNB`, 18 decimals |
| Finality depth | `30` blocks |
| Attestors | 5, signing at a threshold of 3 |
| Vault | `PaxeerXVault`, the same bytecode every EVM chain of the bridge deploys |
| Configuration | `bridge/evm/chains/bnb/config.json` |

## Assets

The configuration registers one asset, the native coin. It is the first asset of the file and is always registered, so the default pair works the moment the chain is opened.

| Asset | Address and asset id | Decimals | Per-transaction cap | Total cap |
| --- | --- | --- | --- | --- |
| BNB | `0x0000000000000000000000000000000000000000` | 18 | `1000000000000000000000` (1,000 BNB) | `20000000000000000000000` (20,000 BNB) |

BNB is deposited through `depositNative` and paid out by the vault's native release path. The caps are in base units and apply to the vault on this chain; the same values go into the `MsgSetCap` body for this asset on the Paxeer side.

## Environment variables

The configuration names the variables; neither the configuration nor the scripts carry a value for any of them.

| Variable | Read by | Holds |
| --- | --- | --- |
| `PAXEER_BRIDGE_BNB_RPC_URL` | `bridge/deploy/deploy-evm-chain.sh` | the BNB Smart Chain endpoint |
| `PAXEER_BRIDGE_BNB_DEPLOY_KEY` | `bridge/deploy/deploy-evm-chain.sh` | the deployer key |
| `PAXEER_BRIDGE_BNB_EXPLORER_KEY` | `bridge/deploy/verify-evm-chain.sh` | the explorer verification key |
| `PAXEER_BRIDGE_DEPLOYMENT_RECORD` | both scripts | the deployment record: written by the deploy, read by the verification |
| `PAXEER_BRIDGE_EVM_CHAINS_ROOT` | both scripts, optional | a chains root holding `bnb/config.json`, in place of `bridge/evm/chains` |

## Before running the deploy script

- `owner` and the five `attestors` in `bridge/evm/chains/bnb/config.json` carry `PLACEHOLDER:` values, and every tool refuses them. Fill in the owner address and the attestor set, strictly ascending; the proposals generator later refuses a set or threshold that differs from `bridge/deploy/attestors.json`.
- The endpoint must answer `eth_chainId` with `56`; the script stops on any other chain id.
- `jq`, `forge`, `cast` and `git` are on the `PATH`.
- The deployer account holds enough BNB to pay for the vault creation, the `setCap` call and the `transferOwnership` call the deployment makes.

```sh
bash bridge/deploy/deploy-evm-chain.sh --preflight bnb
bash bridge/deploy/deploy-evm-chain.sh bnb
bash bridge/deploy/verify-evm-chain.sh --preflight bnb
bash bridge/deploy/verify-evm-chain.sh bnb
```

The deployment hands the vault to the configured owner in two steps: the record reads `"ownership": "proposed"` until the owner calls `acceptOwnership()` on the vault.

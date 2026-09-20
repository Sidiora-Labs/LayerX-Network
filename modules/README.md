# Paxeer modules

Paxeer-specific chain modules live under this directory:

- `evm` — native EVM execution, address association, receipts, pointers, and precompile integration
- `epoch` — time-based hooks and epoch lifecycle management
- `mint` — inflation and native-token minting policy
- `oracle` — validator exchange-rate voting and price aggregation
- `store` — module-level store integration helpers
- `tokenfactory` — permissioned creation and management of native token denominations

Framework-provided modules remain under `sdk/x/`; interchain applications live
under `interchain/modules/`.

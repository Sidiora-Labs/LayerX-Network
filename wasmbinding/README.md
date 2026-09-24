# CosmWasm support

This package contains CosmWasm integration points.

This package provides first class support for:

- Queries
  - Oracle (exchange rates, TWAPs)
  - Epoch
  - Token Factory (denom authority metadata, denoms from creator)
  - EVM (ERC-20/721/1155 helpers, Pax/EVM address mapping, static calls, interface support)
  - Staking extension (unbonding delegations)
- Messages / Execution
  - EVM internal calls (`MsgInternalEVMCall`, `MsgInternalEVMDelegateCall`)
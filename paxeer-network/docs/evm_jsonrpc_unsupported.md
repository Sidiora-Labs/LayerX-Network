# Pax EVM JSON-RPC: explicitly unsupported methods

This page is Paxeer Network RPC documentation inside [Sidiora-Labs/LayerX-Protocol](https://github.com/Sidiora-Labs/LayerX-Protocol) (`paxeer-network/`). It is not a standalone repository.

Some Ethereum JSON-RPC methods are **registered** on Pax’s EVM endpoint but return a **JSON-RPC error** instead of a result, so clients and tools get a stable, documented failure (code **`-32000`**) rather than “method not found” (`-32601`).

## Methods

| Method | Typical `error.message` |
| -------- | ------------------------- |
| `eth_blobBaseFee` | `blobs not supported on this chain` |
| `eth_syncing` | `eth_syncing is not supported on Pax EVM RPC` |
| `eth_newPendingTransactionFilter` | `eth_newPendingTransactionFilter is not supported on Pax EVM RPC` |
| `debug_getRawBlock` | `debug_getRawBlock is not supported on Pax EVM RPC` |
| `debug_getRawHeader` | `debug_getRawHeader is not supported on Pax EVM RPC` |
| `debug_getRawReceipts` | `debug_getRawReceipts is not supported on Pax EVM RPC` |
| `debug_getRawTransaction` | `debug_getRawTransaction is not supported on Pax EVM RPC` |

## Behavior notes

- **`eth_syncing`** — Pax’s consensus model differs from Ethereum’s sync semantics; callers should not rely on this method.
- **`eth_newPendingTransactionFilter`** — Pax has instant finality and does not expose Ethereum-style pending tx filters on this RPC.
- **`debug_getRaw*`** — Raw RLP block/header/receipt/tx payloads are not served on this surface.

Integration coverage: each unsupported method has a dedicated `not-supported.iox` under `integration_test/evm_module/rpc_io_test/testdata/<method>/`.

For broader compatibility rules (pending tags, uncles, trie, PoW, etc.), see [`rpc/AGENTS.md`](../rpc/AGENTS.md).

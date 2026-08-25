# Paxeer Network

**EVM Layer 1 (chain ID `125`). The settlement and custody layer for LayerX.**

Paxeer is where LayerX checkpoints, custody, guarantor bonds, challenges, withdrawals, and emergency exits live. Ordinary LayerX activity stays on LayerX. Periodic checkpoints settle here so custody never leaves an L1 that can be replayed independently of the LayerX sequencer.

This directory is that network: the `paxd` node, EVM/RPC surface, chain modules, storage engines, and Paxeer-native contracts. It lives in the [Sidiora-Labs/LayerX-Protocol](https://github.com/Sidiora-Labs/LayerX-Protocol) monorepo next to LayerX so the two can be reviewed together. Co-location does not grant LayerX authority over Paxeer, or the reverse. Each side keeps its own build, release tags, and trust boundary.

LayerX is under active development and release qualification. Limited beta opens September 7. Source is available for inspection while that work finishes.

## How it sits next to LayerX

| Path | Owns |
| --- | --- |
| Repository root (`src/`, `include/`, `agent/`, `human/`, `platform/`, `programs/`, `interop/`) | LayerX execution: activities, `402LXP` balances, receipts, agent and human surfaces |
| [`contracts/`](../contracts/) at the repository root | LayerX settlement contracts deployed *on* Paxeer: custody, checkpoints, bonds, claims, disputes, exits |
| **This directory (`paxeer-network/`)** | Paxeer Network itself: `paxd`, EVM execution, JSON-RPC, chain modules, Docker, node distribution |
| [`spec/`](../spec/) | Normative LayerX specifications (KVX first) |

`402LXP` remains the only LayerX balance writer. There is no LayerX token. Paxeer is the custody domain; Ethereum and Solana mirrors in `interop/` are archives, not settlement venues.

The Cosmos-style chain identifier used by node distribution is `hyperpax_125-1` (EVM chain ID `125`). See [`hpx/`](hpx/).

## Layout

| Path | Purpose |
| --- | --- |
| `daemon/paxd/` | `paxd` node binary |
| `node/` | Application wiring, genesis, upgrades |
| `modules/` | Paxeer chain modules (`evm`, `epoch`, `mint`, `oracle`, `tokenfactory`) |
| `rpc/` | EVM JSON-RPC compatibility |
| `contracts/` | Paxeer-native Solidity (WPAX, pointers, precompile interfaces) — not the LayerX settlement contracts |
| `consensus/`, `sdk/`, `storage/` | Consensus, Cosmos SDK fork, storage engines |
| `docker/` | Local single-node and cluster compose |
| `hpx/` | Native `paxd` distribution and peer registry tooling |
| `docs/` | Subtree documentation (OpenAPI/Swagger, RPC notes) |

Go module: `github.com/sidiora-labs/paxeer-network`.

## Build and test

Paxeer is a separate Go module. From the **monorepo root**:

```sh
make paxeer-build
make paxeer-lint
make paxeer-test
make paxeer-ci
```

Those targets invoke this directory's Makefile. `make monorepo-ci` at the repository root composes the LayerX gate with `make paxeer-ci`. Paxeer releases use namespaced `paxeer-network/vX.Y.Z` tags.

From **this directory**:

```sh
make build    # ./build/paxd
make install  # go install ./daemon/paxd
make lint
make test
make ci       # lint + test
```

Local Docker cluster targets (`docker-cluster-start`, `run-local-node`, and the rest) are documented in [`docker/README.md`](docker/README.md) and the Makefile. Paths in those files are relative to `paxeer-network/`.

### Foundry

Paxeer-native contracts use Foundry (`foundry.toml` in this directory). From here:

```sh
forge install
forge build
```

See [`contracts/README.md`](contracts/README.md). LayerX settlement contracts at the repository root are a separate Solidity `0.8.27` tree.

A successful local run is development evidence. It is not authorization to deploy validators, move custody, or handle real assets.

## Documentation

Start in [`docs/`](docs/):

- [`docs/README.md`](docs/README.md) — subtree docs index and OpenAPI/Swagger generation
- [`docs/evm_jsonrpc_unsupported.md`](docs/evm_jsonrpc_unsupported.md) — EVM JSON-RPC methods that return a documented error

LayerX protocol behavior, including how checkpoints and custody bind to Paxeer, is specified under [`spec/layerx-protocol/`](../spec/layerx-protocol/).

## License

This subtree is published with the rest of the monorepo under the repository [`LICENSE`](../LICENSE): source-available for inspection and security review during qualification, with a broader license after that work completes.

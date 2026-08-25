# Paxeer Network Documentation

Technical documentation site for Paxeer Network (EVM L1 chain ID 125) - the settlement and custody layer for LayerX.

## About

This is a standalone Next.js documentation site built from source material in `../paxeer-network/`. It provides comprehensive technical documentation for:

- Network architecture and parameters
- Node operation and configuration
- JSON-RPC API reference
- Chain modules (EVM, epoch, mint, oracle, store, tokenfactory)
- Consensus, storage, and engine internals
- WASM support and precompiles
- Docker deployment

## Source Material

Documentation is derived from:
- `paxeer-network/README.md`
- `paxeer-network/Makefile`
- `paxeer-network/docs/`
- `paxeer-network/modules/`
- `paxeer-network/rpc/`
- `paxeer-network/consensus/`
- Other source directories

CSS extracted from https://paxeer.app.

## Development

```bash
npm install
npm run dev
```

Open http://localhost:3000

## Build

```bash
npm run build
```

Output: `out/` directory (static export)

## Architecture

- **Framework:** Next.js 14 with static export
- **Styling:** Custom CSS extracted from paxeer.app
- **Layout:** Custom sidebar navigation (no doc framework)
- **Pages:** TypeScript + React Server Components

## Claim Lock

- Paxeer = EVM L1 chain ID 125 for LayerX custody/checkpoints/bonds/challenges/exits
- LayerX base fee 5,000 µUSDX per activity (~½¢), congestion 1×–64×. NEVER "zero fees"
- LayerX limited beta Sept 7. No invented public LayerX RPC
- PAX = Paxeer gas. USDX = LayerX unit. USDL = Paxeer L1 asset behind it. No LayerX token
- Co-location ≠ shared authority

All content cites source files.

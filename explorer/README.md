# Paxeer X Network Explorer

This directory holds the Paxeer X Network block explorer: a fork of Blockscout,
imported from three upstream trees and deliberately kept apart from the rest of
the monorepo.

| Part | Upstream | Revision |
| --- | --- | --- |
| `backend/` | blockscout/blockscout | tag `v10.2.6`, `90f7dd8e9348123b74dfd23f69ab7da76191a820` |
| `frontend/` | blockscout/frontend | tag `v2.7.2`, `446c409eeb54274aab90ff371a705f9699cd84ef` |
| `services/` | blockscout/blockscout-rs | `934a80f42976bac8e13dc45770104a0ede8bd7d7` |

Only the `smart-contract-verifier` and `sig-provider` services were taken from
`blockscout-rs`, together with the shared `libs` workspace.

## Licence boundary

Everything under `explorer/` is GPL-3.0 (`LICENSE`), except the imported Rust
services under `services/`, which stay MIT (`services/LICENSE-MIT`). Nothing in
this directory is linked into, compiled with, or imported by the Apache-2.0
crates and Go packages in the rest of the repository: the explorer runs as its
own set of processes and reaches the node only over HTTP and JSON-RPC, so the
two licence domains never share a binary.

## Building

- Backend: `cd backend && mix deps.get && mix compile`, with the Elixir and
  Erlang versions from `backend/.tool-versions`.
- Frontend: `cd frontend && corepack enable && yarn install --frozen-lockfile &&
  yarn build`, with the Node version from `frontend/.nvmrc`.
- Services: `cargo check` inside `services/smart-contract-verifier` and
  `services/sig-provider`; both resolve their shared crates from
  `services/libs`.

`UPSTREAM.md` records the exact upstream revisions. Keep local changes to the
imported trees small so the fork can still be rebased onto later upstream
releases.

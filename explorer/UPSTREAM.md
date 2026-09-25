# Upstream revisions

The trees under `explorer/` are imported copies of Blockscout sources. They are
taken at the revisions below; every file is upstream content unless a later
commit in this repository says otherwise.

| Path | Upstream repository | Tag | Commit | Licence |
| --- | --- | --- | --- | --- |
| `backend/` | `blockscout/blockscout` | `v10.2.6` | `90f7dd8e9348123b74dfd23f69ab7da76191a820` | GPL-3.0 |
| `frontend/` | `blockscout/frontend` | `v2.7.2` | `446c409eeb54274aab90ff371a705f9699cd84ef` | GPL-3.0 |
| `services/` | `blockscout/blockscout-rs` | none | `934a80f42976bac8e13dc45770104a0ede8bd7d7` | MIT |

`934a80f42976bac8e13dc45770104a0ede8bd7d7` is the last `blockscout-rs` revision
that still carries `LICENSE-MIT`; the next commit,
`d1342aaa1ea8beeb3d55601a9989e680a5d79657`, removed it. The MIT text is kept at
`services/LICENSE-MIT`, and the GPL-3.0 text at `LICENSE`.

## What was changed during the import

- Upstream `.github/` directories were dropped from all three trees so that
  upstream workflows do not run in this repository's CI.
- From `blockscout-rs` only `smart-contract-verifier`, `sig-provider` and the
  shared `libs` workspace were kept; the other services were left behind. The
  kept workspaces have no path dependencies outside those directories.
- Upstream's per-chain environment presets (`frontend/configs/envs/.env.*`,
  `frontend/deploy/tools/envs-validator/test/.env.*`) and
  `backend/.devcontainer/.blockscout_config.example` are not tracked here: the
  repository-wide ignore rules keep `.env.*` files out of the publication set.
  Recreate them from upstream when a deployment needs them.
- `frontend/configs/envs/.env.vitest` is the one exception, tracked through a
  single negation of that path in the repository's ignore file, because the
  frontend unit tests read it before every suite. It carries upstream's own
  values - localhost placeholders and upstream's network name - rather than the
  product's, because the tracked frontend snapshots and specs assert exactly
  those values; the deployment presets under the same directory are where the
  product's own configuration lives.
- Nothing else was edited: the tracked files are byte-identical to upstream at
  the commits above.

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

## Running backend commands

`deploy/tools/mix-in-builder.sh` runs one mix command against `backend/` inside
the pinned Elixir builder image, so a compile, a format check and a test suite
behave the same wherever they run. Call it from the repository root:

```
explorer/deploy/tools/mix-in-builder.sh compile
explorer/deploy/tools/mix-in-builder.sh format --check-formatted
explorer/deploy/tools/mix-in-builder.sh test apps/explorer/test/explorer
```

It mounts `apps`, `config`, `rel`, `mix.exs`, `mix.lock`, `.formatter.exs` and
`.credo.exs` into the container, keeps the compiled artefacts in a named volume
so a rerun does not recompile the dependencies, starts a disposable PostgreSQL
16 sidecar and joins the mix container to its network namespace so both reach
the database over the same loopback interface, waits until the database accepts
a connection over that interface, installs the headless browser driver when the
run reaches `block_scout_web`, prints the command it resolved with the database
password redacted, and exits with the mix exit code. The sidecar and its volume
are removed when the command finishes and when it is interrupted.

The builder image is built on demand from
`deploy/tools/Dockerfile.elixir-builder`, which pins the Elixir and Erlang
versions the explorer build workflow uses. The script labels the image it
builds with the digest of that Dockerfile, `mix.lock` and the application
manifests, and builds again when the image is absent or when one of those
inputs has moved since, so a stale image is never reused. An image the script
did not build - one named with `--image`, say - carries no such digest, and the
script reports that it cannot tell rather than building over it.

Options come before the mix arguments; `--` ends them.

| Option | What it does |
| --- | --- |
| `--reuse-db` | reuse the sidecar a previous `--reuse-db` run left behind and leave it running at exit, so a suite can be run twice against one database |
| `--image <ref>` | run a different builder image reference |
| `--check` | resolve the container runtime, the image and the mounts, print the command that would run, and exit without starting anything |
| `-h`, `--help` | print the usage, including every variable below |

Every other invocation gets its own database on purpose: a second `mix test`
run against a database an earlier run already migrated trips an upstream
migration-cache defect, which is exactly what `--reuse-db` opts into when a
suite is deliberately run twice over one database.

| Variable | Default |
| --- | --- |
| `MIX_ENV` | `test` |
| `MIX_BUILD_PATH` | `/build`, where the named volume is mounted |
| `MIX_BUILD_VOLUME` | a name derived from the backend path, so two working trees never share one build |
| `CHAIN_TYPE` | `paxeer_x` |
| `ETHEREUM_JSONRPC_VARIANT` | `paxeer_x` |
| `PGUSER`, `PGPASSWORD` | `postgres`, the credentials `backend/apps/explorer/config/test.exs` expects |
| `MIX_IN_BUILDER_IMAGE` | the same override as `--image` |
| `MIX_IN_BUILDER_BROWSER_DRIVER` | unset, which lets the script decide from the mix arguments; `1` always installs the driver, `0` never does |

## Running the whole stack locally

`deploy/docker-compose.local.yml` brings up Postgres, the backend, the
frontend, the smart contract verifier and the signature provider, each built
from the sources in this directory. Four values have no sensible default and
come from your shell:

| Variable | What it is |
| --- | --- |
| `RPC_HTTP_URL` | JSON-RPC HTTP endpoint of the node to index |
| `RPC_WS_URL` | JSON-RPC websocket endpoint of the same node |
| `CHAIN_ID` | EIP-155 chain id of that network |
| `SECRET_KEY_BASE` | Phoenix signing secret, at least 64 bytes; `openssl rand -base64 48` produces one |

```
RPC_HTTP_URL=... RPC_WS_URL=... CHAIN_ID=... SECRET_KEY_BASE=... \
  docker compose -f explorer/deploy/docker-compose.local.yml up --build
```

The backend answers on port 4000 and the frontend on 3000; both are published
to the host and both have a health check, so `docker compose ps` says whether
the stack is actually serving. Leaving `CHAIN_ID` or `SECRET_KEY_BASE` empty
stops the affected container rather than starting it with a stand-in value.
Everything else the two applications read lives in
`deploy/env/backend.example.env` and `deploy/env/frontend.example.env`, which
the compose file loads directly.

The first build compiles the Elixir release, the Next.js bundle and two Rust
services from scratch and takes a long time; after that,
`docker compose -f explorer/deploy/docker-compose.local.yml up` reuses the
layers. To check the definition without building anything:

```
docker compose -f explorer/deploy/docker-compose.local.yml config
```

## Deploying

`deploy/railway/` holds the per-service build and deploy configuration for a
Railway project together with the variable names the backend and the frontend
need. `deploy/railway/README.md` also records the root directory and config
file path each service has to be given, because neither is expressible in
`railway.json`.

`deploy/tools/copy-blockscout-11-to-10.sh` copies the core chain tables out of
a Blockscout 11.x database into a freshly migrated 10.2.6 one, intersecting the
two schemas by column name instead of assuming they match. It takes both
connection strings from the environment; run it with `DRY_RUN=1` first to see
which columns each table would gain and lose.

## Test ratio and lint scripts

Every pull request that touches this directory, or the gate scripts under
`tools/explorer/`, runs two gates besides the builds: `explorer-lint`, one leg
per language, and `explorer-test-ratio`.
Neither is marked `continue-on-error` and neither is skipped by a condition, so
a red leg is a red pull request.

### The ratio rule

A change under `explorer/` carries at least as many test files as source files,
and every changed source file has a changed test that references it. A test
references a source file when it names its Elixir module, names something the
file exports, contains a fragment of its path, or sits where that source file's
test belongs - beside it under the same name, or under the umbrella `test/`
mirror of its `lib/` path.

Files are classified by name, and by content for Rust:

| Class | Files |
| --- | --- |
| source | an Elixir `.ex`; a TypeScript `.ts` or `.tsx` that is not a test; a Rust `.rs` that is not a test module |
| test | an Elixir `*_test.exs`; a TypeScript `*.test.ts`, `*.test.tsx`, `*.spec.ts`, `*.spec.tsx` or `*.pw.tsx`; a Rust file under a `tests/` directory or carrying a `#[cfg(test)]` module |
| neither | everything else - shell scripts, YAML workflows and compose files, JSON deployment definitions, environment presets, SVG marks and Markdown |

A file classified as neither carries no test requirement, and a range that
changes no explorer source file passes whatever else it carries.

### Running the ratio gate locally

`tools/explorer/test-ratio.sh` takes the range to measure. The pull request job
passes the merge base of the base branch and the head commit, which is what the
three-dot form computes:

```
tools/explorer/test-ratio.sh origin/main...HEAD
tools/explorer/test-ratio.sh HEAD~1..HEAD
tools/explorer/test-ratio.sh origin/main HEAD
```

It exits 0 when the ratio holds or the range changes no explorer source file, 1
when it does not and names every source file left without a referencing test,
and 2 when the arguments or the repository do not resolve.
`tools/explorer/tests/test-ratio-test.sh` is the gate's own test: it builds a
throwaway git history and asserts the exit code and the named files for a
satisfied ratio, an unsatisfied one, a test that references nothing changed, a
shell and documentation change and an empty range.

### Running the lint gates locally

One script per language, each running exactly what its leg of `explorer-lint`
runs, each from anywhere in the repository:

```
tools/explorer/lint-backend.sh
tools/explorer/lint-frontend.sh
tools/explorer/lint-services.sh
```

`lint-backend.sh` runs `mix format --check-formatted` and `mix credo --strict`
through `deploy/tools/mix-in-builder.sh`, so the container, the toolchain and
the mix invocations are the ones the job uses; it runs both checks even when the
first fails and exits with the first non-zero code, naming the failing command
and its log. `lint-frontend.sh` installs the frozen lockfile and runs
`yarn lint:eslint` and `yarn lint:tsc` from `frontend/`. `lint-services.sh` runs
`cargo fmt --all --check` and `cargo clippy --all-targets --locked -- -D warnings`
in each Rust service, installing the protocol-buffer compiler and the OpenAPI
plugin the builds generate from if they are absent.

### Scoped upstream exemptions

A lint finding in a file this fork owns is fixed in the code. A finding
inherited from vendored upstream code, where fixing it would mean a mass edit of
files the fork takes from upstream, is silenced only by a scoped configuration
entry, and that entry carries three things: the exact upstream path or glob it
covers, the single rule or check it covers, and a one-line reason recorded in
the configuration file itself - a `.credo.exs` check exclusion, an eslint
override with an explicit file glob, a `clippy` attribute on the vendored
module. A rule is never disabled globally, never disabled on a fork-owned file,
and a job is never marked `continue-on-error` to carry a finding past the gate.
The test ratio has no exemption: a source file the fork changes arrives with a
test that references it.

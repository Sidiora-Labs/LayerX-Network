# Contributing to LayerX

LayerX is security-critical accounting and settlement software. Contributions must preserve deterministic replay, conservation of value, explicit authority, and fail-closed behavior.

This project is licensed under Apache 2.0. By contributing you agree that your work is licensed under those terms. Use Developer Certificate of Origin sign-off on every commit:

```sh
git commit -s
```

See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). Report vulnerabilities only through [SECURITY.md](SECURITY.md).

## Set up

Build and test from the repository root with the [Makefile](Makefile). A development container definition belongs under `.devcontainer/` when that directory is present.

Toolchain:

- C17 for the protocol runtime (`-std=c17` in the Makefile)
- Rust 1.91.1 (`rust-toolchain.toml`)
- Solidity 0.8.27 (`foundry.toml`)
- Further qualification compilers and runners in [docs/QUALIFICATION.md](docs/QUALIFICATION.md)

Useful targets:

```sh
make build
make test
make test-contracts
make ci
```

Paxeer Network is a separate Go module under `paxeer-network/` (`make paxeer-build`, `make paxeer-lint`, `make paxeer-test`, `make paxeer-ci`). See [docs/MONOREPO.md](docs/MONOREPO.md).

## Spec first

Protocol behavior is defined by the normative KVX sources under [`spec/`](spec/). Generated Markdown next to those sources is a reading aid, not the place to start a change. Protocol work needs an explicit requirement and task in the relevant `spec.kvx` before implementation.

Do not hand-edit generated files such as `AGENTS.md` or the Markdown mirrors of KVX.

## Branches and pull requests

- Branch from `main`.
- Keep the change narrow and tied to a requirement or a reproducible defect.
- Complete the pull-request template. Name the affected requirement and task when the change is specified.
- Record the exact verification commands you ran and their outcomes.
- Call out any required gate you could not run.

Reviewers may ask for negative tests, replay vectors, or migration analysis when a change crosses a trust boundary.

## Tests

Run the narrowest affected target while iterating, then the applicable broad gates before review. Arithmetic, replay, recovery, settlement, and cross-architecture changes have extra gates in [docs/QUALIFICATION.md](docs/QUALIFICATION.md).

A local result is not a production certification. No contribution authorizes deployment, validator mutation, custody migration, or a real-value canary.

## No weakening, no fakes

- Do not relax an assertion, loosen a bound, widen a type, add a silent fallback, skip or delete a test, or disable a check to make something pass. If the code and a check disagree, leave both intact and explain the conflict in the pull request.
- Do not add stub, mock, placeholder, or fake implementations to satisfy tests. Test real code paths with real types.
- Preserve canonical byte encodings and result-code assignments.
- Use checked fixed-width integer arithmetic on consensus-critical paths. Floating point is prohibited in transition functions.
- Keep `402LXP` as the only balance writer. Modules emit validated transfer sets.
- Keep network, wall-clock, filesystem enumeration, and database iteration order outside deterministic state transitions.
- Reject malformed or non-canonical input explicitly and transactionally.

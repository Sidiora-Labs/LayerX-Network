# Governance

LayerX Network is maintained by the Sidiora Labs core team. This document describes how decisions are made, how maintainers are added, and how protocol changes move through the specification.

## Maintainers

Maintainers are the Sidiora Labs core team. They:

- merge pull requests to `main`
- cut releases and tags (`vX.Y.Z` for LayerX, `paxeer-network/vX.Y.Z` for Paxeer)
- operate private security intake described in [SECURITY.md](SECURITY.md)
- enforce [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)

Maintainer status is not implied by a merged contribution.

## Decisions

Routine changes merge after review against [CONTRIBUTING.md](CONTRIBUTING.md). Protocol, consensus, custody, or trust-boundary changes need an explicit requirement and task in `spec/` before implementation, and they need review from at least one maintainer who did not author the change.

If maintainers disagree, the Sidiora Labs core team resolves the dispute. The recorded outcome is the merged specification and code, not a side channel.

## Adding maintainers

The Sidiora Labs core team adds maintainers by consensus of the existing maintainers. Candidates have a record of review and implementation on this repository, follow the contribution rules, and accept the security and conduct responsibilities above. Removal follows the same process.

## Spec-first change process

1. Edit the relevant KVX source under [`spec/`](spec/).
2. Regenerate derived Markdown and IDE rule files from that source.
3. Implement against the stated acceptance criteria with real code paths and tests.
4. Run the gates named by the task and by [docs/QUALIFICATION.md](docs/QUALIFICATION.md).
5. Open a pull request that cites the requirement and the commands that actually ran.

Generated files are not an independent source of truth. LayerX and Paxeer keep separate release identities; co-location in this monorepo does not grant one subsystem authority over the other. See [docs/MONOREPO.md](docs/MONOREPO.md).

## Security exceptions

Suspected vulnerabilities skip the public spec-and-PR path. They follow [SECURITY.md](SECURITY.md): private report, coordinated remediation, then a public change once disclosure is agreed. A security fix may land on `main` before the matching specification text is regenerated, but the specification must be updated in the same disclosure window so the published behavior and the KVX source do not diverge.

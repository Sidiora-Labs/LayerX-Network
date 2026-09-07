# Security Policy

LayerX handles authorization, accounting, custody commitments, and emergency exit evidence. Treat any defect that can affect value conservation, authority, finality, data availability, replay determinism, withdrawal uniqueness, upgrade safety, or private key handling as a security issue.

## Supported branches

The supported branch is `main`. Fixes land there. Availability of source or a passing test suite is not a deployment recommendation.

## Report privately

Do not open a public issue, discussion, or pull request for a suspected vulnerability. Use GitHub's private vulnerability reporting flow from the repository Security tab. Include:

- the affected commit or source revision and component
- prerequisites and an end-to-end reproduction
- the violated invariant and plausible impact
- whether exploitation requires sequencer, guarantor, governance, or custody privileges
- any proposed mitigation, test vector, or proof obligation

Do not probe public infrastructure, validators, custody deployments, user data, or third-party systems. Use only systems and assets you own or have explicit written authorization to test. Do not move funds, publish exploit details, or retain secrets obtained during research.

## Coordinated disclosure

The maintainers will validate scope and coordinate remediation and disclosure through the private report. Public discussion happens only after a fix is available or the maintainers agree the report is out of scope.

## Bounty

There is no bounty.

## Scope

The protocol's complete assumptions and exclusions are normative in the [threat model](spec/layerx-protocol/docs/threat-model.md). In particular:

- guarantor threshold signatures are economic attestations, not validity proofs
- the sequencer is a liveness and short-horizon ordering trust role
- emergency exit depends on the last finalized checkpoint and Paxeer contract availability
- local, sanitizer, fuzz, replay, and proof results do not replace independent contract review or a controlled deployment process

Conduct reports that are not product vulnerabilities follow [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) and use the same private reporting path to the maintainers.

# Visa Trusted Agent Protocol

`layerx-visa-tap` is the interoperability adapter for Visa Trusted Agent
credentials (`interop/crates/layerx-visa-tap`). The adapter id is
`visa-tap`. Visa publishes no standalone specification document; the crate
pins repository `README.md` at revision
`16d59bdf3f8a542bc538d0962edbb80ea30a02af`
(`interop/crates/layerx-visa-tap/src/lib.rs`).

The platform specification (`spec/layerx-platform/spec.kvx`, requirement 32
`ac_5`) requires verifying agent credentials per the published specification
and binding them to LayerX agent identities without granting them protocol
authority.

## What the crate binds

The crate distinguishes `AgentIntent::Browse` (`agent-browser-auth`) and
`AgentIntent::Pay` (`agent-payer-auth`). Signature label `sig2` and
components `("@authority" "@path")` are compiled in. `MAX_CLOCK_SKEW_SECONDS`
is `5 * 60`. Binding uses domain `LayerX/interop/visa-tap/binding/v1`.
Receipt verification uses `layerx_proof::receipt::verify` and
`AuthorizedBatch`.

Interop service deployment inputs — protocol network, authoritative module
registry, server-owned TAP clock skew, explicit trusted-agent status, and
per-principal canonical TAP targets — are documented in
`interop/deploy/gateway/README.md` (`interop/README.md`).

Adapters translate. They do not write balances
([Interop](index.md)).

## Status

The adapter crate, pinned vendor README, and tests under
`interop/crates/layerx-visa-tap/tests/` are in this tree. A public Visa TAP
merchant hostname is not named in the wiki set and is not invented here.

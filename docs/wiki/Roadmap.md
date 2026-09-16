# Roadmap: beta surface expansion

These seven items are **in development and not shipped**.

`spec/layerx-beta/spec.kvx` does not define implementation tasks named
8.1–8.7 for these surfaces. That file's `[req.8]` is Artifact Publication
and Provenance; its `reqs = ["8.1", …]` lists are acceptance criteria of
that requirement. The numbering 8.1–8.7 below is the documentation index
for the expansion surfaces named in this pass. Each row states what the
protocol, platform, and agent-interface specs and the current code actually
contain.

---

## 8.1 LXT721

**Status: in development, not shipped.**

The tree ships LXT-20 as an ABI-v2 guest example
(`programs/sdk/rust/src/lxt20.rs`, `programs/sdk/rust/examples/token-lxt20`).
Selectors are `4c581400`–`4c581407`. There is no LXT721 type, activity, or
requirement in `spec/layerx-protocol/spec.kvx`,
`spec/layerx-platform/spec.kvx`, or `spec/layerx-beta/spec.kvx`. No NFT
module ordinal exists in `include/layerx/lxp_module.h`.

See [Programs](Programs.md#lxt-20).

---

## 8.2 Multisig and timelock authority

**Status: in development, not shipped as a LayerX identity kind.**

Protocol identity is one primary key plus session keys and scoped grants
(`spec/layerx-protocol/spec.kvx` requirement 3). Rotation and recovery use
a challenge delay measured in batch timestamps. That is not a general
multisig threshold over several live keys.

Paxeer settlement contracts include timelock artifacts used by hosted
bootstrap (`spec/layerx-beta/spec.kvx` task notes describe an immediate-beta
zero-delay timelock profile beside the standard delay). Those contracts do
not add a LayerX activity type for multisig authority.

---

## 8.3 Naming

**Status: in development, not shipped as a name registry.**

Account names today are the canonical strings hashed under `LX:ACCOUNT:v1`
([Assets](Assets.md)). There is no protocol name-service module, no
`0x000xxxxx` naming activity, and no LNI `name_read` capability in
`agent/schema/lni/v1.kvx`. Platform requirement 2 is the simplicity
contract (banned vocabulary), not a human-readable name registry.

---

## 8.4 `oracle_read`

**Status: in development, not shipped as a Programs host function.**

Perps admits signed `ORACLE_PUSH` activities (`0x00060003`). Crossverse is
an outside adapter; execution never dials out
(`spec/layerx-protocol/spec.kvx` requirement 4 acceptance 5). Programs ABI
v2 has `context_read` and `balance_read`
(`programs/crates/layerx-programs-runtime/src/lib.rs` `ABI_MANIFEST`). It
does not import `oracle_read`.

---

## 8.5 Constant-product swap

**Status: in development, not shipped.**

No constant-product pool, swap activity, or AMM module appears in
`include/layerx/lxp_module.h`, `src/modules/`, or the four cited specs.
Value movement remains Asset send/receive, escrow, budget, stream, perps
margin, and Programs `402LXP` legs.

---

## 8.6 1024-asset registry

**Status: in development, not shipped.**

`LX_ASSET_REGISTRY_CAPACITY` is 64 (`include/layerx/lx_asset.h`). No spec
requirement in the cited files raises that bound to 1024. Do not treat 64
as 1024.

---

## 8.7 Self-custody key export

**Status: in development, not shipped. Out of v1 scope.**

`spec/layerx-platform/spec.kvx` decision `custody` and `v1_scope`, and
requirement 4 acceptance 10: human and managed-agent primary keys are
generated and held inside the human-service keystore. They never leave it.
The v1 service exposes no key-export path. Any future export must be
specified as its own feature. The qualification suite verifies that no API
surface returns key material.

---

## What is shipped today

The nine registered modules, the fee schedule versions 1–4, LNI 1.6,
the Agent API write/approval/subscription contract, and the 21 MCP tools
are documented on the pages linked from [Home](Home.md).

---

## Start here

- [Home](Home.md)
- [Modules](Modules.md)
- [Programs](Programs.md)
- [LNI](LNI.md)

# layerx-mcp

Tenant- and scope-bound Model Context Protocol tools for LayerX Network. A model gets the tools its bound scope allows. It does not get protocol authority.

Every call routes through `layerx-agentd`. There is no MCP-only write path and no tool-owned connection to the C17 core. Authority is fixed at server startup from an ordinary daemon session and capability.

This crate lives in the agent workspace (`agent/`). Related surfaces:

| Surface | Location |
| --- | --- |
| This server | `agent/crates/layerx-mcp` |
| Daemon | `agent/crates/layerx-agentd` |
| MCP / A2A as interop transports | [`interop/`](../../../interop/README.md) |
| `layerx install mcp` / `layerx mcp serve` | `platform/cli/` |

## Tools in this crate

Read tools are absent from the list when the bound scope does not include them. Wallet and token writes reuse the ordinary submit path; `activity.wait` uses the `Wait` operation and the existing receipt tracking stages. Operator walkthrough: [`docs/wiki/RunningAnAgent.md`](../../../docs/wiki/RunningAnAgent.md).

| Tool | Kind | Required scope | Daemon operation |
| --- | --- | --- | --- |
| `balance.get` | read | `read:balance` | `ReadBalance` |
| `wallet.balance` | read | `read:wallet:balance` | `ReadBalance` |
| `wallet.accounts` | read | `read:wallet:accounts` | `ReadAccount` |
| `history.list` | read | `read:history` | `ReadHistory` |
| `receipt.get` | read | `read:receipt` | `ProgramReceipt` |
| `checkpoint.get` | read | `read:checkpoint` | `ReadCheckpoint` |
| `proof.get` | read | `read:proof` | `ReadProofBundle` |
| `availability.get` | read | `read:availability` | `AvailabilityFetch` |
| `activity.prepare` | write | `write:prepare` | `Prepare` |
| `activity.disclose` | write | `write:disclose` | `Prepare` |
| `activity.sign` | write | `write:sign` | `Sign` |
| `activity.submit` | write | `write:submit` | `Submit` |
| `wallet.send` | write | `write:wallet:send` | `Submit` |
| `token.create` | write | `write:token:create` | `Submit` |
| `token.mint` | write | `write:token:mint` | `Submit` |
| `token.transfer` | write | `write:token:transfer` | `Submit` |
| `grant.issue` | write | `write:grant:issue` | `Submit` |
| `grant.draw` | write | `write:grant:draw` | `Submit` |
| `activity.track` | write | `write:track` | `Track` |
| `activity.wait` | write | `write:activity:wait` | `Wait` |

Write tools follow the ordinary daemon path: prepare, disclose, sign, submit, track. Outcomes are evidence-shaped (`Executed` + receipt, `Unknown`, or `Failed`). Read-only deployment omits write tools entirely. This catalogue is not the CLI `layerx mcp serve` surface (`receipt.get` / `activity.submit` only).

The wallet and token tools (`wallet.accounts`, `wallet.balance`, `wallet.send`, `token.create`, `token.mint`, `token.transfer`) are registered in `src/server.rs` and implemented in `src/tools/wallet.rs` and `src/tools/write.rs`. Payment walkthrough: [`docs/wiki/PaymentsQuickstart.md`](../../../docs/wiki/PaymentsQuickstart.md).

Untrusted tool arguments cannot change tenant, scope, or counterparty. See `src/untrusted.rs` and `src/validate.rs`. Payment payload bytes bound at disclose/sign live in `layerx-crypto` (`payments` / `disclosure`).

## Test

From the monorepo root:

```sh
make agent-test
```

Crate tests include `scope`, `read`, `write`, `approval`, `readonly`, and `injection` (`agent/tests/mcp/injection.rs`).

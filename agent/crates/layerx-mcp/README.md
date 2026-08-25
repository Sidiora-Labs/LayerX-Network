# layerx-mcp

Tenant- and scope-bound Model Context Protocol tools for LayerX. A model gets the tools its bound scope allows. It does not get protocol authority.

Every call routes through `layerx-agentd`. There is no MCP-only write path and no tool-owned connection to the C17 core. Authority is fixed at server startup from an ordinary daemon session and capability.

This crate lives in the agent workspace (`agent/`). Related surfaces:

| Surface | Location |
| --- | --- |
| This server | `agent/crates/layerx-mcp` |
| Daemon | `agent/crates/layerx-agentd` |
| MCP / A2A as interop transports | [`interop/`](../../../interop/README.md) |
| `layerx install mcp` / `layerx mcp serve` | `platform/cli/` |
| Tool design (normative) | [`spec/layerx-agent-interface/docs/mcp-tools.md`](../../../spec/layerx-agent-interface/docs/mcp-tools.md) |

## Tools in this crate

Read tools are absent from the list when the bound scope does not include them:

| Tool | Kind |
| --- | --- |
| `balance.get` | read |
| `history.list` | read |
| `receipt.get` | read |
| `checkpoint.get` | read |
| `proof.get` | read |
| `availability.get` | read |
| `activity.prepare` | write |
| `activity.disclose` | write |
| `activity.sign` | write |
| `activity.submit` | write |
| `activity.track` | write |

Write tools follow the ordinary daemon path: prepare, disclose, sign, submit, track. Outcomes are evidence-shaped (`Executed` + receipt, `Unknown`, or `Failed`). Read-only deployment omits write tools entirely.

Untrusted tool arguments cannot change tenant, scope, or counterparty. See `src/untrusted.rs` and `src/validate.rs`.

## Test

From the monorepo root:

```sh
make agent-test
```

Crate tests include `scope`, `read`, `write`, `approval`, `readonly`, and `injection` (`agent/tests/mcp/injection.rs`).

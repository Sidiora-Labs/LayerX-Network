# MCP

The agent-interface specification (`spec/.beta/layerx-agent-interface/spec.kvx`,
decision `mcp_scope`) requires MCP servers to expose the `agent-api` contract
as tools under an explicit scope. Read tools return verified core bytes. Write
tools prepare and submit canonical activities and return receipts. No MCP tool
returns a claimed outcome that is not backed by a receipt or proof.

There are two MCP surfaces in this repository (`interop/README.md`,
`agent/crates/layerx-mcp/README.md`):

| Surface | Role |
| --- | --- |
| `agent/crates/layerx-mcp` | Tenant- and scope-bound server. Every call routes through `layerx-agentd`. |
| Developer CLI | `layerx install mcp` and `layerx mcp serve` bind the same daemon session from a binding document. |

There is no standalone `layerx-a2a` crate. A2A is a transport on the
interoperability gateway and on x402, plus `layerx install a2a` /
`layerx a2a serve` in `platform/cli`. The gateway-bound A2A catalogue is a
different tool set (`receipt.get`, `activity.submit`, `faucet.request`) and
is not an MCP surface ([Running an agent](running.md)).

## Tools in `layerx-mcp`

The crate README lists these tools. Read tools are absent when the bound
scope does not include them.

| Tool | Kind | Required scope |
| --- | --- | --- |
| `balance.get` | read | `read:balance` |
| `wallet.balance` | read | `read:wallet:balance` |
| `wallet.accounts` | read | `read:wallet:accounts` |
| `history.list` | read | `read:history` |
| `receipt.get` | read | `read:receipt` |
| `checkpoint.get` | read | `read:checkpoint` |
| `proof.get` | read | `read:proof` |
| `availability.get` | read | `read:availability` |
| `activity.prepare` | write | `write:prepare` |
| `activity.disclose` | write | `write:disclose` |
| `activity.sign` | write | `write:sign` |
| `activity.submit` | write | `write:submit` |
| `wallet.send` | write | `write:wallet:send` |
| `token.create` | write | `write:token:create` |
| `token.mint` | write | `write:token:mint` |
| `token.transfer` | write | `write:token:transfer` |
| `grant.issue` | write | `write:grant:issue` |
| `grant.draw` | write | `write:grant:draw` |
| `activity.track` | write | `write:track` |
| `activity.wait` | write | `write:activity:wait` |
| `faucet.request` | write | `write:faucet:claim` |

Write tools follow prepare, disclose, sign, submit, and track. Outcomes are
evidence-shaped: executed plus receipt, unknown, or failed. Read-only
deployment omits write tools.

The payments developer path notes that burn, account-open, Asset info/list,
fee estimate, receipt wait, and live watch are not in this MCP catalogue
([Payments](../overview/payments.md)). `activity.wait` is in the crate
README above; treat that page's "not in the catalogue" list as the
payment-CLI overlap note, and this table as the crate inventory.

## Serving

`layerx-mcp <absolute path to a binding document>` binds the daemon session
the document names and serves the catalogue on a peer-credential admitted
Unix socket. `layerx mcp serve --daemon-binding <path>` serves the same
session on standard input and output. The binding document fields are in
`agent/crates/layerx-mcp/README.md`. The served path holds no signing seed
and no gateway credential.

Host values for `layerx install mcp` are `layerx`, `claude-code`,
`claude-desktop`, `cursor`, and `vscode` ([CLI](../platform/cli.md)).

## Interop ingress

MCP is also an ingress label on the interoperability gateway, next to `http`
and `a2a` (`interop/README.md`). That path translates foreign protocols; it
does not write balances. See [x402](../interop/x402.md).

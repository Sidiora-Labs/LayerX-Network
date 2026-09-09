# Running an agent

At the end of this path the operator has a `testnet` CLI profile, one
Ed25519 key in operating-system credential storage, a hosted gateway
key scoped to `receipt:read` and (unless `--read-only`)
`activity:write`, an MCP server registered into an agent-runtime host
document, one signed Asset SEND submitted through `activity.submit`,
and the JSON fields that tool returns. Daemon session credentials,
MCP crate write stages, protocol budget objects, Human-plane
approvals, and session revocation are library or Unix-peer
operations. Those steps name the function or opcode and the absence
of a `layerx` subcommand (`platform/cli/src/main.rs:42-80`).

This is the second developer path after the testnet quickstart. It
does not document emulator administration. MCP and A2A installation
refuse the emulator
(`platform/cli/src/install/mod.rs:573-577`).

Two MCP surfaces exist and do not share a tool catalogue or a
transport to core:

| Surface | Process | Tools | Authority |
| --- | --- | --- | --- |
| CLI install / `layerx mcp serve` | `layerx` stdio MCP (`platform/cli/src/mcp.rs:13-56`; `platform/cli/src/main.rs:439-455, 527-545`) | `receipt.get`, `activity.submit` (`platform/cli/src/toolset.rs:23-38, 104-116`) | Hosted gateway `Authorization: LayerX-Key` (`platform/cli/src/http.rs:50-64, 229-231`; `platform/cli/src/toolset.rs:95`) |
| `layerx-mcp` crate | `Server::bind` / `ReadOnly::bind` (`agent/crates/layerx-mcp/src/server.rs:348-363`; `agent/crates/layerx-mcp/src/readonly.rs:25-40`) | Eleven tools in `TOOL_CATALOGUE` (`agent/crates/layerx-mcp/src/server.rs:51-129`) | One daemon session and one capability (`agent/crates/layerx-mcp/README.md:3-6`; `agent/crates/layerx-mcp/src/server.rs:136-145, 390`) |

`layerx-mcp` has no binary (`agent/crates/layerx-mcp/Cargo.toml:1-19`).
Every crate tool call routes through `layerx-agentd`; there is no
MCP-only write path (`agent/crates/layerx-mcp/README.md:5-6`;
`agent/README.md:12-13`). The CLI server does not open that crate
server. It signs locally and posts JSON to `/v1/activities`
(`platform/cli/src/toolset.rs:190-204, 288-291`).

`layerx-agentd` is a library plus a binary
(`agent/crates/layerx-agentd/src/lib.rs:1`;
`agent/crates/layerx-agentd/src/main.rs:554-558`). The binary does
not load `StartupConfig` and does not run `Gate::new`. See
[Agentd](Agentd.md) for the library evidence, budget, and approval
paths. See [CLI](Cli.md) for credential storage and command
inventory.

---

## Point the CLI at testnet

Required command:

```
layerx environment use testnet --endpoint <url> --network-id <id> \
  --sequencer-trust-anchor <hex>
```

or `--sequencer-trust-anchor-file` in place of
`--sequencer-trust-anchor`
(`platform/cli/src/main.rs:96-107, 712-776`). The three bound
inputs must be supplied together or omitted together
(`platform/cli/src/emulator.rs:751-791`). Omitting them selects an
already-configured `testnet` profile
(`platform/cli/src/main.rs:741-760`). The name must be `emulator`,
`testnet`, or `production`
(`platform/cli/src/config.rs:121-126`).

Non-loopback endpoints must use `https://`
(`platform/cli/src/http.rs:27-37`). The testnet gateway Ingress host
in `layerx-testnet` is `api.testnet.layerx.network`
(`platform/hosted/gateway/deployment.yaml:182, 190-192`). The CLI
does not default that URL.

Success envelope kind `environment.selected`. Printed `data` fields:
`name`, `endpoint`, `network_id`, `sequencer_trust_anchor`
(`platform/cli/src/main.rs:768-775`). `--json` wraps
`{ok, kind, message, data}` (`platform/cli/src/output.rs:18-30`).

There is no `layerx` subcommand that starts `layerx-agentd`
(`platform/cli/src/main.rs:42-80`). The binary reads
`LAYERX_AGENT_*` environment keys and binds a loopback
program-balance listener plus a Human Unix owner
(`agent/crates/layerx-agentd/src/main.rs:50-54, 293-327, 498-558`).
Library handshake configuration uses `node_endpoint` as an absolute
normalised path, not an `https://` URL
(`agent/crates/layerx-agentd/src/config.rs:35-36, 69-70, 303, 474-488`;
`agent/crates/layerx-agentd/src/boot.rs:76-93`).

---

## Tenant credential and scope set

### Hosted gateway key (CLI command)

```
layerx key create <name>
printf '%s\n' "<identity-session>" | layerx auth set --environment testnet
```

`key create` stores a 32-byte OS-random Ed25519 seed under keyring
service `dev.layerx.cli` (`platform/cli/src/main.rs:111-117, 784-790`;
`platform/cli/src/credential.rs:11, 83-96`). Printed `data` fields:
`name`, `did`, `public_key` (`platform/cli/src/main.rs:786-789`).
`auth set` reads an API token from stdin
(`platform/cli/src/main.rs:135-140, 858-865`).

`layerx install mcp` then provisions `/v1/keys` (or
`/v1/keys/{id}/rotate`)
(`platform/cli/src/install/mod.rs:594-640`). Payment mode scopes:
`activity:write`, `receipt:read`. Read-only scope: `receipt:read`
(`platform/cli/src/install/mod.rs:587-593`). Gateway alias:
`{environment}:{component}:{mode}:{name}` with `component` `mcp` and
`mode` `payment` or `read` (`platform/cli/src/install/mod.rs:587-588`).
The issued secret stays in credential storage. Installed host JSON
is refused if a field name contains a secret marker
(`platform/cli/src/install/mod.rs:22-32, 892-900`).

Without `--key`, install uses the default key, else fallback name
`mcp`, else it creates `mcp`
(`platform/cli/src/install/mcp.rs:46`;
`platform/cli/src/install/mod.rs:735-755`). Without a stored identity
session: `no {environment} identity session is held in credential
storage; pipe one in with --token-stdin or run layerx auth set
--environment {environment}`
(`platform/cli/src/install/mod.rs:603-606`).

Hosted `layerx account create` requires `--email`,
`--display-name`, `--idempotency-key`; `--initial-amount` must be
`0` (`platform/cli/src/main.rs:154-167`;
`platform/cli/src/account.rs:42-54`).

### Daemon session (no CLI command)

There is no `layerx` command that opens an agentd session
(`platform/cli/src/main.rs:42-80`). `session::open` records
`OpenRequest` with `tenant`, `agent`, `authority`,
`permitted_activity_types`, `scopes`, `expiry_sequence`
(`agent/crates/layerx-agentd/src/session.rs:110-123, 515-539`).
`Capability::new` refuses any missing dimension
(`agent/crates/layerx-agentd/src/capability/mod.rs:47-94`).
`Server::bind` authenticates the bearer, restores that capability,
and keeps only catalogue scopes the session actually carries
(`agent/crates/layerx-mcp/src/server.rs:348-401, 192-206`).

The Human Unix owner can install that pair as opcode `OWNER_INSTALL`
(`23`) with `HumanOwnerInstall.scopes` and
`permitted_activity_types` (`agent/crates/layerx-agentd/src/human.rs:23, 124-144, 273`).
There is no `layerx` subcommand that speaks that socket
(`platform/cli/src/main.rs:42-80`).

---

## Start the MCP server and connect a runtime

Payment-capable install (hosted `testnet` or `production` only):

```
layerx install mcp --environment testnet --host <runtime> --key <name> \
  --source-account <64-hex> --asset <64-hex>
```

Flags: `--environment`, repeatable `--host`, `--key`, `--read-only`,
`--token-stdin`, `--rotate`, `--source-account`, `--asset`
(`platform/cli/src/main.rs:397-415, 596-612`). `--host` values:
`layerx`, `claude-code`, `claude-desktop`, `cursor`, `vscode`
(`platform/cli/src/install/mod.rs:53-64`). Empty `--host` selects
`layerx` plus every other host whose marker directory or config path
exists (`platform/cli/src/install/mod.rs:758-779`). Payment mode
requires `--source-account` and `--asset` as 64-hex
(`platform/cli/src/install/mcp.rs:142-156`). `--read-only` rejects
those two flags (`platform/cli/src/install/mcp.rs:143-146`).

The same process also serves stdio MCP:

```
layerx mcp serve --environment testnet --key <name> \
  --gateway-credential <alias> --source-account <64-hex> --asset <64-hex>
```

`--gateway-credential` is required. `--environment`, `--key`,
`--source-account`, `--asset`, `--read-only` are optional
(`platform/cli/src/main.rs:439-455, 527-545`).

Install success kind `install.mcp`
(`platform/cli/src/main.rs:607-612`). Human mode prints the message
then pretty `data` (`platform/cli/src/output.rs:31-39`). `data` is
this object (`platform/cli/src/install/mcp.rs:79-98`):

```
{
  "component": "mcp",
  "transport": "stdio",
  "environment": <selection.environment>,
  "endpoint": <selection.endpoint>,
  "network_id": <selection.network_id>,
  "deployment_mode": "full" | "read-only",
  "server": {
    "name": "layerx",
    "command": <current executable>,
    "args": [<launch arguments>],
    "env": {
      "LAYERX_CONFIG": <CLI config path>,
      "LAYERX_GATEWAY_KEY_ID": <gateway key id>
    }
  },
  "tools": [<descriptor>, ...],
  "scopes": [<required_scope>, ...],
  "credentials": {
    "key": <name>,
    "did": <did>,
    "public_key": <hex>,
    "created": <bool>,
    "storage": "operating-system-credential-store",
    "scope": "one environment and one key",
    "written_to_disk": false,
    "gateway_alias": <alias>,
    "gateway_key_id": <id>,
    "gateway_scopes": [<scope>, ...],
    "gateway_authorization": "LayerX-Key",
    "gateway_rotated": <bool>
  },
  "registrations": [<report>, ...],
  "changed": <bool>,
  "idempotent": true
}
```

`deployment_mode` is `toolset::mode_name`
(`platform/cli/src/toolset.rs:126-130`). `server.name` is
`SERVER_NAME` `"layerx"` (`platform/cli/src/install/mod.rs:20`;
`platform/cli/src/install/mcp.rs:87`).
`command` is `env::current_exe` canonicalized
(`platform/cli/src/install/mod.rs:782-790`). `env` starts as
`LAYERX_CONFIG` then adds `LAYERX_GATEWAY_KEY_ID`
(`platform/cli/src/install/mod.rs:792-799`;
`platform/cli/src/install/mcp.rs:53-56`). `credentials` is
`Selection::credentials` (`platform/cli/src/install/mod.rs:503-517`).
Each tool descriptor
(`platform/cli/src/toolset.rs:140-149`):

```
{
  "name": <tool.name>,
  "kind": "read" | "write",
  "scope": <tool.required_scope>,
  "mutation": <tool.mutation>,
  "evidence": <tool.evidence>,
  "arguments": <JSON Schema>
}
```

Each registration report (`platform/cli/src/install/mcp.rs:119-128`;
`platform/cli/src/install/mod.rs:870-878`):

```
{
  "path": <host path>,
  "section": "mcpServers" | "servers",
  "name": "layerx",
  "action": "created" | "updated" | "unchanged",
  "changed": <bool>,
  "permissions": "owner-only",
  "host": "layerx" | "claude-code" | "claude-desktop" | "cursor" | "vscode"
}
```

Launch `args` (`platform/cli/src/install/mcp.rs:163-191`):

```
mcp serve --environment <env> --key <key> --gateway-credential <alias>
  [--source-account <hex> --asset <hex>] [--read-only]
```

The host document entry written under section `mcpServers` (or
`servers` for `vscode`) is (`platform/cli/src/install/mod.rs:75-81, 103-117`):

```
{
  "command": <command>,
  "args": <args>,
  "env": <env>
}
```

`vscode` also inserts `"type": "stdio"`
(`platform/cli/src/install/mod.rs:110-112`). Host paths
(`platform/cli/src/install/mod.rs:83-91, 1112-1117, 1119-1125, 1142-1155`):

| `--host` | Path | Section |
| --- | --- | --- |
| `layerx` | parent of the CLI config file, `mcp.json` | `mcpServers` |
| `claude-code` | `{HOME}/.claude.json` | `mcpServers` |
| `claude-desktop` | `{XDG_CONFIG_HOME}/Claude/claude_desktop_config.json` (macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`) | `mcpServers` |
| `cursor` | `{HOME}/.cursor/mcp.json` | `mcpServers` |
| `vscode` | `{XDG_CONFIG_HOME}/Code/User/mcp.json` (macOS: `~/Library/Application Support/Code/User/mcp.json`) | `servers` |

`LAYERX_INSTALL_ROOT` replaces `HOME` for those roots
(`platform/cli/src/install/mod.rs:1119-1130`).

stdio `initialize` result fields: `protocolVersion` `2025-06-18`,
`capabilities.tools.listChanged` `false`, `serverInfo.name`
`layerx`, `serverInfo.title` `LayerX`, `serverInfo.version` crate
version, `instructions`, `_meta.layerx/deployment_mode`
(`platform/cli/src/mcp.rs:9-11, 96-107`). `tools/list` entries add
`description`, `inputSchema`, `annotations.readOnlyHint`,
`destructiveHint` `false`, `idempotentHint`, `openWorldHint` `true`,
`_meta.layerx/scope`, `_meta.layerx/mutation`, `_meta.layerx/evidence`
(`platform/cli/src/mcp.rs:110-132`).

`layerx install a2a` writes a different `data` object (`component`
`a2a`, `transport` `JSONRPC`) and is not the MCP host JSON
(`platform/cli/src/install/a2a.rs:40-119, 187-223`).

There is no command that starts the `layerx-mcp` crate server. Bind
it in-process with `Server::bind` or `ReadOnly::bind`
(`agent/crates/layerx-mcp/src/server.rs:348-363`;
`agent/crates/layerx-mcp/src/readonly.rs:25-40`).

---

## Grant a budget

There is no `layerx budget` command (`platform/cli/src/main.rs:42-80`).

`LocalLimit::new` constructs a daemon-only limit labelled
`daemon-enforced` with bypass statement `daemon-enforced only;
bypassing layerx-agentd bypasses this limit`
(`agent/crates/layerx-agentd/src/budget/create.rs:7-8, 95-106`).
`BudgetLimiter::new` takes `LimitConfig` values with scopes tenant,
agent, session, capability, counterparty
(`agent/crates/layerx-agentd/src/budget/reserve.rs:13-20, 23-29, 60-82`).
The binary installs one such `LimitConfig` from
`LAYERX_AGENT_HUMAN_LIMIT_*` (`agent/crates/layerx-agentd/src/main.rs:101-121, 255-261`).

`create_protocol_budget` submits a verifier-bound activity, verifies
the receipt, and returns `ProtocolObjectEffectUnavailable` on
success. It never returns `Ok`
(`agent/crates/layerx-agentd/src/budget/create.rs:122-162`).

The TypeScript and Python SDK examples call `client.call("budget.create",
request)` with `enforcement` `"ProtocolBudget"` or `"DaemonLimit"`
(`agent/sdk/typescript/examples/budget-constrained-spending.ts:3-10`;
`agent/sdk/python/examples/budget_constrained_spending.py:8-16`).
Those examples are not a CLI.

Human opcode `AGENT_LIMIT` (`27`) carries `agent_id`,
`monthly_limit`, `currency`, `replacement_budget_id`, `evidence`
(`agent/crates/layerx-agentd/src/human.rs:31, 1437-1443`). There is
no `layerx` subcommand for it.

---

## Read state

CLI MCP read tool: `receipt.get` with argument `activity_id`
(64-hex). It `GET`s `/v1/receipts/{activity_id}`
(`platform/cli/src/toolset.rs:25-30, 161-168, 190-198`).

`layerx receipt get <id>` is the same HTTP fetch outside MCP
(`platform/cli/src/main.rs:194-195, 956-964`).

Daemon MCP read tools `balance.get`, `history.list`, `receipt.get`,
`checkpoint.get`, `proof.get`, `availability.get` return
`VerifiedToolResult` with `value`, `verification_level`, `freshness`,
`page` (`agent/crates/layerx-mcp/src/server.rs:51-93`;
`agent/crates/layerx-mcp/src/tools/read.rs:101-108`). Read-only
deployment omits write tools
(`agent/crates/layerx-mcp/src/server.rs:29-32, 391-396`;
`agent/crates/layerx-mcp/README.md:35`).

The TypeScript and Python payment examples call `client.call("submit",
request)` and require `VerificationLevel.SequencerSigned` /
`VerificationLevel.SEQUENCER_SIGNED`
(`agent/sdk/typescript/examples/payment-with-verification.ts:12-19`;
`agent/sdk/python/examples/payment_with_verification.py:12-22`).
Offline receipt verification is `verifyReceipt` /
`verify_receipt` (`agent/sdk/typescript/examples/offline-receipt-verification.ts:8-11`;
`agent/sdk/python/examples/offline_receipt_verification.py:9-14`).

---

## Submit one signed activity

CLI write tool `activity.submit` required arguments:
`destination`, `amount`, `account_sequence`, `not_before_ms`,
`expires_at_ms`, `fee_limit`, `idempotency_key`
(`platform/cli/src/toolset.rs:169-185`). It builds a canonical Asset
SEND (`SEND_ACTIVITY` `5`), signs it, and `POST`s
`{"activity": <hex>}` to `/v1/activities`
(`platform/cli/src/toolset.rs:22, 207-298`). Validity window must be
non-empty and ≤ `300_000` ms
(`platform/cli/src/toolset.rs:21, 222-226`). Amount must be greater
than zero (`platform/cli/src/toolset.rs:216-218`).

Tool result fields (`platform/cli/src/toolset.rs:293-298`):

```
{
  "source": <64-hex>,
  "asset": <64-hex>,
  "idempotency_key": <64-hex>,
  "gateway": <gateway JSON>
}
```

stdio wraps that as `structuredContent` `{ "result": <value> }` with
`content[0].type` `text` and `isError` true when
`/gateway/state` or `/gateway/result/state` equals `"refused"`
(`platform/cli/src/mcp.rs:152-178`). Transport failure `gateway`
object: `state` `"unknown"` and `failure.code`
`gateway_transport_unavailable` (`platform/cli/src/http.rs:135-142`).
HTTP 4xx decode: `state` `"refused"`; other non-2xx: `state`
`"unknown"` (`platform/cli/src/http.rs:310-325`).

A `202` gateway body uses `ok`, `result.state` `"unknown"`,
`result.activity_id`, `result.idempotency_key`,
`result.retained_signed_activity`, `trace`
(`platform/hosted/gateway/src/main.rs:1987-2004`).

Daemon writes follow `ORDINARY_WRITE_STAGES`: Prepare, Disclose,
Policy, Sign, Submit, Track
(`agent/crates/layerx-mcp/src/tools/write.rs:10-28`).
`tools::write::execute` invokes tool name `activity.submit`
(`agent/crates/layerx-mcp/src/tools/write.rs:94-121`). Non-error
outcomes: `Executed` (`submission_ref`, `receipt`), `Unknown`
(`submission_ref`, `age_ms`), `Pending` (`submission_ref`, `state`)
(`agent/crates/layerx-mcp/src/tools/write.rs:64-78`).
`activity.prepare` / `disclose` / `sign` / `submit` / `track` are
separate catalogue tools (`agent/crates/layerx-mcp/src/server.rs:94-128`).

Human opcodes `PREPARE` `1`, `SUBMIT` `2`, `TRACK` `3`
(`agent/crates/layerx-agentd/src/human.rs:13-15`). There is no
`layerx` subcommand for them.

---

## Approve

The CLI MCP `activity.submit` path does not call the daemon approval
registry (`platform/cli/src/toolset.rs:190-204, 207-298`). There is
no `layerx approval` command (`platform/cli/src/main.rs:42-80`).

`layerx-mcp::approval::require` holds the prepared disclosure when
the summed amount exceeds `ApprovalPolicy.amount_threshold`
(`agent/crates/layerx-mcp/src/approval.rs:9-12, 36-68`).
`approve` / `reject` decide only the disclosure on the held ticket
(`agent/crates/layerx-mcp/src/approval.rs:71-113`).
`ApprovalService::approve` releases that preparation into
`ApprovalSubmissionQueue`
(`agent/crates/layerx-agentd/src/approval/mod.rs:379-486`).
Approvals are `DaemonOnly` and confer no protocol authority
(`agent/crates/layerx-agentd/src/approval/mod.rs:24-30`).

Human opcodes `APPROVAL_LIST` `9`, `APPROVAL_GET` `10`,
`APPROVAL_APPROVE` `11`, `APPROVAL_REJECT` `12`. Approve/reject
fields: `approval_id`, `held_digest`, `idempotency_key`,
`current_sequence` (`agent/crates/layerx-agentd/src/human.rs:18-21, 247-258, 1359-1378`).

---

## Observe the receipt

CLI: `receipt.get` as above, or `layerx receipt get <id>`
(`platform/cli/src/main.rs:194-195, 956-964`). Independent local
verify: `layerx receipt verify` with `--receipt`, `--batch-id`,
`--asset`, `--previous-state-root`, `--resulting-state-root`,
`--sequencer-public-key` (`platform/cli/src/main.rs:196-214, 966-978`).
It reports `verified: true` only after `verify_outcome` succeeds
(`platform/cli/src/receipt.rs:18-52`).

Daemon MCP `receipt.get` returns `ReceiptValue` with
`canonical_receipt` and non-empty `evidence_ids`
(`agent/crates/layerx-mcp/src/tools/read.rs:32-40, 144-178`).
`WriteOutcome::Executed.receipt` carries `receipt_ref`,
`canonical_receipt`, `verification_level`, `evidence_ids`
(`agent/crates/layerx-mcp/src/tools/write.rs:46-53, 66-69`).
Executed without that verified receipt is
`WriteToolError::SuccessWithoutVerifiedReceipt`
(`agent/crates/layerx-mcp/src/tools/write.rs:194-204`;
`agent/crates/layerx-mcp/tests/write.rs:269-272`).

Human opcode `RECEIPT_LOOKUP` `4` carries `idempotency_key` and
`expected_activity_id` (`agent/crates/layerx-agentd/src/human.rs:16, 233-236`).

---

## Observe revocation

There is no `layerx` command that revokes a session or prints a
revocation report (`platform/cli/src/main.rs:42-80`).

`session::close` advances generation and sets `open: false`
(`agent/crates/layerx-agentd/src/session.rs:549-569`).
`apply_revocation` closes matching open sessions from a core
`RevocationEvent` (`IdentityFrozen`, `PrimaryKeyRotated`,
`SessionKeyRevoked`, `CapabilityGrantRevoked`, `AccountRecovered`)
and reports `invalidated_sessions`, `invalidated_generations`,
`cancelled_preparations`, `unresolved_left_for_resolution`,
`executed_untouched`
(`agent/crates/layerx-agentd/src/session_revocation.rs:10-58, 61-131`).
Prepared and signed work is cancelled; queued/unknown work continues
resolution; executed/failed work is left untouched
(`agent/crates/layerx-agentd/src/session_revocation.rs:109-121`).

After close, MCP `activity.submit` and `activity.track` return
`WriteToolError::Server(ServerError::RevokedSession)` before any
write transcript (`agent/crates/layerx-mcp/src/server.rs:676-694`;
`agent/crates/layerx-mcp/tests/write.rs:276-325`).

Hosted gateway key revocation response fields: `ok`, `id`, `state`
`"revoked"` (`platform/hosted/gateway/src/main.rs:1124`). That is a
gateway key, not an agentd session.

Daemon event delivery to a local consumer is Unix frames `LXOW` /
`LXOA`, not an HTTP webhook
(`agent/crates/layerx-agentd/src/events/outbound.rs:1, 21-23`).
`CONSUMER_DEDUPLICATION_OBLIGATION` requires deduplication by
`deduplication_id` (`agent/crates/layerx-agentd/src/events/deliver.rs:26-28`).
There is no CLI command that registers that endpoint.

---

## MCP tools

### CLI / `layerx mcp serve`

| Name | Scope | Read/write |
| --- | --- | --- |
| `receipt.get` | `receipt:read` | read |
| `activity.submit` | `activity:write` | write |

(`platform/cli/src/toolset.rs:23-38, 104-116`). Read-only mode drops
`activity.submit`. An empty surface is refused
(`platform/cli/src/toolset.rs:107-114`).

### `layerx-mcp` crate

| Name | Scope | Read/write |
| --- | --- | --- |
| `balance.get` | `read:balance` | read |
| `history.list` | `read:history` | read |
| `receipt.get` | `read:receipt` | read |
| `checkpoint.get` | `read:checkpoint` | read |
| `proof.get` | `read:proof` | read |
| `availability.get` | `read:availability` | read |
| `activity.prepare` | `write:prepare` | write |
| `activity.disclose` | `write:disclose` | write |
| `activity.sign` | `write:sign` | write |
| `activity.submit` | `write:submit` | write |
| `activity.track` | `write:track` | write |

(`agent/crates/layerx-mcp/src/server.rs:51-129`;
`agent/crates/layerx-mcp/README.md:21-34`). Mapped daemon operations:
`ReadBalance`, `ReadHistory`, `ProgramReceipt`, `ReadCheckpoint`,
`ReadProofBundle`, `AvailabilityFetch`, `Prepare`, `Sign`, `Submit`,
`Track` (`agent/crates/layerx-mcp/src/server.rs:624-637`).

---

## Agentd configuration keys

Library startup is one UTF-8 `key=value` file. Exact `LAYERX_*`
environment values override the file. No security-relevant value has
a default. Unknown `LAYERX_*` names are refused
(`agent/crates/layerx-agentd/src/config.rs:1-6, 64-65, 171-173`).

| File key | Environment key | Role |
| --- | --- | --- |
| `network_id` | `LAYERX_NETWORK_ID` | Non-zero network id |
| `node_endpoint` | `LAYERX_NODE_ENDPOINT` | Absolute normalised node path |
| `expected_protocol_version` | `LAYERX_EXPECTED_PROTOCOL_VERSION` | Occupancy protocol |
| `tenants` | `LAYERX_TENANTS` | Non-empty unique tenant ids |
| `policy_sources` | `LAYERX_POLICY_SOURCES` | One absolute path per tenant |
| `signer_configurations` | `LAYERX_SIGNER_CONFIGURATIONS` | One absolute path per tenant |
| `verification_defaults` | `LAYERX_VERIFICATION_DEFAULTS` | Per-tenant `sequencer-signed`, `batch-included`, `state-proven`, `checkpoint-finalised`, or `settlement-anchored` |
| `sequencer_authority_source` | `LAYERX_SEQUENCER_AUTHORITY_SOURCE` | Protected `layerx-sequencer-authority-v1` file |

(`agent/crates/layerx-agentd/src/config.rs:29-62, 295-320, 438-471`).

The binary reads a disjoint `LAYERX_AGENT_*` set
(`agent/crates/layerx-agentd/src/main.rs:50-54, 293-327`):

| Key | Role |
| --- | --- |
| `LAYERX_AGENT_PROGRAM_LISTEN` | Loopback `127.0.0.1:<port>` listener |
| `LAYERX_AGENT_PROGRAM_BEARER_TOKEN` | Program-balance bearer; length ≥ 32; distinct from the other two bearers |
| `LAYERX_AGENT_NODE_BEARER_TOKEN` | Node bearer |
| `LAYERX_AGENT_AUTHORITY_BEARER_TOKEN` | Authority bearer |
| `LAYERX_AGENT_NODE_ENDPOINT` | Node HTTP endpoint for program-balance reads |
| `LAYERX_AGENT_AUTHORITY_ENDPOINT` | Authority HTTP endpoint |
| `LAYERX_AGENT_AUTHORITY_REPLICA_ID` | 32-byte hex replica id |
| `LAYERX_AGENT_SEQUENCER_TRUST_HISTORY` | Protected sequencer trust history path |
| `LAYERX_AGENT_PROGRAM_MAX_STALENESS_MS` | Non-zero staleness bound |
| `LAYERX_AGENT_DEPLOYMENT_JOURNAL` | Directory of `*.admission` proofs |
| `LAYERX_AGENT_PROGRAM_PROBE_ID` | Probe program id |
| `LAYERX_AGENT_HUMAN_NODE_LNI` | Absolute Human LNI path |
| `LAYERX_AGENT_HUMAN_STORE` | Absolute store path |
| `LAYERX_AGENT_HUMAN_SOCKET` | Absolute Human Unix socket |
| `LAYERX_AGENT_HUMAN_SESSION_KEY_ROOT` | Absolute session-key root |
| `LAYERX_AGENT_HUMAN_SESSION_OPERATOR_SECRET_FILE` | Protected operator secret |
| `LAYERX_AGENT_HUMAN_PEERS` | Comma-separated `uid=<u32>;tenant=<tenant>;principal=<did>` entries, e.g. `uid=4020;tenant=beta;principal=did:layerx:beta:alice`. Tenant: 1–128 ASCII letters, digits, `-` or `_`. Values cannot contain `;`, commas, whitespace or control characters. Principals require `did:<method>:<id>` within the protocol DID byte bound. Positional entries and duplicate UIDs are refused with a zero-based entry index. |
| `LAYERX_AGENT_HUMAN_LIMIT_ID` | 16-byte `LimitId` |
| `LAYERX_AGENT_HUMAN_LIMIT_SCOPE` | `tenant` / `agent` / `session` / `capability` / `counterparty` |
| `LAYERX_AGENT_HUMAN_LIMIT_SCOPE_ID` | 32-byte hex scope identity |
| `LAYERX_AGENT_HUMAN_LIMIT_NAME` | Limit name |
| `LAYERX_AGENT_HUMAN_LIMIT_CEILING` | Ceiling |
| `LAYERX_AGENT_HUMAN_LIMIT_CONSUMED` | Consumed |
| `LAYERX_AGENT_HUMAN_NETWORK_ID` | Human handshake network id |
| `LAYERX_AGENT_HUMAN_PROTOCOL_VERSION` | Occupancy protocol |
| `LAYERX_AGENT_HUMAN_AUTHORITY_ENDPOINT` | Human authority endpoint |
| `LAYERX_AGENT_HUMAN_AUTHORITY_BEARER` | Human authority bearer |
| `LAYERX_AGENT_HUMAN_SOCKET_UID` / `SOCKET_GID` / `SOCKET_MODE` | Socket owner and mode |

Missing required values print `{name} is required`
(`agent/crates/layerx-agentd/src/main.rs:50-54`). Boot failure prints
`layerx-agentd: ` plus a redacted diagnostic and exits `2`
(`agent/crates/layerx-agentd/src/main.rs:554-558`).

`RejectionReason` for the library file: `Missing`, `Empty`,
`Duplicate`, `Unknown`, `InvalidInteger`, `UnsupportedProtocol`,
`InvalidTenant`, `InvalidPath`, `IncompleteTenantMap`,
`InvalidVerificationLevel`, `TooLarge`, `InvalidEncoding`,
`Unavailable`, `Unprotected`
(`agent/crates/layerx-agentd/src/config.rs:80-94`).

---

## Typed refusals the agent sees

### CLI stdio MCP

| Refusal | When |
| --- | --- |
| JSON-RPC `-32700` `the message is not valid JSON` | Non-JSON stdin line (`platform/cli/src/mcp.rs:64-68`) |
| `-32600` `the message did not name a method` | Missing `method` (`platform/cli/src/mcp.rs:72-77`) |
| `-32601` `method {method} is not implemented` | Unknown method (`platform/cli/src/mcp.rs:88-92`) |
| `-32602` `the call did not name a tool` | `tools/call` without `name` (`platform/cli/src/mcp.rs:141-143`) |
| `-32602` `tool {name} is not served by this deployment` | Name outside the bound surface (`platform/cli/src/mcp.rs:145-149`; `platform/cli/src/toolset.rs:200-203`) |
| `structuredContent.refusal` plus `tool`, `isError` true | `toolset::invoke` `Err` (`platform/cli/src/mcp.rs:157-160, 172-178`) |
| `isError` true on a `result` | `/gateway/state` or `/gateway/result/state` is `"refused"` (`platform/cli/src/mcp.rs:154-155, 164-169`) |
| `amount must be greater than zero` | Zero `amount` (`platform/cli/src/toolset.rs:216-218`) |
| `payment validity must be non-empty and no wider than 300000 milliseconds` | `expires_at_ms` window (`platform/cli/src/toolset.rs:21, 222-226`) |
| `the runtime has no payment source binding` / `asset binding` | Full mode missing install binding (`platform/cli/src/toolset.rs:208-213`) |
| `gateway credential alias … is absent; rerun layerx install for this runtime` | Missing stored gateway secret (`platform/cli/src/toolset.rs:58-61`) |

### `layerx-mcp` crate

| Type | Meaning |
| --- | --- |
| `ServerError::MissingSession` / `MissingCapability` | Bind-time records absent (`agent/crates/layerx-mcp/src/server.rs:381-389, 640-657`) |
| `ClosedSession` / `RevokedSession` | Session closed or generation advanced (`agent/crates/layerx-mcp/src/server.rs:168-169, 676-694`) |
| `TenantMismatch` / `CapabilityMismatch` | Session and capability disagree (`agent/crates/layerx-mcp/src/server.rs:171-175, 189-191`) |
| `ExpiredAuthority` | `core_sequence` at or past session or capability expiry (`agent/crates/layerx-mcp/src/server.rs:177-180`) |
| `NoScope` | No catalogue scope remains after filtering (`agent/crates/layerx-mcp/src/server.rs:182-205, 399-400`) |
| `ToolAbsent` | Unknown, out-of-scope, or non-matching read/write tool (`agent/crates/layerx-mcp/src/server.rs:483, 551-555, 583-587`; `agent/crates/layerx-mcp/src/readonly.rs:74-79`) |
| `InvalidInvocation` | Empty/oversized/NUL tool name or arguments > `1_048_576` (`agent/crates/layerx-mcp/src/server.rs:19-20, 457-462`) |
| `ValidationError::ScopeDenied` / `CounterpartyDenied` / `AuthorityOverride` | Untrusted arguments cannot change tenant, scope, or counterparty (`agent/crates/layerx-mcp/src/untrusted.rs:64-71, 81-125`) |
| `ReadToolError::Unverified` / `InvalidBounds` / `CursorMismatch` / `ResultTooLarge` / `MissingReceiptEvidence` | Read envelope refusals (`agent/crates/layerx-mcp/src/tools/read.rs:110-119`) |
| `WriteToolError::Stage(StageFailure)` | Named `WriteStage` with `FailureClass` `Refused` / `Unavailable` / `InvalidEvidence` / `Protocol` (`agent/crates/layerx-mcp/src/tools/write.rs:30-44, 80-87`) |
| `WriteToolError::SuccessWithoutVerifiedReceipt` | `Executed` without verified receipt evidence (`agent/crates/layerx-mcp/src/tools/write.rs:194-204`) |
| `LimitRefusal::Exceeded` | Names `limit`, `name`, `ceiling`, `consumed`, `held`, `requested` (`agent/crates/layerx-agentd/src/budget/reserve.rs:161-169, 203-211`) |
| `ApprovalError::DisclosureChanged` | Presented disclosure ≠ held ticket (`agent/crates/layerx-mcp/src/approval.rs:25-28, 139-140`) |

Sources: `docs/wiki/Agentd.md`, `docs/wiki/Cli.md`,
`agent/README.md`, `agent/crates/layerx-mcp/README.md`,
`agent/crates/layerx-mcp/src/`, `agent/crates/layerx-agentd/src/`,
`platform/cli/src/install/`, `platform/cli/src/toolset.rs`,
`platform/cli/src/mcp.rs`, `agent/sdk/typescript/examples/`,
`agent/sdk/python/examples/`.

[Home](Home.md)

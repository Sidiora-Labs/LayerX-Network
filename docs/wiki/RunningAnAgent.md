# Running an agent

At the end of this path the operator has a `testnet` CLI profile, one
Ed25519 key in operating-system credential storage, an agent daemon
that has enrolled and published a binding document, an MCP server
registered into an agent-runtime host document against that document,
and the daemon-bound tool catalogue reachable over stdio. The MCP path
holds no gateway key and no signing seed: every tool call is authorized
by the daemon the binding names
(`platform/cli/src/mcp.rs:6-16`;
`agent/crates/layerx-mcp/src/binding.rs:339`). Enrolment itself is a
library call, protocol budget objects, Human-plane approvals and
session revocation likewise. Those steps name the function or opcode
and the absence of a `layerx` subcommand
(`agent/crates/layerx-agentd/src/enrolment.rs:397-403`;
`platform/cli/src/main.rs:43-83`).

This is the second developer path after the testnet quickstart. It
does not document emulator administration. `layerx install a2a`
refuses the emulator
(`platform/cli/src/install/mod.rs:573-577`); `layerx install mcp`
reads no environment profile at all, only the binding document
(`platform/cli/src/install/mcp.rs:33-53`). The wallet, token, and
payment-agent surfaces are covered by the
[payments developer path](PaymentsQuickstart.md).

The two MCP entry points share one catalogue and one transport to
core. The CLI process opens the binding document agent-daemon
enrolment published and binds the same daemon session the crate binds
in process:

| Entry point | Process | Tools | Authority |
| --- | --- | --- | --- |
| CLI install / `layerx mcp serve` | `layerx` stdio MCP over one binding document (`platform/cli/src/mcp.rs:11-22`; `platform/cli/src/main.rs:452-460, 559-565`) | The daemon catalogue the binding's mode admits (`platform/cli/src/toolset.rs:146-153`; `agent/crates/layerx-mcp/src/catalogue.rs:385-391`) | One daemon session and one capability, restored from the binding (`agent/crates/layerx-mcp/src/binding.rs:339`) |
| `layerx-mcp` crate | `Server::bind` / `ReadOnly::bind` (`agent/crates/layerx-mcp/src/server.rs:421-436`; `agent/crates/layerx-mcp/src/readonly.rs:25-41`) | Twenty-one tools in `TOOL_CATALOGUE` (`agent/crates/layerx-mcp/src/server.rs:60-203`) | One daemon session and one capability (`agent/crates/layerx-mcp/README.md:3-6`) |

The gateway-bound, locally signing surface is no longer an MCP
surface: `receipt.get`, `activity.submit` and `faucet.request` over
`Authorization: LayerX-Key` are now reached only through
`layerx a2a serve` (`platform/cli/src/toolset.rs:24-40, 42-108`;
`platform/cli/src/a2a.rs:89-176`).

`layerx-mcp` has no binary (`agent/crates/layerx-mcp/Cargo.toml:1-19`).
Every tool call routes through `layerx-agentd`; there is no MCP-only
write path (`agent/crates/layerx-mcp/README.md:5-6`;
`agent/README.md:12-13`). The CLI server opens that crate's bound
session: it reads the binding document named on its launch line,
applies `--read-only`, and serves the session on standard input and
output (`platform/cli/src/mcp.rs:11-22`).

`layerx-agentd` is a library plus a binary
(`agent/crates/layerx-agentd/src/lib.rs:1`;
`agent/crates/layerx-agentd/src/main.rs:782-787`). The binary does
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
(`platform/cli/src/main.rs:99-110, 712-776`). The three bound
inputs must be supplied together or omitted together
(`platform/cli/src/emulator.rs:751-791`). Omitting them selects an
already-configured `testnet` profile
(`platform/cli/src/main.rs:745-764`). The name must be `emulator`,
`testnet`, or `production`
(`platform/cli/src/config.rs:121-126`).

Non-loopback endpoints must use `https://`
(`platform/cli/src/http.rs:27-37`). The testnet gateway Ingress host
in `layerx-testnet` is `api.testnet.layerx.network`
(`platform/hosted/gateway/deployment.yaml:182, 190-192`). The CLI
does not default that URL.

Success envelope kind `environment.selected`. Printed `data` fields:
`name`, `endpoint`, `network_id`, `sequencer_trust_anchor`
(`platform/cli/src/main.rs:772-779`). `--json` wraps
`{ok, kind, message, data}` (`platform/cli/src/output.rs:18-30`).

There is no `layerx` subcommand that starts `layerx-agentd`
(`platform/cli/src/main.rs:43-83`). The binary reads
`LAYERX_AGENT_*` environment keys and binds a loopback
program-balance listener plus a Human Unix owner
(`agent/crates/layerx-agentd/src/main.rs:61-67, 514-549, 720-787`).
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
service `dev.layerx.cli` (`platform/cli/src/main.rs:114-120, 784-790`;
`platform/cli/src/credential.rs:11, 83-96`). Printed `data` fields:
`name`, `did`, `public_key` (`platform/cli/src/main.rs:790-793`).
`auth set` reads an API token from stdin
(`platform/cli/src/main.rs:138-143, 858-865`).

`layerx install a2a` then provisions `/v1/keys` (or
`/v1/keys/{id}/rotate`)
(`platform/cli/src/install/mod.rs:594-640`). Payment mode scopes:
`activity:write`, `receipt:read`. Read-only scope: `receipt:read`
(`platform/cli/src/install/mod.rs:587-593`). Gateway alias:
`{environment}:{component}:{mode}:{name}` with `component` `a2a` and
`mode` `payment` or `read` (`platform/cli/src/install/mod.rs:587-588`;
`platform/cli/src/install/a2a.rs:55-65`).
The issued secret stays in credential storage. Installed host JSON
is refused if a field name contains a secret marker
(`platform/cli/src/install/mod.rs:22-32, 892-900`).

Without `--key`, that install uses the default key, else fallback name
`a2a`, else it creates `a2a`
(`platform/cli/src/install/a2a.rs:60`;
`platform/cli/src/install/mod.rs:735-755`). Without a stored identity
session: `no {environment} identity session is held in credential
storage; pipe one in with --token-stdin or run layerx auth set
--environment {environment}`
(`platform/cli/src/install/mod.rs:603-606`).

`layerx install mcp` provisions nothing. It reads no environment, no
key and no identity session, and writes an empty process environment
into the host document (`platform/cli/src/install/mcp.rs:33-56`).

Hosted `layerx account create` requires `--email`,
`--display-name`, `--idempotency-key`; `--initial-amount` must be
`0` (`platform/cli/src/main.rs:157-170`;
`platform/cli/src/account.rs:42-54`).

### Daemon enrolment and the binding document

There is no `layerx` command that opens an agentd session or writes a
binding document (`platform/cli/src/main.rs:43-83`). The agent daemon
writes it at boot instead. Setting `LAYERX_AGENT_MCP_BINDING_ROOT`
turns that enrolment on; every other `LAYERX_AGENT_MCP_*` key is then
required, and before the daemon accepts a request it resolves the
configured agent DID through the human authority, registers it against
the store, and enrols one session
(`agent/crates/layerx-agentd/src/main.rs:219-253, 265-339, 443-450,
720-724`). A restart that names the session it already enrolled keeps
the document it published; a session that is open without its document
is a boot refusal rather than a second enrolment
(`agent/crates/layerx-agentd/src/main.rs:318-329`;
`agent/crates/layerx-agentd/src/enrolment.rs:444-469`). The
human-owner-only mode refuses the key outright, because the document
names the program endpoint and probe program only the full agent mode
serves (`agent/crates/layerx-agentd/src/human_owner_mode.rs:77-82`).

`enrolment::enrol` is the library call underneath, and it is also the
call a hosted deployment reaches directly. It restores the
named capability for the identity's tenant, mints a 32-byte session
token from operating-system randomness, calls `session::open`, and
publishes the binding through a `BindingPublisher`; a session whose
binding cannot be published is closed again, and
`OrphanedSession` names a session that stayed open because that close
also failed (`agent/crates/layerx-agentd/src/enrolment.rs:397-442`).

`session::open` records `OpenRequest` with `tenant`, `agent`,
`authority`, `permitted_activity_types`, `scopes`, `expiry_sequence`
(`agent/crates/layerx-agentd/src/session.rs:110-123, 515-539`).
`Capability::new` refuses any missing dimension
(`agent/crates/layerx-agentd/src/capability/mod.rs:47-94`).
`Server::bind` authenticates the bearer, restores that capability,
and keeps only catalogue scopes the session actually carries
(`agent/crates/layerx-mcp/src/server.rs:421-436, 463-472`).

`BindingPublisher::publish` writes three files into one binding
directory (`agent/crates/layerx-agentd/src/enrolment.rs:27-31,
283-343`):

| File | Contents |
| --- | --- |
| `binding.json` | The document `Binding::open` parses |
| `session-token` | The hex-encoded minted session token |
| `daemon-bearer` | The daemon transport bearer |

The directory is created or narrowed to `0o700` and each file is
created `0o600` with `create_new`, so a pre-existing secret file is a
refusal rather than an overwrite; a partial write removes every file
it already created
(`agent/crates/layerx-agentd/src/enrolment.rs:317-335, 500-512,
542-549`).

The document names `mode`, `tenant`, `store`, `audit_root`,
`session_id`, `session_token_file`, `session_generation`,
`capability_id`, `core_sequence`, `deadline_ms`, `agent`
(`endpoint`, `bearer_file`, `probe_program`) and `limit` (`id`,
`name`, `scope`, `scope_id`, `ceiling`, `consumed`)
(`agent/crates/layerx-agentd/src/enrolment.rs:347-383`). The parser
accepts exactly those keys plus an optional `listener` object of
`socket`, `owner_uid`, `owner_gid`, `mode`, `admitted_uids`, and
refuses any other key
(`agent/crates/layerx-mcp/src/binding.rs:28-45, 227-229`). No token or
bearer byte is written into the document: it names the protected
files that hold them.

Publishing the identical document again leaves it in place and
reports `created: false`; a different document at the same path is
refused as `AlreadyPublished` rather than replaced
(`agent/crates/layerx-agentd/src/enrolment.rs:58-69, 294-307`).
`DaemonSurface::new` refuses an endpoint outside `127.0.0.1:` and a
bearer shorter than 32 bytes
(`agent/crates/layerx-agentd/src/enrolment.rs:133-159`).

The Human Unix owner can install that pair as opcode `OWNER_INSTALL`
(`23`) with `HumanOwnerInstall.scopes` and
`permitted_activity_types` (`agent/crates/layerx-agentd/src/human.rs:23, 124-144, 273`).
There is no `layerx` subcommand that speaks that socket
(`platform/cli/src/main.rs:43-83`).

---

## Start the MCP server and connect a runtime

Install runs after the daemon has enrolled, because the installer
opens the same binding document the served path opens:

```
layerx install mcp --host <runtime>
```

Flags: repeatable `--host`, `--read-only`, `--daemon-binding <path>`
(`platform/cli/src/main.rs:412-417, 420-427, 612-628`). `--host`
values: `layerx`, `claude-code`, `claude-desktop`, `cursor`, `vscode`
(`platform/cli/src/install/mod.rs:53-64`). Empty `--host` selects
`layerx` plus every other host whose marker directory or config path
exists (`platform/cli/src/install/mod.rs:758-779`). An unknown alias
is refused before the binding document is read
(`platform/cli/src/install/mcp.rs:34-41`;
`platform/cli/tests/install.rs:53-59`).

Without `--daemon-binding`, the installer resolves
`mcp/binding.json` beside the CLI configuration file; a relative
explicit path is anchored against the current directory so the
installed launch line never depends on the runtime's working
directory (`platform/cli/src/install/mcp.rs:148-169`). The installer
then opens that document exactly as the served path does and refuses
when it is absent, unreadable, malformed or refused:
`the daemon binding document at {path} could not be used: {detail};
agent-daemon enrolment writes it before the MCP server is installed`
(`platform/cli/src/install/mcp.rs:42-48`;
`platform/cli/tests/install.rs:81-89`).

There is no environment, key, gateway credential, source account or
asset flag on this command; each is refused by the parser
(`platform/cli/src/main.rs:420-427`;
`platform/cli/tests/install.rs:61-79`). `--read-only` narrows the
installed surface; without it the installer serves the mode the
binding declares (`platform/cli/src/install/mcp.rs:49-53`).

The same process serves stdio MCP from the same document:

```
layerx mcp serve --daemon-binding <path>
```

`--daemon-binding` is required; `--read-only` is optional
(`platform/cli/src/main.rs:452-460, 559-565`). The served path opens
the binding, applies `--read-only`, opens the daemon session and
serves it on standard input and output
(`platform/cli/src/mcp.rs:11-22`).

Install success kind `install.mcp`, message
`Installed the LayerX model context protocol server bound to the
agent daemon at {agent endpoint}`
(`platform/cli/src/main.rs:620-627`). Human mode prints the message
then pretty `data` (`platform/cli/src/output.rs:31-39`). `data` is
this object (`platform/cli/src/install/mcp.rs:79-103`):

```
{
  "component": "mcp",
  "transport": "stdio",
  "authorization": "agent-daemon",
  "deployment_mode": "full" | "read-only",
  "daemon_binding": {
    "path": <binding document path>,
    "tenant": <tenant>,
    "declared_mode": "full" | "read-only",
    "agent_endpoint": <127.0.0.1:port>,
    "store": <agent store path>,
    "session_generation": <u64>
  },
  "server": {
    "name": "layerx",
    "command": <current executable>,
    "args": [<launch arguments>],
    "env": {}
  },
  "tools": [<descriptor>, ...],
  "scopes": [<required_scope>, ...],
  "registrations": [<report>, ...],
  "changed": <bool>,
  "idempotent": true
}
```

There is no `environment`, `endpoint`, `network_id`, `credentials` or
`account_binding` field: the installed server holds no gateway key
and no payment binding.

`deployment_mode` is `toolset::mode_name` of the effective mode, and
`daemon_binding.declared_mode` is the mode the document itself
declares, so `--read-only` shows as a narrowing of a `full` binding
(`platform/cli/src/toolset.rs:132-137`;
`platform/cli/src/install/mcp.rs:49-53, 83-91`). The remaining
`daemon_binding` fields are read straight off the parsed document
(`agent/crates/layerx-mcp/src/binding.rs:295-317`). `server.name` is
`SERVER_NAME` `"layerx"` (`platform/cli/src/install/mod.rs:20`).
`command` is `env::current_exe` canonicalized
(`platform/cli/src/install/mod.rs:782-790`). `server.env` is empty:
no CLI configuration path and no gateway key identity reaches the
served process (`platform/cli/src/install/mcp.rs:24-25, 56`).
Each tool descriptor carries the catalogue description and argument
schema, and a tool without either is refused rather than registered
(`platform/cli/src/toolset.rs:155-170`):

```
{
  "name": <tool.name>,
  "kind": "read" | "write",
  "scope": <tool.required_scope>,
  "mutation": <tool.mutation>,
  "evidence": <tool.evidence>,
  "description": <catalogue description>,
  "arguments": <JSON Schema>
}
```

Each registration report (`platform/cli/src/install/mcp.rs:125-134`;
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

Launch `args` (`platform/cli/src/install/mcp.rs:180-191`):

```
mcp serve --daemon-binding <absolute binding document path> [--read-only]
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
version, `instructions`, and `_meta` carrying
`layerx/deployment_mode`, `layerx/binding` `agent-daemon`,
`layerx/read_tools`, `layerx/write_tools` and
`layerx/mutations_reachable`
(`agent/crates/layerx-mcp/src/stdio.rs:14-16, 156-174`).
`tools/list` entries carry `name`, `description`, `inputSchema`,
`annotations.readOnlyHint`, `destructiveHint` `false`,
`idempotentHint`, `openWorldHint` `true`, `_meta.layerx/scope`,
`_meta.layerx/mutation`, `_meta.layerx/evidence`
(`agent/crates/layerx-mcp/src/catalogue.rs:393-413`).

`layerx install a2a` writes a different `data` object (`component`
`a2a`, `transport` `JSONRPC`) and is not the MCP host JSON
(`platform/cli/src/install/a2a.rs:40-119, 187-223`).

`layerx mcp serve` is the command that starts the `layerx-mcp`
server from a binding document; in process the same server is bound
with `Server::bind` or `ReadOnly::bind`
(`platform/cli/src/mcp.rs:11-22`;
`agent/crates/layerx-mcp/src/binding.rs:339`;
`agent/crates/layerx-mcp/src/server.rs:421-436`;
`agent/crates/layerx-mcp/src/readonly.rs:25-41`).

---

## Grant a budget

There is no `layerx budget` command (`platform/cli/src/main.rs:43-83`).

`LocalLimit::new` constructs a daemon-only limit labelled
`daemon-enforced` with bypass statement `daemon-enforced only;
bypassing layerx-agentd bypasses this limit`
(`agent/crates/layerx-agentd/src/budget/create.rs:7-8, 95-106`).
`BudgetLimiter::new` takes `LimitConfig` values with scopes tenant,
agent, session, capability, counterparty
(`agent/crates/layerx-agentd/src/budget/reserve.rs:13-20, 23-29, 60-82`).
The binary installs one such `LimitConfig` from
`LAYERX_AGENT_HUMAN_LIMIT_*` (`agent/crates/layerx-agentd/src/main.rs:114-135, 476-482`).

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

`layerx mcp serve` serves the daemon read tools `balance.get`,
`wallet.balance`, `wallet.accounts`, `history.list`, `receipt.get`,
`checkpoint.get`, `proof.get` and `availability.get`. It reaches no
hosted gateway route; `receipt.get` resolves through the daemon like
every other catalogue entry (`platform/cli/src/mcp.rs:11-22`).

`layerx receipt get <id>` is a separate hosted HTTP fetch outside MCP
(`platform/cli/src/main.rs:197-198, 956-964`).

Those eight read tools are catalogue entries authorized through the
daemon (`agent/crates/layerx-mcp/src/server.rs:60-203`;
`agent/crates/layerx-mcp/src/catalogue.rs:385-391`). Helpers
`balance`, `history`, `receipt`, `checkpoint`, `proof`, and
`availability` return `VerifiedToolResult` with `value`,
`verification_level`, `freshness`, `page`
(`agent/crates/layerx-mcp/src/tools/read.rs:103-108`). Read-only
deployment omits write tools, whether the narrowing came from the
binding document's own `mode` or from `--read-only`
(`agent/crates/layerx-mcp/src/server.rs:463-472`;
`agent/crates/layerx-mcp/src/binding.rs:319-321`;
`platform/cli/src/mcp.rs:13-15`;
`agent/crates/layerx-mcp/README.md:43`).

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

`layerx mcp serve` has no locally signing write path. `activity.submit`
over stdio is the daemon catalogue entry, validated against the
catalogue argument schema and then executed through the bound daemon
session (`agent/crates/layerx-mcp/src/stdio.rs:185-212`;
`agent/crates/layerx-mcp/src/server.rs:60-203`).

stdio wraps a success as `structuredContent`
`{ "tool": <name>, "result": <value> }` with `content[0].type` `text`
and `isError` `false`; a refusal is the same envelope with `isError`
`true` and a `refusal` object naming `tool`, `stage`
(`arguments`, `freshness` or `daemon`) and, for a daemon refusal,
`state` `"refused"` or `"unknown"`
(`agent/crates/layerx-mcp/src/stdio.rs:197-211, 257-263, 296-305`).

The locally signing, gateway-posting `activity.submit` with
`destination`, `amount`, `account_sequence`, `not_before_ms`,
`expires_at_ms`, `fee_limit` and `idempotency_key` now belongs to
`layerx a2a serve` alone
(`platform/cli/src/toolset.rs:183-199, 207-298`;
`platform/cli/src/a2a.rs:89-176`).

Daemon writes follow `ORDINARY_WRITE_STAGES`: Prepare, Disclose,
Policy, Sign, Submit, Track
(`agent/crates/layerx-mcp/src/tools/write.rs:10-28`).
`tools::write::execute` invokes tool name `activity.submit`
(`agent/crates/layerx-mcp/src/tools/write.rs:138-156`).
`execute_payment` uses the same stages for `wallet.send`,
`token.create`, `token.mint`, and `token.transfer`
(`agent/crates/layerx-mcp/src/tools/write.rs:89-131`).
`wait` uses the track stages under `activity.wait`
(`agent/crates/layerx-mcp/src/tools/write.rs:209-227`). Non-error
outcomes: `Executed` (`submission_ref`, `receipt`), `Unknown`
(`submission_ref`, `age_ms`), `Pending` (`submission_ref`, `state`)
(`agent/crates/layerx-mcp/src/tools/write.rs:64-78`).
`activity.prepare` / `disclose` / `sign` / `submit` / `track` and the
wallet, token, and wait tools are separate catalogue entries
(`agent/crates/layerx-mcp/src/server.rs:94-177`).

Human opcodes `PREPARE` `1`, `SUBMIT` `2`, `TRACK` `3`
(`agent/crates/layerx-agentd/src/human.rs:13-15`). There is no
`layerx` subcommand for them.

---

## Approve

No served path calls the approval hold. `layerx mcp serve` invokes the
daemon write tools directly through the bound session
(`agent/crates/layerx-mcp/src/stdio.rs:215-253`), and
`layerx-mcp::approval::require` is reached only by a caller that binds
it itself; no module of `layerx-mcp` outside `approval.rs` calls it.
There is no `layerx approval` command
(`platform/cli/src/main.rs:43-83`).

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
(`platform/cli/src/main.rs:197-198, 956-964`). Independent local
verify: `layerx receipt verify` with `--receipt`, `--batch-id`,
`--asset`, `--previous-state-root`, `--resulting-state-root`,
`--sequencer-public-key` (`platform/cli/src/main.rs:199-217, 966-978`).
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
(`agent/crates/layerx-mcp/src/tools/write.rs:291-301`;
`agent/crates/layerx-mcp/tests/write.rs:269-272`).

Human opcode `RECEIPT_LOOKUP` `4` carries `idempotency_key` and
`expected_activity_id` (`agent/crates/layerx-agentd/src/human.rs:16, 233-236`).

---

## Observe revocation

There is no `layerx` command that revokes a session or prints a
revocation report (`platform/cli/src/main.rs:43-83`).

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

After close, MCP write tools including `activity.submit`,
`wallet.send`, `token.create`, `token.mint`, `token.transfer`,
`activity.track`, and `activity.wait` return
`WriteToolError::Server(ServerError::RevokedSession)` before any
write transcript (`agent/crates/layerx-mcp/src/server.rs:729-745`;
`agent/crates/layerx-mcp/tests/write.rs:276-325`).

Hosted gateway key revocation response fields: `ok`, `id`, `state`
`"revoked"` (`platform/hosted/gateway/src/main.rs:1144`). That is a
gateway key, not an agentd session.

Daemon event delivery to a local consumer is Unix frames `LXOW` /
`LXOA`, not an HTTP webhook
(`agent/crates/layerx-agentd/src/events/outbound.rs:1, 21-23`).
`CONSUMER_DEDUPLICATION_OBLIGATION` requires deduplication by
`deduplication_id` (`agent/crates/layerx-agentd/src/events/deliver.rs:26-28`).
There is no CLI command that registers that endpoint.

---

## MCP tools

### `layerx mcp serve` and the `layerx-mcp` crate

Both entry points serve one catalogue of twenty-one tools, eight read
and thirteen write:

| Name | Scope | Read/write |
| --- | --- | --- |
| `balance.get` | `read:balance` | read |
| `history.list` | `read:history` | read |
| `receipt.get` | `read:receipt` | read |
| `checkpoint.get` | `read:checkpoint` | read |
| `proof.get` | `read:proof` | read |
| `availability.get` | `read:availability` | read |
| `wallet.accounts` | `read:wallet:accounts` | read |
| `wallet.balance` | `read:wallet:balance` | read |
| `activity.prepare` | `write:prepare` | write |
| `activity.disclose` | `write:disclose` | write |
| `activity.sign` | `write:sign` | write |
| `activity.submit` | `write:submit` | write |
| `activity.track` | `write:track` | write |
| `wallet.send` | `write:wallet:send` | write |
| `token.create` | `write:token:create` | write |
| `token.mint` | `write:token:mint` | write |
| `token.transfer` | `write:token:transfer` | write |
| `grant.issue` | `write:grant:issue` | write |
| `grant.draw` | `write:grant:draw` | write |
| `activity.wait` | `write:activity:wait` | write |
| `faucet.request` | `write:faucet:claim` | write |

(`agent/crates/layerx-mcp/src/server.rs:52-203`;
`agent/crates/layerx-mcp/README.md:20-41`). Read-only mode keeps the
read rows only, and a mode that would serve nothing is refused before
installation (`agent/crates/layerx-mcp/src/catalogue.rs:385-391`;
`platform/cli/src/toolset.rs:146-153`). `Server::bind` narrows the
result again to the scopes the bound session actually carries and
refuses an empty result with `NoScope`
(`agent/crates/layerx-mcp/src/server.rs:463-472`). Mapped daemon
operations: `ReadBalance`, `ReadAccount`, `ReadHistory`,
`ProgramReceipt`, `ReadCheckpoint`, `ReadProofBundle`,
`AvailabilityFetch`, `Prepare`, `Sign`, `Submit`, `Track`,
`FaucetClaim`, `Wait`
(`agent/crates/layerx-mcp/src/server.rs:697-715`).
Wallet, token, and grant writes alias `Submit`; `activity.wait` uses `Wait`.

### `layerx a2a serve`

The gateway-bound agent-to-agent surface is a different, smaller
catalogue: `receipt.get` (`receipt:read`), `activity.submit`
(`activity:write`) and `faucet.request` (`write:faucet:claim`).
Read-only mode drops the two write entries and an empty surface is
refused (`platform/cli/src/toolset.rs:24-40, 110-122`).

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
(`agent/crates/layerx-agentd/src/main.rs:46-59, 514-549`):

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

The model context protocol enrolment is one further group, read only
when `LAYERX_AGENT_MCP_BINDING_ROOT` is set; setting it makes every
other key in this group required
(`agent/crates/layerx-agentd/src/main.rs:168-253`):

| Key | Role |
| --- | --- |
| `LAYERX_AGENT_MCP_BINDING_ROOT` | Absolute binding directory the three published files land in |
| `LAYERX_AGENT_MCP_AUDIT_ROOT` | Absolute audit root the document names |
| `LAYERX_AGENT_MCP_PEER_UID` | UID of the `LAYERX_AGENT_HUMAN_PEERS` entry whose tenant and principal own the session |
| `LAYERX_AGENT_MCP_AGENT_DID` | Agent DID, resolved and verified through the human authority before registration |
| `LAYERX_AGENT_MCP_SESSION_ID` | 32-byte hex session id |
| `LAYERX_AGENT_MCP_CAPABILITY_ID` | 32-byte hex capability id restored for that tenant |
| `LAYERX_AGENT_MCP_ACTIVITY_TYPES` | Comma-separated permitted activity types; a repeated type is refused |
| `LAYERX_AGENT_MCP_SCOPES` | Comma-separated scopes; an empty or repeated scope is refused |
| `LAYERX_AGENT_MCP_EXPIRY_SEQUENCE` | Session expiry sequence |
| `LAYERX_AGENT_MCP_OPENING_CLIENT` | Opening client recorded on the session |
| `LAYERX_AGENT_MCP_POLICY_VERSION` | Policy version recorded on the session |
| `LAYERX_AGENT_MCP_CORE_SEQUENCE` | Core sequence written into the document |
| `LAYERX_AGENT_MCP_MODE` | `full` or `read-only`, the mode the document declares |
| `LAYERX_AGENT_MCP_DEADLINE_MS` | Non-zero transport deadline in milliseconds |
| `LAYERX_AGENT_MCP_LIMIT_ID` / `LIMIT_NAME` / `LIMIT_SCOPE` / `LIMIT_SCOPE_ID` / `LIMIT_CEILING` / `LIMIT_CONSUMED` | The one `LimitConfig` the document declares, with the same scope names as the human limit |

The daemon endpoint and probe program the document names are the ones
this binary already serves: `LAYERX_AGENT_PROGRAM_LISTEN`,
`LAYERX_AGENT_PROGRAM_BEARER_TOKEN` and `LAYERX_AGENT_PROGRAM_PROBE_ID`
(`agent/crates/layerx-agentd/src/main.rs:255-263`). `install mcp` then
reads that document, and neither it nor `mcp serve` needs any further
key (`platform/cli/src/install/mcp.rs:38-48`).

Missing required values print `{name} is required`
(`agent/crates/layerx-agentd/src/main.rs:61-67`). Boot failure prints
`layerx-agentd: ` plus a redacted diagnostic and exits `2`
(`agent/crates/layerx-agentd/src/main.rs:782-787`).

`RejectionReason` for the library file: `Missing`, `Empty`,
`Duplicate`, `Unknown`, `InvalidInteger`, `UnsupportedProtocol`,
`InvalidTenant`, `InvalidPath`, `IncompleteTenantMap`,
`InvalidVerificationLevel`, `TooLarge`, `InvalidEncoding`,
`Unavailable`, `Unprotected`
(`agent/crates/layerx-agentd/src/config.rs:80-94`).

---

## Typed refusals the agent sees

### `layerx install mcp`

| Refusal | When |
| --- | --- |
| `agent runtime {value} is not supported; use layerx, claude-code, claude-desktop, cursor, or vscode` | `--host` naming an undocumented runtime, checked before the binding document is read (`platform/cli/src/install/mod.rs:60-62`; `platform/cli/src/install/mcp.rs:34-41`) |
| `no agent runtime was selected for installation` | No `--host` given and no runtime detected (`platform/cli/src/install/mcp.rs:35-37`) |
| `the daemon binding document at {path} could not be used: {detail}; agent-daemon enrolment writes it before the MCP server is installed` | The document is absent, unreadable, malformed, or refused by the daemon (`platform/cli/src/install/mcp.rs:42-48`; `agent/crates/layerx-mcp/src/binding.rs:57-63`) |
| `--daemon-binding requires a path` | Empty explicit path (`platform/cli/src/install/mcp.rs:159-162`) |
| `tool {name} carries no catalogue description` / `argument schema` | A served catalogue entry without a description or schema (`platform/cli/src/toolset.rs:156-161`) |

### `layerx-agentd` MCP enrolment at boot

Every one of these exits `2` with the redacted boot diagnostic before
the daemon serves anything.

| Refusal | When |
| --- | --- |
| `{name} must be an absolute path` | `LAYERX_AGENT_MCP_BINDING_ROOT` or `LAYERX_AGENT_MCP_AUDIT_ROOT` given as a relative path (`agent/crates/layerx-agentd/src/main.rs:69-76`) |
| `LAYERX_AGENT_MCP_MODE is invalid` | A mode other than `full` or `read-only` (`agent/crates/layerx-agentd/src/main.rs:226-230`) |
| `LAYERX_AGENT_MCP_ACTIVITY_TYPES lists an invalid activity type` / `repeats an activity type` | A non-numeric or duplicated activity type (`agent/crates/layerx-agentd/src/main.rs:168-180`) |
| `LAYERX_AGENT_MCP_SCOPES lists an empty scope` / `repeats a scope` | An empty or duplicated scope (`agent/crates/layerx-agentd/src/main.rs:182-194`) |
| `LAYERX_AGENT_MCP_PEER_UID names no configured human peer` | A UID outside `LAYERX_AGENT_HUMAN_PEERS` (`agent/crates/layerx-agentd/src/main.rs:282-284`) |
| `the MCP daemon surface is invalid: {detail}` | The program listener or bearer the document would name is not loopback or is under 32 bytes (`agent/crates/layerx-agentd/src/main.rs:255-263`; `agent/crates/layerx-agentd/src/enrolment.rs:133-159`) |
| `the MCP agent identity is unverified: {detail}` | The human authority does not bind that DID for that tenant and principal (`agent/crates/layerx-agentd/src/main.rs:292-294`) |
| `LAYERX_AGENT_MCP_SESSION_ID names an open session without its binding document` | A restart whose session is open while the published document names a different session or none (`agent/crates/layerx-agentd/src/main.rs:318-329`) |
| `MCP enrolment failed: {detail}` | `enrolment::enrol` refused: an unrestorable capability, a pre-existing binding file, or a close after a failed publication (`agent/crates/layerx-agentd/src/main.rs:330-337`; `agent/crates/layerx-agentd/src/enrolment.rs:397-442`) |
| `LAYERX_AGENT_MCP_BINDING_ROOT requires the full agent mode, which serves the program endpoint and probe program the binding names` | The key set in the human-owner-only mode (`agent/crates/layerx-agentd/src/human_owner_mode.rs:77-82`) |

### `layerx mcp serve` stdio

| Refusal | When |
| --- | --- |
| JSON-RPC `-32700` `the message is not valid JSON` | Non-JSON stdin line (`agent/crates/layerx-mcp/src/stdio.rs:124-130`) |
| `-32600` `the message did not name a method` | Missing `method` (`agent/crates/layerx-mcp/src/stdio.rs:132-138`) |
| `-32601` `method {method} is not implemented` | Unknown method (`agent/crates/layerx-mcp/src/stdio.rs:148-152`) |
| `-32602` `the call did not name a tool` | `tools/call` without `name` (`agent/crates/layerx-mcp/src/stdio.rs:186-188`) |
| `-32602` `tool {name} is not served by this deployment` | Name outside the bound surface (`agent/crates/layerx-mcp/src/stdio.rs:189-195`) |
| `structuredContent.refusal` plus `tool` and `stage` `arguments`, `isError` true | Arguments outside the catalogue schema (`agent/crates/layerx-mcp/src/stdio.rs:197-205`) |
| `structuredContent.refusal` plus `stage` `freshness` or `daemon` and `state` `"refused"` / `"unknown"`, `isError` true | The daemon refused the invocation (`agent/crates/layerx-mcp/src/stdio.rs:215-220, 257-263`) |
| `structuredContent.refusal` plus `stage` `authority`, `isError` true | The bound session or capability is absent, closed, revoked, expired, cross-tenant, scope-empty or otherwise unusable (`agent/crates/layerx-mcp/src/stdio.rs:247-252, 266-285`) |
| `the binding document is unreadable` / `is malformed` / `the daemon refused the binding` | `Binding::open` before the server ever starts (`agent/crates/layerx-mcp/src/binding.rs:57-63`; `platform/cli/src/mcp.rs:12`) |

### `layerx-mcp` crate

| Type | Meaning |
| --- | --- |
| `ServerError::MissingSession` / `MissingCapability` | Bind-time records absent (`agent/crates/layerx-mcp/src/server.rs:422-438, 652-669`) |
| `ClosedSession` / `RevokedSession` | Session closed or generation advanced (`agent/crates/layerx-mcp/src/server.rs:217-218, 729-745`) |
| `TenantMismatch` / `CapabilityMismatch` | Session and capability disagree (`agent/crates/layerx-mcp/src/server.rs:220-224`) |
| `ExpiredAuthority` | `core_sequence` at or past session or capability expiry (`agent/crates/layerx-mcp/src/server.rs:226-229`) |
| `NoScope` | No catalogue scope remains after filtering (`agent/crates/layerx-mcp/src/server.rs:231-250, 448-449`) |
| `ToolAbsent` | Unknown, out-of-scope, or non-matching read/write tool (`agent/crates/layerx-mcp/src/server.rs:532, 600-604, 632-636`; `agent/crates/layerx-mcp/src/readonly.rs:74-79`) |
| `InvalidInvocation` | Empty/oversized/NUL tool name or arguments > `1_048_576` (`agent/crates/layerx-mcp/src/server.rs:19-20, 506-511`) |
| `ValidationError::ScopeDenied` / `CounterpartyDenied` / `AuthorityOverride` | Untrusted arguments cannot change tenant, scope, or counterparty (`agent/crates/layerx-mcp/src/untrusted.rs:64-71, 81-125`) |
| `ReadToolError::Unverified` / `InvalidBounds` / `CursorMismatch` / `ResultTooLarge` / `MissingReceiptEvidence` | Read envelope refusals (`agent/crates/layerx-mcp/src/tools/read.rs:110-119`) |
| `WriteToolError::Stage(StageFailure)` | Named `WriteStage` with `FailureClass` `Refused` / `Unavailable` / `InvalidEvidence` / `Protocol` (`agent/crates/layerx-mcp/src/tools/write.rs:30-44, 80-87`) |
| `WriteToolError::SuccessWithoutVerifiedReceipt` | `Executed` without verified receipt evidence (`agent/crates/layerx-mcp/src/tools/write.rs:291-301`) |
| `LimitRefusal::Exceeded` | Names `limit`, `name`, `ceiling`, `consumed`, `held`, `requested` (`agent/crates/layerx-agentd/src/budget/reserve.rs:161-169, 203-211`) |
| `ApprovalError::DisclosureChanged` | Presented disclosure ≠ held ticket (`agent/crates/layerx-mcp/src/approval.rs:25-28, 139-140`) |

Sources: `docs/wiki/Agentd.md`, `docs/wiki/Cli.md`,
`agent/README.md`, `agent/crates/layerx-mcp/README.md`,
`agent/crates/layerx-mcp/src/` (`binding.rs`, `catalogue.rs`,
`stdio.rs`, `server.rs`), `agent/crates/layerx-agentd/src/`
(`enrolment.rs`, `session.rs`, `main.rs`), `platform/cli/src/install/`,
`platform/cli/src/toolset.rs`, `platform/cli/src/mcp.rs`,
`agent/sdk/typescript/examples/`, `agent/sdk/python/examples/`.

[Home](Home.md)

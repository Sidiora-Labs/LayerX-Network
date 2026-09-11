# LayerX CLI

The developer CLI binary is `layerx` (`platform/cli/Cargo.toml:11-13`;
`platform/cli/src/main.rs:30-31`). The crate is `layerx-platform-cli`. The
stable graph anchor is `layerx-cli-v1`
(`platform/cli/src/main.rs:490-494`). Global `--json` emits one JSON object
instead of human presentation (`platform/cli/src/main.rs:33-38`). Success
envelopes are `{ok, kind, message, data}`; failures are `{ok: false, error:
{code, detail}}` (`platform/cli/src/output.rs:18-59`). Typed machine codes
are taken only from a leading `snake_case` token before `": "`; other
errors use `command_failed` (`platform/cli/src/output.rs:62-75`).

This page covers that binary: commands, credential storage, how it reaches an
endpoint, typed refusals, receipt verification, and the test suite. It does
not document SDK clients; those live under `platform/docs/content/`. It does
not document portable receipt JSON; see
[Portable receipt verifier](PortableVerifier.md).

The `layerx wallet` and `layerx token` command groups are not in this tree:
`platform/cli/src/main.rs` declares no such variants, and the tables below that
describe them are forward-looking. Every command in the tables that follow this
section is implemented and cited. Wallet, faucet, send, token, program, and 402
steps: [Payments developer path](PaymentsQuickstart.md).

---

## Commands

Required inputs are clap-required unless a default is named. Optional flags
that the binary implements are listed; unimplemented flags are omitted.

| Command | Purpose | Required inputs |
| --- | --- | --- |
| `layerx new <name>` | Scaffold a deterministic Rust program project (`platform/cli/src/main.rs:45-46, 83-88`; `platform/cli/src/scaffold.rs:9-38`) | `name` (lowercase Cargo package name, 1–64, digits/`-`, not starting with `-`; `platform/cli/src/scaffold.rs:52-62`). `--directory` default `.` |
| `layerx workspace` | With no subcommand, open the visual workspace on a TTY (`platform/cli/src/workspace.rs:738-740, 750-752`) | TTY required for interactive mode; `--json` or a non-TTY stdin refuses |
| `layerx workspace modules` | List every module the workspace CLI controls (`platform/cli/src/workspace.rs:21-23, 741`) | none |
| `layerx workspace doctor` | Inspect tools and module readiness without changing anything (`platform/cli/src/workspace.rs:24-25, 742`) | `--module` repeatable/comma-separated; `--all`; `--environment` |
| `layerx workspace install` | Resolve locked project dependencies (`platform/cli/src/workspace.rs:26-27, 743`) | selection flags plus `--dry-run`, `--fail-fast`, `-y`/`--yes`, `--ci`, `--allow-production` (`platform/cli/src/workspace.rs:50-69`) |
| `layerx workspace build` | Build selected modules (`platform/cli/src/workspace.rs:28-29, 744`) | same as install |
| `layerx workspace test` | Test selected modules (`platform/cli/src/workspace.rs:30-31, 745`) | same as install. Production tests refuse unless `--allow-production` (`platform/cli/src/workspace.rs:66-68, 1067-1071`) |
| `layerx workspace all` | Install, build, and test in order (`platform/cli/src/workspace.rs:32-33, 746`) | same as install |
| `layerx environment list` | List configured endpoint profiles (`platform/cli/src/main.rs:94-96, 684-702`) | none |
| `layerx environment current` | Show the active endpoint profile (`platform/cli/src/main.rs:97-98, 704-710`) | none |
| `layerx environment use <name>` | Select a profile, configuring its endpoint when first used (`platform/cli/src/main.rs:99-110, 712-776`) | `name` must be `emulator`, `testnet`, or `production` (`platform/cli/src/config.rs:121-126`). `--endpoint`, `--network-id`, and one of `--sequencer-trust-anchor` / `--sequencer-trust-anchor-file` must be supplied together or omitted together (`platform/cli/src/emulator.rs:751-791`) |
| `layerx key create <name>` | Generate an Ed25519 seed from OS randomness and store it (`platform/cli/src/main.rs:114-120, 784-790`; `platform/cli/src/credential.rs:83-96`) | `name` (1–128 ASCII alnum/`-`/`_`; `platform/cli/src/credential.rs:294-303`). `--did` optional |
| `layerx key import <name>` | Import a 32-byte hexadecimal Ed25519 seed from stdin (`platform/cli/src/main.rs:121-126, 792-798`; `platform/cli/src/credential.rs:142-155`) | `name`; seed on stdin. `--did` optional (`platform/cli/src/main.rs:123-127`) |
| `layerx key list` | List public key metadata without opening secret material (`platform/cli/src/main.rs:127-128, 800-817`) | none |
| `layerx key show <name>` | Show public metadata for one key (`platform/cli/src/main.rs:129-130, 819-834`) | `name` |
| `layerx key default <name>` | Select the default key used by account commands (`platform/cli/src/main.rs:131-132, 836-842`) | `name` |
| `layerx key delete <name>` | Permanently delete a key from credential storage (`platform/cli/src/main.rs:133-134, 844-850`) | `name` |
| `layerx auth set` | Read an API token from stdin and save it (`platform/cli/src/main.rs:138-143, 858-865`) | token on stdin. `--environment` optional (else current; `platform/cli/src/main.rs:1354-1360`) |
| `layerx auth status` | Report whether a token exists without printing it (`platform/cli/src/main.rs:144-148, 867-878`) | `--environment` optional |
| `layerx auth delete` | Permanently delete a stored API token (`platform/cli/src/main.rs:149-153, 880-887`) | `--environment` optional |
| `layerx register` | Register a self-service identity principal for one local signing key: sign the tagged tenant binding, POST `lx_register`, and accept the result only when it names the same tenant, subject, and signer key (`platform/cli/src/main.rs:63-64, 512`; `platform/cli/src/register.rs:43-50, 52-88, 90-130`) | a local key: `--key`, else the configured default (`platform/cli/src/register.rs:93-103`). The tenant is fixed to `beta`, and a principal naming another tenant, subject, or signer key is refused (`platform/cli/src/register.rs:11, 78-86`) |
| `layerx faucet` | Claim one bounded testnet grant for the active identity session and a local key, through gateway `lx_requestFunds` (`platform/cli/src/main.rs:65-66, 513`; `platform/cli/src/faucet.rs:89-118`) | a stored session token for the active environment (`layerx auth set`) and a local key: `--key`, else the configured default (`platform/cli/src/faucet.rs:92-103`) |
| `layerx account create` | Register an account on the active endpoint (`platform/cli/src/main.rs:157-170, 896-926`; `platform/cli/src/account.rs:6-64`) | emulator: a local key (`--key` or default) (`platform/cli/src/account.rs:18-20`). Hosted: `--email`, `--display-name`, `--idempotency-key`; `--initial-amount` must be `0` (`platform/cli/src/account.rs:42-54`). `--initial-amount` default `0` |
| `layerx account get` | Read the active hosted profile or one emulator DID account (`platform/cli/src/main.rs:171-175, 928-932`; `platform/cli/src/account.rs:67-84`) | emulator requires `--did` (`platform/cli/src/account.rs:68-69`) |
| `layerx payment test` | Quote and commit a test payment through the active endpoint (`platform/cli/src/main.rs:179-192, 936-950`; `platform/cli/src/payment.rs:5-37`) | `--from`, `--to`, `--currency` (alias `--asset`), `--amount` (>0), `--idempotency-key` (16–128 alnum/`-`/`_`; `platform/cli/src/http.rs:366-377`) |
| `layerx receipt get <id>` | Fetch exact receipt material from the active endpoint (`platform/cli/src/main.rs:197-198, 956-964`) | `id` (path-safe; `platform/cli/src/http.rs:352-363`) |
| `layerx receipt verify` | Verify a canonical receipt against independently supplied batch facts, locally (`platform/cli/src/main.rs:199-217, 966-978`; `platform/cli/src/receipt.rs:18-52`) | `--receipt`, `--batch-id`, `--asset`, `--previous-state-root`, `--resulting-state-root`, `--sequencer-public-key` |
| `layerx program discover <program_id>` | Discover one active program from receipt-backed registry state (`platform/cli/src/main.rs:220-222, 1026-1033`; `platform/cli/src/programs.rs:628-644`) | `program_id` |
| `layerx program interface get <program_id>` | Read a canonical code-bound program interface (`platform/cli/src/main.rs:337-340, 1128-1132`; `platform/cli/src/programs.rs:647-673`) | `program_id` |
| `layerx program interface publish <program_id>` | Publish a canonical interface (`platform/cli/src/main.rs:341-347, 1133-1141`; `platform/cli/src/programs.rs:676-697`) | `program_id`, `--interface`, `--idempotency-key` |
| `layerx program build` | Compile to WASM and enforce the deterministic runtime policy locally (`platform/cli/src/main.rs:226-232, 1036-1043`; `platform/cli/src/programs.rs:122-138`) | `--manifest-path` default `Cargo.toml`; `--artifact` optional |
| `layerx program bindings` | Generate digest-bound Rust, TypeScript, and guest bindings (`platform/cli/src/main.rs:233-247, 1044-1053`; `platform/cli/src/programs.rs:40-96`) | `--interface`, `--digest`, `--code-hash`; `--output` default `bindings` |
| `layerx program deploy <artifact>` | Validate and submit a WASM artifact for receipt-backed deployment (`platform/cli/src/main.rs:248-257, 1054-1070`; `platform/cli/src/programs.rs:245-279`) | `artifact` plus lifecycle flags below. `--upgrade-authority` and `--interface` optional |
| `layerx program upgrade <artifact>` | Submit a WASM upgrade (`platform/cli/src/main.rs:258-270, 1071-1091`; `platform/cli/src/programs.rs:289-328`) | `artifact`, `--old-hash`, lifecycle flags. `--migration-hook` optional. `--interface` conflicts with `--clear-interface` (`platform/cli/src/main.rs:263-267, 1479-1493`) |
| `layerx program wind-down route` | Route a program-owned account (`platform/cli/src/main.rs:303-315, 1148-1168`) | `--account`, `--asset`, `--destination`; `--seed` default empty; lifecycle flags |
| `layerx program wind-down deprecate` | Deprecate toward an exit program (`platform/cli/src/main.rs:316-323, 1170-1181`) | `--exit-program`, `--deadline-batch`; lifecycle flags |
| `layerx program wind-down tombstone` | Tombstone the program (`platform/cli/src/main.rs:324-327, 1183-1191`) | lifecycle flags |
| `layerx program wind-down exit` | Exit a program-owned account (`platform/cli/src/main.rs:328-333, 1193-1199`) | `--account`; lifecycle flags |
| `layerx program call` | Submit calldata and render the receipt-verified result (`platform/cli/src/main.rs:273-274, 1099-1123, 1232-1285`; `platform/cli/src/programs.rs:810-838`) | `program_id`, `--fuel`, `--idempotency-key`, `--account-sequence`, `--not-before-ms`, `--expires-at-ms`. `--calldata` optional. `--fee-limit` default `0`. `--capability` repeatable. `--key` optional. Native ABI flags default ABI 2 / `layerx_call` / 16_777_216 memory (`platform/cli/src/programs.rs:724-756`; `platform/cli/src/main.rs:1500-1530`) |
| `layerx program simulate` | Execute a program call against current state without committing it (`platform/cli/src/main.rs:275-276, 1288-1341`; `platform/cli/src/programs.rs:902-919`) | same as `program call` |
| `layerx program registry get <program_id>` | Read one program's receipt-backed registry record (`platform/cli/src/main.rs:352-353, 1208-1212`; `platform/cli/src/programs.rs:554-625`) | `program_id` |
| `layerx program registry verify-source <program_id>` | Submit a source digest and location (`platform/cli/src/main.rs:354-363, 1213-1227`; `platform/cli/src/programs.rs:700-717`) | `program_id`, `--source-uri`, `--source-digest`, `--idempotency-key` |
| `layerx emulator provision` | Generate the sequencer seed and publish its trust anchor under the profile directory (`platform/cli/src/main.rs:370-375, 508-512`; `platform/cli/src/emulator.rs:223-305`) | `--force` optional; without it an existing seed or anchor is refused |
| `layerx emulator up` | Start the local real-transition gateway (`platform/cli/src/main.rs:368-369, 375-387, 513-525`) | `--sequencer-seed-file`. `--listen`, `--network-id`, `--time-ms`, `--prefund` optional |
| `layerx install mcp` | Install a payment-capable MCP server (`platform/cli/src/main.rs:393-395, 397-415, 596-612`; `platform/cli/src/install/mcp.rs:24-98`) | hosted environment only (`platform/cli/src/install/mod.rs:573-577`). `--host` values: `layerx`, `claude-code`, `claude-desktop`, `cursor`, `vscode` (`platform/cli/src/install/mod.rs:53-64`). Payment mode requires `--source-account` and `--asset` (`platform/cli/src/install/mcp.rs:142-156`) |
| `layerx install a2a` | Install a payment-capable A2A server (`platform/cli/src/main.rs:396-397, 417-437, 614-631`; `platform/cli/src/install/a2a.rs:40-119`) | hosted environment only. `--listen` default `127.0.0.1:9433` |
| `layerx mcp serve` | Serve MCP on stdin/stdout (`platform/cli/src/main.rs:442-458, 527-545`; `platform/cli/src/mcp.rs:13-56`) | `--gateway-credential`. `--environment`, `--key`, `--source-account`, `--asset`, `--read-only` optional |
| `layerx a2a serve` | Serve the agent card and task interface on loopback (`platform/cli/src/main.rs:461-481, 547-572`; `platform/cli/src/a2a.rs:89-176`) | `--gateway-credential`, `--authorization-file`. `--listen` default `127.0.0.1:9433` |
| `layerx a2a start` | Start the installed managed A2A runtime (`platform/cli/src/main.rs:482-483, 574-577`; `platform/cli/src/a2a.rs:710-748`) | none (reads `a2a/runtime.json`) |
| `layerx a2a stop` | Stop the installed managed A2A runtime (`platform/cli/src/main.rs:484-485, 578-582`; `platform/cli/src/a2a.rs:750-770`) | none |
| `layerx a2a status` | Report the installed managed A2A runtime state (`platform/cli/src/main.rs:486-487, 583-587`; `platform/cli/src/a2a.rs:772-781`) | none |

These command groups are not implemented in this tree. The flags, refusals, and
RPC calls named in this section have no counterpart in `platform/cli/src/`; they
describe the intended surface, not the shipped one:

| Command | Purpose and important inputs |
| --- | --- |
| `layerx wallet create <name>` | Emulator only: create a private wallet, register its DID, and open its main account; public use returns `wallet_registration_unavailable` before generating a key |
| `layerx wallet import <name>` | Import a 32-byte hexadecimal seed from stdin; does not register or fund |
| `layerx wallet list` | List public wallet metadata without exposing seeds |
| `layerx wallet balance` | Read one DID or Asset balance through public RPC |
| `layerx wallet history` | Read history where the public surface supports it; DID history otherwise returns unavailable |
| `layerx wallet receipt <activity_id>` | Retrieve and verify a receipt; `--wait` selects commitment |
| `layerx wallet send` | Sign native debit and envelope; requires `--to`, `--asset`, `--amount`, receipt policy, and fee limit |
| `layerx wallet open-account` | Open the selected wallet's per-Asset account |
| `layerx wallet estimate-fee <canonical_hex>` | Estimate from the committed native fee schedule |
| `layerx wallet watch` | Receive one live `receipts`, `checkpoints`, or `account` notification, then reconcile |
| `layerx token create` | Register a native Asset from symbol, name, decimals, cap, and salt |
| `layerx token mint` | Move issuance units to an existing per-Asset account |
| `layerx token burn` | Return selected-wallet units to issuance |
| `layerx token transfer` | Transfer from the selected wallet's per-Asset account |
| `layerx token info <asset_id>` | Call `lx_getAsset` |
| `layerx token list` | Call `lx_listAssets` |

Those public writes are intended to require an RPC endpoint and gateway
credential, an independently supplied receipt policy, and a fee limit, with
remote RPC URLs restricted to HTTPS and a `/rpc` path. None of those flags
exists in this tree; the only globally applied argument the binary declares is
`--json` (`platform/cli/src/main.rs:34-39`), the per-command
`--gateway-credential` belongs to `mcp serve` and `a2a serve`
(`platform/cli/src/main.rs:451-452, 467-468`), and `--fee-limit` is optional
with default `0` (`platform/cli/src/main.rs:297-298, 1109-1110`). A pending
result retains the activity id; rerun receipt lookup instead of creating a
second payment.

Lifecycle flags shared by deploy, upgrade, and wind-down
(`platform/cli/src/main.rs:282-300, 1409-1475`): `--program-id`,
`--idempotency-key`, `--account-sequence`, `--not-before-ms`,
`--expires-at-ms`, `--previous-state-root` required; `--key` optional;
`--fee-limit` default `0`.

`layerx program registry list` is not a command
(`platform/cli/src/main.rs:1534-1536`).

`platform/docs/content/guide/programs.md:75` states there is no dedicated
program account-registration CLI command; that activity is submitted as
canonical bytes to `POST /v1/activities`. The CLI command table above does
not add one.

---

## Credentials

## Headless credential storage

The OS keyring is the default. On a headless Linux server, container, or CI
runner, explicitly select the encrypted file store before creating a key:

```bash
export LAYERX_CREDENTIAL_STORE=file
read -r -s -p 'Credential passphrase: ' LAYERX_CREDENTIAL_PASSPHRASE
export LAYERX_CREDENTIAL_PASSPHRASE
layerx --json key create quickstart
layerx --json key list
layerx --json auth status
```

Use a strong passphrase of 12–16384 bytes. In CI, supply
`LAYERX_CREDENTIAL_PASSPHRASE` through the CI secret environment. Keep it
available for subsequent CLI and MCP/A2A processes, and unset it when finished.
Secret imports still read stdin; the passphrase does not consume that input.
`auth status` reports whether an API token is stored, independently of keys;
store one with `layerx auth set` using the token on stdin.

The store writes `credentials/vault` beside the resolved CLI config file
(`LAYERX_CONFIG`, otherwise `$XDG_CONFIG_HOME/layerx/config.json`, otherwise
`$HOME/.config/layerx/config.json`). It encrypts all keys, tokens and gateway
credentials using AES-256-GCM and PBKDF2-HMAC-SHA256 with 600,000 iterations,
a fresh 16-byte salt and a fresh 12-byte nonce on every update. Vault and lock
files use mode 0600 inside a mode 0700 directory. Group/world-accessible files,
symlinks, incorrect ownership, wrong passphrases and damaged vaults are refused.
Updates use a process lock and atomic replacement. This backend currently
requires Unix file permissions.

There is no automatic fallback or migration between stores. Unset
`LAYERX_CREDENTIAL_STORE` (or set it to `os`) to use the OS keyring. Retain the
passphrase and encrypted vault together in your backup procedure: a lost
passphrase cannot be recovered. Changing the environment passphrase does not
rotate the vault password; it makes authentication fail.

Secrets are stored under keyring service `dev.layerx.cli` with entry names
`{kind}:{name}` (`platform/cli/src/credential.rs:11, 55-59`). Kinds are
`key`, `token`, and `gateway` (`platform/cli/src/credential.rs:127, 179,
231`).

In normal operation `ensure_store` defers to the keyring v1 initialiser:
Keychain Services on macOS, Credential Manager on Windows, Secret Service
on other Unix (`platform/cli/src/credential.rs:21-26, 47-52`). Configuration
JSON holds only public metadata (`did`, `public_key`) and environment
endpoints (`platform/cli/src/config.rs:19-32`;
`platform/cli/tests/credential.rs:56-77`). On Unix the config file is
created `0o600` (`platform/cli/src/config.rs:85-88`;
`platform/cli/tests/credential.rs:80-97`).

| Kind | How it is written | How it is read |
| --- | --- | --- |
| Ed25519 seed | `key create` fills 32 OS-random bytes, or `key import` reads hex from stdin, then `set_password` (`platform/cli/src/credential.rs:83-132`) | `key_seed` `get_password` then hex-decode (`platform/cli/src/credential.rs:219-226`) |
| Hosted API token | `auth set` reads stdin, validates visible ASCII, `set_password` (`platform/cli/src/credential.rs:175-186, 209-216`) | `token` `get_password`; `NoEntry` is `None` (`platform/cli/src/credential.rs:195-206`) |
| Gateway key | `install mcp` / `install a2a` provision `/v1/keys` or `/v1/keys/{id}/rotate` and `set_gateway` (`platform/cli/src/install/mod.rs:594-640, 673-690`; `platform/cli/src/credential.rs:228-236`) | `gateway` `get_password`; MCP/A2A serve load by alias (`platform/cli/src/toolset.rs:67-71`) |

Stdin secrets are capped at 16 KiB
(`platform/cli/src/credential.rs:12, 65-73`). Gateway credentials must be
`{id}:lxp_live_` plus 64 hex digits, 73-byte secret
(`platform/cli/src/credential.rs:263-276`). Installed host JSON is refused
if a field name contains a secret marker; secrets stay in credential
storage (`platform/cli/src/install/mod.rs:22-32, 892-900`).

`LAYERX_CREDENTIAL_STORE` selects an isolated in-memory store only when the
binary is built with `test-credential-store` and the value is `mock`
(`platform/cli/src/credential.rs:14-19, 34-45`;
`platform/cli/Cargo.toml:8-9`). A production / `--no-default-features`
binary accepts explicit `file` or `os` selection. It refuses `mock` with
`credential store override mock is unavailable in this binary`, and writes no config
(`platform/cli/src/credential.rs:43-45`;
`platform/cli/tests/production-credential-refusal.sh:8-26`).

`platform/docs/content/install.md:48-49` says emulator key creation never
asks the operator to type key material. `layerx key import` does read a
32-byte hex seed from stdin (`platform/cli/src/main.rs:121-126`;
`platform/cli/src/credential.rs:98-111`). Those two sources disagree on
whether a seed may be typed; the import command exists.

---

## Environment variables

| Variable | Read by | Effect |
| --- | --- | --- |
| `LAYERX_CONFIG` | `config::path` (`platform/cli/src/config.rs:129-131`) | Absolute or CWD-relative config JSON path |
| `XDG_CONFIG_HOME` | `config::path` (`platform/cli/src/config.rs:133-134`) | `{XDG_CONFIG_HOME}/layerx/config.json` when `LAYERX_CONFIG` is unset |
| `HOME` | `config::path` (`platform/cli/src/config.rs:136-139`); install host paths (`platform/cli/src/install/mod.rs:1119-1125`) | `{HOME}/.config/layerx/config.json`; agent-runtime install roots |
| `LAYERX_CREDENTIAL_STORE` | `install_store` (`platform/cli/src/credential.rs:17, 34-45`) | `file` selects encrypted storage; `os` selects OS storage; `mock` requires `test-credential-store` |
| `LAYERX_GATEWAY_KEY_ID` | MCP/A2A runtime (`platform/cli/src/toolset.rs:72-82`); written into install env (`platform/cli/src/install/mcp.rs:53-55`; `platform/cli/src/install/a2a.rs:67-69`) | Must match the non-secret id of the stored gateway credential |
| `LAYERX_INSTALL_ROOT` | install host path resolution (`platform/cli/src/install/mod.rs:1119-1130`) | Replaces `HOME` for host config discovery |
| `LAYERX_REPO_ROOT` | workspace (`platform/cli/src/workspace.rs:1098`) | Repository root for workspace commands |
| `LAYERX_PROGRAM_SDK` | scaffold (`platform/cli/src/scaffold.rs:65-68`) | Path written into a new program `Cargo.toml` |
| `LAYERX_CREDENTIAL_PASSPHRASE` | `file_store::Entry::from_environment` (`platform/cli/src/file_store.rs:36-42`) | Required by the `file` store; 12-16384 bytes, otherwise the command refuses |
| `CARGO` | `program build` (`platform/cli/src/programs.rs:181`) | Cargo executable for the Rust WASM toolchain |

Workspace child processes receive `LAYERX_ENVIRONMENT`, `LAYERX_ENDPOINT`,
and `LAYERX_NETWORK_ID` (`platform/cli/src/workspace.rs:972-974`). Managed
A2A start admits only `LAYERX_CONFIG` and `LAYERX_GATEWAY_KEY_ID` in the
installed env map and strips `LAYERX_TOKEN`, `LAYERX_API_TOKEN`, and
`LAYERX_AUTH_TOKEN` from the child (`platform/cli/src/a2a.rs:645-679`).

The CLI does not read `LAYERX_API_TOKEN` (or `LAYERX_TOKEN` /
`LAYERX_AUTH_TOKEN`) as a substitute for `auth set`.
`platform/docs/content/install.md:104-111` documents `LAYERX_API_URL` and
`LAYERX_API_TOKEN` as SDK application inputs, not CLI credential inputs.

Default config when the file is absent: environment `emulator`, endpoint
`http://127.0.0.1:9402`, `network_id` 402, no sequencer trust anchor
(`platform/cli/src/config.rs:34-49`). Config version must be `1`
(`platform/cli/src/config.rs:9, 65-69`).

---

## Emulator, hosted gateway, and node

The CLI talks HTTP to the active environment endpoint
(`platform/cli/src/main.rs:1348-1351`; `platform/cli/src/http.rs:22-47`).
There is no CLI command that opens a `layerxd` node RPC.

| Target | How the CLI reaches it |
| --- | --- |
| Emulator | `emulator up` runs `layerx_platform_emulator::run` in-process (`platform/cli/src/main.rs:72-74, 513-524`). Other commands HTTP to the configured emulator endpoint. `environment use emulator` with bound inputs waits for a listener and `GET /v1/sequencer`, then refuses `network_id_mismatch` or `sequencer_trust_anchor_mismatch` (`platform/cli/src/emulator.rs:860-891`; `platform/cli/src/main.rs:731-761`) |
| Hosted gateway | `environment use testnet` or `production` stores `https://…` (non-loopback `http://` is refused; `platform/cli/src/http.rs:27-37`). Account, payment, receipt, and program commands send `Authorization: Bearer` when a token exists (`platform/cli/src/http.rs:222-227`; `platform/cli/src/main.rs:1348-1351`). MCP/A2A send `Authorization: LayerX-Key` (`platform/cli/src/http.rs:50-64, 229-231`; `platform/cli/src/toolset.rs:103-104`). Install MCP/A2A refuse the emulator (`platform/cli/src/install/mod.rs:573-577`) |
| Node | Not a CLI transport. `layerx receipt verify` is local `layerx_proof` against caller-supplied batch facts (`platform/cli/src/receipt.rs:4, 25-34`). `platform/docs/content/concepts/receipts.md:3-4` states the same: verification needs no LayerX node, gateway, or hosted service |

Emulator account create posts `/__emulator/accounts/prefund`
(`platform/cli/src/account.rs:25-34`). Hosted account create posts
`/v1/accounts` (`platform/cli/src/account.rs:56-63`). Payments post
`/v1/moves/quote` then `/v1/moves` (`platform/cli/src/payment.rs:23-32`).
Receipts get `/v1/receipts/{id}` (`platform/cli/src/main.rs:967`). Program
lifecycle posts octet-stream to `/v1/programs/deploy`, `/upgrade`,
`/wind-down`, `/call`, `/simulate` (`platform/cli/src/programs.rs:436-442,
816, 906`). MCP/A2A `activity.submit` posts JSON to `/v1/activities`
(`platform/cli/src/toolset.rs:341-344`).

---

## Refusals

Machine codes in the first column are the `error.code` values emitted when
the detail string is `code: …` (`platform/cli/src/output.rs:62-75`;
`platform/cli/src/emulator.rs:96-118`). Prose errors use `command_failed`.

| Refusal | Condition |
| --- | --- |
| `credential store override … is unavailable in this binary` (`command_failed`) | `LAYERX_CREDENTIAL_STORE` set to an unsupported value; `mock` requires `test-credential-store` (`platform/cli/src/credential.rs:34-45`; `platform/cli/tests/production-credential-refusal.sh:17-25`) |
| missing credential (`command_failed`) | `token` `NoEntry` is not itself a refusal; HTTP proceeds without `Authorization` (`platform/cli/src/credential.rs:195-202`; `platform/cli/src/http.rs:46, 70-73`). Install without a stored identity session: `no {environment} identity session is held in credential storage…` (`platform/cli/src/install/mod.rs:603-606`). MCP/A2A serve without a gateway alias: `gateway credential alias … is absent; rerun layerx install…` (`platform/cli/src/toolset.rs:67-70`). Empty stdin secret (`platform/cli/src/credential.rs:77-78`). OS store unavailable (`platform/cli/src/credential.rs:47-50`) |
| `network_id_mismatch` | `environment use emulator` supplied network id disagrees with `GET /v1/sequencer` (`platform/cli/src/emulator.rs:116, 871-876`; `platform/cli/tests/emulator.rs:910-926`) |
| `network_id_reserved` | `--network-id 0` (`platform/cli/src/emulator.rs:113, 180, 768-769`) |
| `environment must be emulator, testnet, or production` (`command_failed`) | any other profile name (`platform/cli/src/config.rs:121-126`; `platform/cli/src/main.rs:723`) |
| `non-loopback environments must use https://` (`command_failed`) | endpoint scheme `http://` with a non-loopback host (`platform/cli/src/http.rs:27-37`) |
| unverified receipt (`command_failed`) | `receipt verify` / program render: `receipt verification failed at {:?}` or `program receipt verification failed at {:?}` (`platform/cli/src/receipt.rs:33-34`; `platform/cli/src/programs.rs:1175-1178, 2131`). Empty receipt (`platform/cli/src/programs.rs:1172-1173`). Forged local file (`platform/cli/tests/commands.rs:256-278`). Success siblings with unverifiable bytes are still refused (`platform/cli/src/programs.rs:2131`) |
| version mismatch (`command_failed`) | config `version != 1`: `unsupported CLI configuration version` (`platform/cli/src/config.rs:65-69`). Program receipt `protocol_version != 3` or `module_version != 4` (`platform/cli/src/programs.rs:1187-1191, 494-501`). `program receipt ABI does not match verified discovery` (`platform/cli/src/programs.rs:1200-1203`). Stale interface digest (`platform/cli/src/programs.rs:62-67`) |
| `environment_input_missing` | `environment use` given a partial endpoint/network/anchor set (`platform/cli/src/emulator.rs:106, 779-790`; `platform/cli/tests/commands.rs:90-105`) |
| `sequencer_trust_anchor_mismatch` | supplied anchor disagrees with advertised sequencer identity (`platform/cli/src/emulator.rs:117, 878-884`) |
| `sequencer_seed_exists` / `sequencer_trust_anchor_exists` | `emulator provision` without `--force` when those files exist (`platform/cli/src/emulator.rs:100-101, 126-135, 246-252`) |
| `MCP and A2A installation require a configured hosted testnet or production gateway…` (`command_failed`) | install against emulator (`platform/cli/src/install/mod.rs:573-577`; `platform/cli/tests/install.rs:35-52`) |

---

## Receipt verification before success

`layerx receipt verify` reads the file as hex text or raw bytes, builds
`AuthorizedBatch` from the five hex flags, and calls
`layerx_proof::receipt::verify_outcome` (`platform/cli/src/receipt.rs:18-52`).
It reports `verified: true` only after that call succeeds and protocol facts
plus a digest exist (`platform/cli/src/receipt.rs:35-51`). The check is local;
it does not contact the endpoint (`platform/cli/src/main.rs:970-982`).

`layerx program call` does not render a typed success until
`verify_program_outcome_at_root` binds the returned receipt to the discovered
sequencer key and prior state root, the activity id and protocol 3 / module
version 4 match, ABI matches discovery, sequence extends discovery, and the
terminal payload digest matches the receipt commitment
(`platform/cli/src/programs.rs:802-809, 1155-1232`). A receipt result code
and the typed outcome must agree
(`platform/cli/src/programs.rs:1155-1158`). Lifecycle deploy/upgrade/wind-down
verify the sequencer signature, bind protocol 3 / module 9 / version 4, and
on result code 0 call `verify_program_state`
(`platform/cli/src/programs.rs:461-526`). Simulation additionally requires
`committed: false` and sealed non-commit evidence
(`platform/cli/src/programs.rs:902-911, 1609-1699`).

`layerx payment test` reports the quote and commit journey JSON; it does not
run `verify_outcome` (`platform/cli/src/payment.rs:5-37`). Independent
verification is `receipt verify` after fetching bytes.

---

## Test suite

Isolation for the command suite lives in `platform/cli/tests/common/mod.rs`,
which declares `mod credential_environment;`
(`platform/cli/tests/common/mod.rs:7`) and builds one
`CredentialEnvironment` per `Cli` fixture
(`platform/cli/tests/common/mod.rs:45-51`).

| Gate | What it drives |
| --- | --- |
| `make platform-test` | `cargo test` of the platform workspace with `--features layerx-platform-cli/test-credential-store` (`platform/Makefile.inc:112-113`). CI `build-lint-test` runs this (`.github/workflows/platform.yml:53-54`) |
| `make platform-test-cli-production-credential-refusal` | `bash platform/cli/tests/production-credential-refusal.sh` (`platform/Makefile.inc:134-135`): builds `--no-default-features`, sets `LAYERX_CREDENTIAL_STORE=mock`, requires refusal and no config file |
| `make platform-test-tooling` | production-credential-refusal, then `cargo test -p layerx-platform-cli --features test-credential-store`, emulator/faucet/testnet crate tests, script syntax, `cargo build -p layerx-platform-cli`, `clean-bootstrap.sh` (`platform/Makefile.inc:115-132`). CI names this “Exercise the developer CLI end to end against the emulator” (`.github/workflows/platform.yml:57-58`) |
| `platform/cli/tests/common/mod.rs` | Real `layerx` child process against an isolated `LAYERX_CONFIG` and `LAYERX_REPO_ROOT` (`platform/cli/tests/common/mod.rs:1-6, 64-67`). `Emulator::start` spawns `layerx emulator up` on an ephemeral loopback port and waits for `/healthz` `"status":"ready"` (`platform/cli/tests/common/mod.rs:136-200`) |
| `platform/cli/tests/common/credential_environment.rs` | A private Secret Service for each fixture: an isolated XDG root, a `dbus-daemon` started from a generated `bus.conf` (`platform/cli/tests/common/credential_environment.rs:36-58`) and a `gnome-keyring-daemon --components=secrets` unlocked on stdin (`platform/cli/tests/common/credential_environment.rs:71-82`) |
| `platform/cli/tests/file_store.rs` | Encrypted file store: `LAYERX_CREDENTIAL_PASSPHRASE` handling and the 12-16384 byte bound (`platform/cli/src/file_store.rs:36-42`) |
| `platform/cli/tests/credential.rs` | Key/token commands: seeds and tokens accepted by the store never appear in config or stdout |
| `platform/cli/tests/commands.rs` | Envelope coverage and malformed-input refusals without emulator gateway routes (`platform/cli/tests/commands.rs:1-6`) |
| `platform/cli/tests/emulator.rs` | Live emulator: environment bind, account prefund, payment quote/commit, identity mismatches (`platform/cli/tests/emulator.rs:1-6`) |
| `platform/cli/tests/install.rs` | Install refusals; this file **removes** `LAYERX_CREDENTIAL_STORE` (`platform/cli/tests/install.rs:22`) so it does not use the mock |
| `platform/cli/tests/workspace.rs` | Workspace module inventory against `LAYERX_REPO_ROOT` |
| `platform/cli/tests/clean-bootstrap.sh` | Published `install.md` bootstrap sequence in a clean `HOME` against a real binary (`platform/cli/tests/clean-bootstrap.sh:53-115`) |
| `make platform-test-agent-install` | `install-journey.sh` against a hosted gateway (`platform/Makefile.inc:190-201`). Scheduled/dispatch CI installs `dbus-x11` and `gnome-keyring` and runs that journey under `dbus-run-session` / `gnome-keyring-daemon` (`.github/workflows/platform.yml:1458-1510`) |

The cargo command suite therefore exercises a **real emulator** and a **real
Secret Service**: each fixture starts its own `dbus-daemon` and
`gnome-keyring-daemon`, so the production Unix credential path
(`platform/cli/src/credential.rs:23-26`) is the path under test and no
developer keychain is touched
(`platform/cli/tests/common/mod.rs:3-5`;
`platform/cli/tests/common/credential_environment.rs:36-82`). The mock store
is a build-time feature, not the harness default
(`platform/cli/src/credential.rs:14-19, 34-45`); the
production-credential-refusal script proves a release-shaped binary will not
admit it. `platform/cli/tests/install.rs` removes `LAYERX_CREDENTIAL_STORE`
outright (`platform/cli/tests/install.rs:22`). The scheduled hosted journey
adds a keyring provisioned by CI rather than by the fixture
(`.github/workflows/platform.yml:1458-1510`).

[Home](Home.md)

# Install the tools

Three things get you to a verified payment: an SDK for your language, the `layerx` command line tool, and an environment to talk to. Nothing here asks you to generate signing material by hand, encode a payload or handle a byte.

This page describes the install path at the source revision it is built from. The beta is not a qualified release: the canonical beta contract records `readiness_claim: false` and the beta artifact manifest is not emitted, so no hosted endpoint or package coordinate below is asserted live or installable until a release publishes it. The commands are checked against the CLI source; the local emulator path runs the real core transition from a source checkout.

## Install the command line tool

```text
cargo install --path platform/cli
```

The binary is called `layerx`. Every subcommand accepts `--json` and will then emit exactly one JSON object, which is what you want in a script.

## Pick an environment

| Environment | What it is | Money |
|---|---|---|
| Local emulator | The real transition function running on your machine | Freely prefunded, worthless |
| Hosted testnet | A shared network reset on a published schedule | Faucet-issued, worthless |
| Production | The live network | Real |

Start on the emulator. It runs the same transition function as production, so a payment it accepts is a payment production would accept, and a refusal it gives you is a real refusal rather than a stub.

The emulator signs receipts with a sequencer key it has no compiled-in copy of. `layerx emulator provision` generates that key once, under your profile directory (`~/.config/layerx/emulator/`, readable only by you), and publishes its trust anchor beside it. It prints the paths and the anchor, never the seed. Start the emulator with the seed, then bind the profile to the endpoint, the network id and the anchor file; the CLI checks the anchor against the identity the running emulator advertises before it saves anything:

The canonical local bootstrap is supported on Linux, Android and Apple hosts, where the CLI can atomically publish the owner-only seed and anchor directory. Other host targets fail closed before writing profile material.

<!-- layerx:bootstrap-sequence -->
```bash
layerx emulator provision
layerx emulator up --sequencer-seed-file "$HOME/.config/layerx/emulator/sequencer.seed" &
layerx environment use emulator --endpoint http://127.0.0.1:9402 --network-id 402 \
  --sequencer-trust-anchor-file "$HOME/.config/layerx/emulator/sequencer.anchor"
```

If `XDG_CONFIG_HOME` or `LAYERX_CONFIG` is set, the profile directory moves with it; use the paths `layerx emulator provision` prints. Provisioning refuses to replace an existing seed unless you pass `--force`.

`layerx environment current` shows the active profile, and `layerx environment list` shows every profile you have configured.

## Get credentials

For the hosted testnet and for production you provision with a short-lived identity session. The CLI reads it from standard input and stores it in your operating system credential store, so it never lands in your shell history or a dotfile:

```text
layerx auth set --environment testnet
layerx auth status --environment testnet
```

For the emulator you create a local account keyed by an Ed25519 seed the CLI generates from operating-system randomness. You never type key material:

```text
layerx key create dev
layerx account create --key dev --initial-amount 1000000
```

## Confirm it works before you write code

```text
layerx payment test --from "$LAYERX_SOURCE" --to "$LAYERX_DESTINATION" \
  --currency "$LAYERX_CURRENCY" --amount "$LAYERX_AMOUNT" \
  --idempotency-key "$LAYERX_PAYMENT_KEY" --json
```

That performs a real quote and a real commit against the active endpoint and prints the journey. If it works, your language quickstart will work.

Native `layerx wallet` and `layerx token` commands are a separate surface,
documented in the [CLI wallet quickstart](https://github.com/Sidiora-Labs/LayerX-Network/blob/86fa9803386b75d1fea152cb40cacb3f1069cf4a/platform/cli/README.md#wallet-quickstart).
Wallet creation on the emulator registers an identity and opens its main account.
Hosted wallet send is implemented through the shared debit-authorization signer
and verifies the returned receipt; it requires the RPC transport, a receipt policy,
and a fee limit. Emulator send refuses because it cannot obtain the authenticated
identity sequence. The hosted `wallet create` command still refuses without a
published identity-and-main-account registration contract. These are distinct
paths; the release candidate's executed journeys must qualify each supported one.

## Five-minute agent-runtime installation

A2A uses the hosted gateway, not the emulator. Create or import the Ed25519 key already bound to your funded account, select the hosted environment, and keep the source account and asset identifiers nearby.

MCP is daemon-bound, not gateway-bound. `layerx install mcp` registers a launch line for `layerx mcp serve` against the agent-daemon binding document that agent-daemon enrolment wrote; it opens that document and refuses when it is absent. No gateway key, environment or payment flag takes part, because the daemon holds every credential the tools need. Name one or more supported hosts:

```text
layerx install mcp --host claude-code --json
```

`--daemon-binding <path>` overrides the binding document the CLI resolves beside its configuration, and `--read-only` registers the read-only tool set. Supported hosts are `layerx`, `claude-code`, `claude-desktop`, `cursor` and `vscode`.

For A2A, the already stored identity session provisions a separate least-scoped key and the command starts the loopback runtime:

```text
layerx install a2a --environment testnet --key agent-runtime \
  --source-account "$LAYERX_SOURCE_ACCOUNT" --asset "$LAYERX_ASSET" \
  --listen 127.0.0.1:9433 --json
layerx a2a status --json
```

The A2A installation result also names an owner-only `authorization.credential_file`. Keep that file local and send its value as `Authorization: Bearer <value>` on every JSON-RPC POST. The public Agent Card stays readable without that credential. The installed runtime rejects missing or incorrect bearer values before it invokes a read or payment tool.

The A2A installation JSON contains only the executable arguments, the configuration path, a non-secret gateway key identifier, an opaque credential alias and the path to that local authorization file. Gateway secrets and signing seeds stay in the operating-system credential store. A normal reinstall is idempotent; add `--rotate` to replace both the component's gateway key and its local A2A bearer before restarting the managed process. `--read-only` removes the payment tool and issues only `receipt:read` scope.

## Install an SDK

| Language | Install |
|---|---|
| TypeScript | `npm install @sidiora/layerx-sdk` |
| Python | `python3 -m pip install layerx-sdk` |
| Go | `go get github.com/Sidiora-Labs/LayerX-Network/platform/sdk/go` |
| Java and Kotlin | Add `com.sidiora.layerx:layerx-sdk:0.1.0` to your build |
| Swift | Add the `LayerXSDK` package to `Package.swift` |
| C# | `dotnet add package LayerX.Sdk` |
| Rust | `cargo add layerx-sdk` |

Those coordinates are the beta artifact set declared in the canonical beta contract. This checkout does not provide a published artifact manifest that verifies their availability, so the install commands above are not qualified installation instructions. Build from `platform/sdk/*`, `agent/sdk/*` and `agent/crates/*` until a release publishes the coordinate.

## What your application reads from the environment

A server-side integration needs exactly two values, both supplied to you:

| Variable | What it is |
|---|---|
| `LAYERX_API_URL` | The base URL of the environment you chose |
| `LAYERX_API_TOKEN` | A bearer token identifying your account |

Every SDK wraps the token in a secret container that redacts itself in logs and zeroes its storage when destroyed. No quickstart on this site asks you to construct signing material.

A mobile or browser application gets a different arrangement entirely: it holds no long-lived credential and exchanges a publishable configuration for short-lived session tokens through a broker you run. See [iOS](framework-ios.html) and [Android](framework-android.html).

## Enforced by

| Capability | Layer | What that means here |
|---|---|---|
| Atomic settlement | `protocol` | True on the emulator, the testnet and production alike, because it is the same transition function. |
| Refusal to publish a secret | `service` | The mobile configuration accepts publishable values only, and the Next.js bundle scanner fails a build whose client bundle contains a declared secret. |
| Testnet faucet funding | `hosted-surface` | Faucet eligibility is a hosted control. Once issued, test funds are ordinary protocol balances. |
| Scheduled testnet resets | `hosted-surface` | Nothing on the testnet is durable, and no protocol rule promises otherwise. |

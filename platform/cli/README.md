# LayerX developer CLI

Binary `layerx` (`platform/cli`). The wallet and token commands described here
are on the testnet branch. The complete payment path is
[`docs/wiki/PaymentsQuickstart.md`](../../docs/wiki/PaymentsQuickstart.md).

Wallet commands are `create`, `import`, `list`, `balance`, `history`,
`receipt`, `send`, `open-account`, `estimate-fee`, and `watch`. Token commands
are `create`, `mint`, `burn`, `transfer`, `info`, and `list`. Public writes
require `--rpc`, `--gateway-credential`, `--receipt-policy`, and `--fee-limit`.
The RPC URL must end in `/rpc`; remote RPC requires HTTPS.

Send and token transfer read the identity and source-account sequences
independently, sign the native debit authorization, then sign the canonical
outer activity. Pending outcomes retain the activity id for recovery. Each new
invocation creates a new idempotency key, so do not repeat an uncertain write.
Public wallet registration and DID history are unavailable and fail without
inventing results.

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

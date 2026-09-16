# Rust SDK quickstart

Package: `layerx-sdk` at `agent/crates/layerx-sdk`. There is no
`platform/sdk/rust`.

This crate is the only shipped SDK with typed public JSON-RPC
wrappers for `lx_register`, `lx_requestFunds`, and
`lx_sendActivity` (`agent/crates/layerx-sdk/src/rpc.rs`,
`agent/crates/layerx-sdk/src/register.rs`).

---

## Register

`RpcClient::register` takes a 32-byte Ed25519 public key and its
64-byte signature over `layerx_sdk::register::binding`. The gateway
derives the subject; the caller cannot name another principal.

```rust
use layerx_sdk::register::{binding, TENANT};
use layerx_sdk::rpc::RpcClient;

fn register(rpc: &RpcClient, public_key: [u8; 32], signature: &[u8; 64])
    -> Result<layerx_sdk::register::Registration, layerx_sdk::rpc::RpcError>
{
    let _digest = binding(TENANT, &public_key);
    rpc.register(public_key, signature)
}
```

`TENANT` is the literal `"beta"`. `binding` / `subject` are the
same SHA-256 tagged constructions the gateway uses.

---

## Fund from faucet

`RpcClient::request_funds` calls `lx_requestFunds` and accepts only
a grant the faucet confirmed as `funded`.

The OpenRPC contract requires
`Authorization: Bearer <identity session token>`
(`platform/hosted/gateway/openrpc.json`). `RpcClient::connect`
accepts only `Option<LayerXKeyCredential>`, which attaches
`LayerX-Key` (`layerx_sdk::programs::LayerXKeyCredential`). This
crate has no Bearer session constructor. The method exists; the
session header the gateway requires is not something `RpcClient`
can build today. The hosted HTTP claim
`POST /v1/faucet/claims` with Bearer is on
[Hosted faucet](HostedFaucet.md).

```rust
use layerx_sdk::rpc::{FaucetGrant, RpcClient, RpcError};

fn request_funds(rpc: &RpcClient, did: &str, public_key: &[u8; 32])
    -> Result<FaucetGrant, RpcError>
{
    rpc.request_funds(did, public_key)
}
```

A successful `FaucetGrant` exposes `funding_id`, `transaction_id`,
`amount`, and `network`.

---

## Send

```rust
use layerx_sdk::rpc::{Commitment, RpcClient, RpcError};

fn send(rpc: &RpcClient, canonical: &[u8]) -> Result<serde_json::Value, RpcError> {
    rpc.send_activity(canonical, Commitment::Executed)
}
```

`Commitment` is `Executed`, `Batched`, or `Finalised`. Canonical
bytes must be non-empty and at most 524_288 bytes. Connect with a
`LayerXKeyCredential` that holds `activity:write` or the gateway
refuses the method. `RpcClient::send_activity` returns the RPC
outcome, including pending; it is not itself a verified receipt.

`layerx_sdk::wallet::Wallet::send` is a higher-level Asset ordinal-5
path that still ends in the same RPC submission.

```rust
use layerx_sdk::production::SecretBytes;
use layerx_sdk::programs::LayerXKeyCredential;
use layerx_sdk::rpc::RpcClient;

fn connect(endpoint: &str, key_id: &str, secret: &[u8]) -> Result<RpcClient, layerx_sdk::rpc::RpcError> {
    let secret = SecretBytes::new(secret).map_err(|_| layerx_sdk::rpc::RpcError::InvalidRequest)?;
    let credential = LayerXKeyCredential::new(key_id, secret).map_err(layerx_sdk::rpc::RpcError::Configuration)?;
    RpcClient::connect(endpoint, Some(credential))
}
```

---

## Verify a receipt

```rust
use layerx_proof::receipt::AuthorizedBatch;
use layerx_sdk::production::verify_receipt;

fn verify(
    receipt: &[u8],
    batch_id: [u8; 32],
    asset: [u8; 32],
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    sequencer_public_key: [u8; 32],
) -> Result<layerx_proof::receipt::VerifiedReceipt, layerx_sdk::production::ReceiptVerificationFailure> {
    let authorised = AuthorizedBatch::new(
        batch_id,
        asset,
        previous_state_root,
        resulting_state_root,
        sequencer_public_key,
    );
    verify_receipt(receipt, &authorised)
}
```

The five batch facts must come from a source you already trust.
Taking them from the same service that handed you the receipt
proves nothing.

[SDK quickstarts](SdkQuickstarts.md) · [Home](Home.md)

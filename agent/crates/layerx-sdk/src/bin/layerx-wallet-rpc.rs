use std::future::Future;
use std::io::{Read as _, Write as _};
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

use layerx_crypto::payments::Payment;
use layerx_crypto::signer::KeystoreSigner;
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_sdk::production::SecretBytes;
use layerx_sdk::programs::LayerXKeyCredential;
use layerx_sdk::rpc::{Commitment, RpcClient, RpcError};
use layerx_sdk::rpc_verification::{ReceiptPolicy, VerifiedRpcReceipt};
use layerx_sdk::wallet::{PaymentOptions, Wallet};
use layerx_types::payload::ModuleId;
use serde_json::{json, Value};

struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
fn run<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, ()> {
    v[key].as_str().ok_or(())
}
fn number(v: &Value, key: &str) -> Result<u64, ()> {
    let text = text(v, key)?;
    let n: u64 = text.parse().map_err(|_| ())?;
    if n.to_string() != text {
        return Err(());
    }
    Ok(n)
}
fn bytes(v: &Value, key: &str) -> Result<Vec<u8>, ()> {
    let s = text(v, key)?;
    if s.len() > 1_048_576
        || !s.len().is_multiple_of(2)
        || !s.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(());
    }
    s.as_bytes()
        .chunks_exact(2)
        .map(|b| {
            std::str::from_utf8(b)
                .ok()
                .and_then(|s| u8::from_str_radix(s, 16).ok())
                .ok_or(())
        })
        .collect()
}
fn fixed<const N: usize>(v: &Value, key: &str) -> Result<[u8; N], ()> {
    bytes(v, key)?.try_into().map_err(|_| ())
}
fn hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|b| {
            [
                char::from(H[usize::from(b >> 4)]),
                char::from(H[usize::from(b & 15)]),
            ]
        })
        .collect()
}
fn commitment(v: &Value) -> Result<Commitment, ()> {
    match text(v, "commitment")? {
        "executed" => Ok(Commitment::Executed),
        "batched" => Ok(Commitment::Batched),
        "finalised" => Ok(Commitment::Finalised),
        _ => Err(()),
    }
}
fn verified(receipt: &VerifiedRpcReceipt) -> Result<Value, ()> {
    let p = receipt.receipt().protocol().ok_or(())?;
    Ok(
        json!({"receipt":hex(receipt.canonical_bytes()), "activity_id":hex(&p.activity_id()), "result_code":p.result_code(), "state":if p.result_code() == 0 { "executed" } else { "refused" }, "commitment":receipt.commitment().as_str()}),
    )
}
enum BridgeError {
    Invalid,
    Rpc(RpcError),
}
impl From<()> for BridgeError {
    fn from((): ()) -> Self {
        Self::Invalid
    }
}
fn execute(v: &Value) -> Result<Value, BridgeError> {
    let c = &v["configuration"];
    let credential = if c.get("key_id").is_some() {
        Some(
            LayerXKeyCredential::new(
                text(c, "key_id")?,
                SecretBytes::new(text(c, "key_secret")?.as_bytes()).map_err(|_| ())?,
            )
            .map_err(|_| ())?,
        )
    } else {
        None
    };
    let rpc = RpcClient::connect(text(c, "endpoint")?, credential).map_err(|_| ())?;
    let policy = ReceiptPolicy {
        protocol_version: u16::try_from(number(c, "protocol_version")?).map_err(|_| ())?,
        network_id: u32::try_from(number(c, "network_id")?).map_err(|_| ())?,
        sequencer: SequencerAuthorization::new(
            fixed(c, "sequencer_id")?,
            fixed(c, "sequencer_key")?,
            number(c, "first_batch")?,
            number(c, "last_batch")?,
        ),
        trusted_checkpoint_context_digest: if c.get("checkpoint_context_digest").is_some() {
            Some(fixed(c, "checkpoint_context_digest")?)
        } else {
            None
        },
    };
    match text(v, "action")? {
        "accounts" => return rpc.get_balances(text(v, "did")?).map_err(BridgeError::Rpc),
        "balance" => {
            return rpc
                .wallet(fixed(c, "native_asset")?)
                .balance(text(v, "did")?, fixed(v, "asset")?)
                .map_err(BridgeError::Rpc)
        }
        "wait" => {
            return verified(
                &rpc.wait_for(
                    fixed(v, "activity_id")?,
                    commitment(v)?,
                    &policy,
                    Duration::from_millis(number(v, "timeout_ms")?),
                )
                .map_err(BridgeError::Rpc)?,
            )
            .map_err(BridgeError::from)
        }
        "submit" => {}
        _ => return Err(BridgeError::Invalid),
    }
    let signer = KeystoreSigner::new(text(c, "signer_socket")?, fixed(c, "signer_public_key")?)
        .map_err(|_| ())?;
    let wallet = Wallet {
        rpc: &rpc,
        signer: &signer,
        policy: &policy,
        native_asset: fixed(c, "native_asset")?,
    };
    let options = PaymentOptions {
        actor: text(v, "actor")?.into(),
        idempotency_key: fixed(v, "idempotency_key")?,
        fee_limit: text(v, "fee_limit")?.parse().map_err(|_| ())?,
        not_before: number(v, "not_before")?,
        not_after: number(v, "not_after")?,
        commitment: commitment(v)?,
        wait_timeout: Duration::from_millis(number(v, "timeout_ms")?),
    };
    let ordinal = u16::try_from(number(v, "ordinal")?).map_err(|_| ())?;
    let payload = bytes(v, "payload")?;
    let receipt = if ordinal == 5 {
        run(wallet.send(&payload, &options))
    } else {
        let payment = Payment::decode(ModuleId::Asset, ordinal, &payload, options.actor.as_bytes())
            .map_err(|_| ())?;
        run(wallet.payment(payment, &options))
    }
    .map_err(BridgeError::Rpc)?;
    verified(&receipt).map_err(BridgeError::from)
}
fn main() {
    let mut input = Vec::new();
    let outcome = std::io::stdin()
        .take(2_097_153)
        .read_to_end(&mut input)
        .map_err(|_| BridgeError::Invalid)
        .and_then(|_| {
            if input.len() > 2_097_152 {
                return Err(BridgeError::Invalid);
            }
            let value: Value = serde_json::from_slice(&input).map_err(|_| ())?;
            execute(&value)
        });
    let output = match outcome {
        Ok(value) => json!({"ok":true,"result":value}),
        Err(BridgeError::Rpc(RpcError::Remote {
            code,
            message,
            data,
        })) => json!({"ok":false,"error":{"code":code,"message":message,"data":data}}),
        Err(BridgeError::Rpc(RpcError::Pending { activity_id })) => {
            json!({"ok":false,"error":{"kind":"pending","activity_id":hex(&activity_id)}})
        }
        Err(_) => json!({"ok":false,"error":{"kind":"wallet_request_refused_or_unavailable"}}),
    };
    if let Ok(bytes) = serde_json::to_vec(&output) {
        let _ = std::io::stdout().write_all(&bytes);
    }
}

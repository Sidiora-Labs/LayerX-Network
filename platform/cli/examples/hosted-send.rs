use std::future::Future;
use std::io::{Read as _, Write as _};
use std::sync::Arc;
use std::task::{Context, Poll, Wake};

use layerx_crypto::local::LocalSigner;
use layerx_crypto::send::SendDebit;
use layerx_crypto::signer::Signer as _;
use layerx_platform_cli::wallet_signing::{PreparedPayment, SigningFacts};
use serde::Deserialize;
use serde_json::json;
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    seed: String,
    actor: String,
    from: String,
    to: String,
    from_name: String,
    to_name: String,
    asset: String,
    amount: String,
    source_sequence: u64,
    identity_sequence: u64,
    network_id: u32,
    not_before: u64,
    not_after: u64,
    idempotency_key: String,
}

fn fixed(text: &str) -> Result<[u8; 32], String> {
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("invalid fixed hexadecimal field".into());
    }
    let mut result = [0; 32];
    for (index, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
            .map_err(|_| "invalid hexadecimal field")?;
    }
    Ok(result)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                char::from(DIGITS[usize::from(byte >> 4)]),
                char::from(DIGITS[usize::from(byte & 15)]),
            ]
        })
        .collect()
}

struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

fn complete<F: Future>(future: F) -> F::Output {
    let waker = Arc::new(ThreadWake(std::thread::current())).into();
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}

fn run() -> Result<(), String> {
    let mut input = Zeroizing::new(String::new());
    std::io::stdin()
        .take(8193)
        .read_to_string(&mut input)
        .map_err(|_| "cannot read signing request")?;
    if input.len() > 8192 {
        return Err("signing request exceeds its bound".into());
    }
    let request: Request = serde_json::from_str(&input).map_err(|_| "invalid signing request")?;
    let seed = Zeroizing::new(request.seed);
    let signer = LocalSigner::new(fixed(&seed)?);
    if request.actor != format!("did:layerx:{}", hex(&signer.public_key())) {
        return Err("signer does not own the requested actor".into());
    }
    let amount: u128 = request.amount.parse().map_err(|_| "invalid amount")?;
    if amount == 0 || amount.to_string() != request.amount {
        return Err("invalid amount".into());
    }
    for (name, identifier) in [
        (&request.from_name, &request.from),
        (&request.to_name, &request.to),
    ] {
        let account = layerx_types::account::AccountId::parse(name)
            .map_err(|error| format!("invalid account: {error:?}"))?;
        let expected = layerx_wire::hash::account_id_for_protocol(&account, 3)
            .map_err(|error| format!("invalid account identifier: {error:?}"))?;
        if hex(&expected) != *identifier {
            return Err("account name and identifier differ".into());
        }
    }
    if request.from_name != format!("agent:{}:main", request.actor)
        && request.from_name != format!("agent:{}:asset:{}", request.actor, request.asset)
    {
        return Err("source account is not owned by the signer".into());
    }
    let idempotency_key = fixed(&request.idempotency_key)?;
    let debit = SendDebit {
        from: fixed(&request.from)?,
        to: fixed(&request.to)?,
        asset: fixed(&request.asset)?,
        amount,
        source_sequence: request.source_sequence,
        idempotency_key,
        expires_at: request.not_after,
        context_hash: [0; 32],
        conditions: Vec::new(),
        authorization_kind: 1,
        network_id: request.network_id,
        protocol_version: 3,
    };
    let payload = complete(debit.sign(&signer)).map_err(|error| format!("{error:?}"))?;
    let facts = SigningFacts {
        actor: &request.actor,
        public_key: signer.public_key(),
        network_id: request.network_id,
        identity_next_sequence: request.identity_sequence,
        not_before_ms: request.not_before,
        expires_at_ms: request.not_after,
        fee_limit: 1_000_000_000_000,
        idempotency_key,
    };
    let (canonical, activity) =
        PreparedPayment::from_send(&payload, &facts)?.sign_with_id(&signer)?;
    let output = json!({"canonical": hex(&canonical), "activity_id": hex(&activity)});
    writeln!(std::io::stdout(), "{output}").map_err(|_| "cannot write signed activity".into())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("hosted SEND refused: {error}");
        std::process::exit(1);
    }
}

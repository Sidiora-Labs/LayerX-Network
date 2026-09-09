use layerx_crypto::send::{encode_send_envelope, EnvelopeOptions, SendCondition, SendDebit};
use layerx_crypto::signer::{sign_disclosed, LocalSigner, Signer, SigningDisclosure};
use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
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
fn debit() -> SendDebit {
    SendDebit {
        from: [1; 32],
        to: [2; 32],
        asset: [3; 32],
        amount: 123,
        source_sequence: 7,
        idempotency_key: [4; 32],
        expires_at: 100,
        context_hash: [5; 32],
        conditions: vec![SendCondition {
            kind: 1,
            timestamp: 10,
        }],
        authorization_kind: 1,
        network_id: 17,
        protocol_version: 3,
    }
}
#[test]
fn disclosed_debit_and_envelope_sign_independent_domains() -> Result<(), Box<dyn std::error::Error>>
{
    let signer = LocalSigner::new([42; 32]);
    let debit = debit();
    let message = debit.authorization_message()?;
    let request = debit.signing_request(&message)?;
    assert!(
        matches!(request.disclosure(), SigningDisclosure::SendDebit(actual) if actual == &debit)
    );
    let payload = run(debit.sign(&signer))?;
    let options = EnvelopeOptions {
        actor: "did:layerx:alice",
        public_key: signer.public_key(),
        network_id: 17,
        protocol_version: 3,
        identity_sequence: 19,
        idempotency_key: [4; 32],
        fee_limit: 20,
        not_before: 10,
        not_after: 100,
    };
    let encoded = encode_send_envelope(&payload, &options)?;
    assert_eq!(encoded.disclosure.envelope_sequence(), 19);
    assert_eq!(encoded.disclosure.payload_sequence()?, Some(7));
    let signature = run(sign_disclosed(
        &signer,
        &encoded.canonical,
        &encoded.disclosure,
        &encoded.registry,
    ))?;
    let debit_signature = run(signer.sign(request))?;
    assert_ne!(signature.as_bytes(), debit_signature.as_bytes());
    for offset in 0..message.len() {
        let mut changed = message.clone();
        changed[offset] ^= 1;
        assert!(debit.signing_request(&changed).is_err());
    }
    for offset in 0..payload.len() {
        let mut changed = payload.clone();
        changed[offset] ^= 1;
        assert!(
            encode_send_envelope(&changed, &options).is_err(),
            "offset {offset}"
        );
    }
    let mut changed = debit.clone();
    changed.source_sequence += 1;
    assert!(changed
        .encode_signed(signer.public_key(), *debit_signature.as_bytes())
        .is_err());
    let mut changed = debit.clone();
    changed.context_hash[0] ^= 1;
    assert!(changed
        .encode_signed(signer.public_key(), *debit_signature.as_bytes())
        .is_err());
    assert!(debit
        .encode_signed(
            LocalSigner::new([43; 32]).public_key(),
            *debit_signature.as_bytes()
        )
        .is_err());
    Ok(())
}
#[test]
fn malformed_debits_never_reach_signing() {
    let mut d = debit();
    d.amount = 0;
    assert!(d.authorization_message().is_err());
    let mut d = debit();
    d.to = d.from;
    assert!(d.authorization_message().is_err());
    let mut d = debit();
    d.conditions = vec![
        SendCondition {
            kind: 1,
            timestamp: 1
        };
        9
    ];
    assert!(d.authorization_message().is_err());
    let mut d = debit();
    d.conditions[0].kind = 3;
    assert!(d.authorization_message().is_err());
    let mut d = debit();
    d.authorization_kind = 7;
    assert!(d.authorization_message().is_err());
    let mut d = debit();
    d.network_id = 0;
    assert!(d.authorization_message().is_err());
    let mut d = debit();
    d.protocol_version = 0;
    assert!(d.authorization_message().is_err());
}

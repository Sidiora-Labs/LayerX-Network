use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

mod support;

use layerx_crypto::disclosure::{bind, AmountRole, CounterpartyRole, DisclosureError, Expiry};
use layerx_crypto::ed25519;
use layerx_crypto::signer::{sign_disclosed, LocalSigner, SignError, Signer};
use layerx_crypto::SignatureMessage;
use layerx_wire::hash::Domain;
use layerx_wire::limits::PROTOCOL_VERSION;

struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

fn ready<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::from(Arc::new(NoopWake));
    let mut context = Context::from_waker(&waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("local signer future unexpectedly blocked"),
    }
}

#[test]
fn canonical_send_discloses_every_authorised_semantic_and_reencodes_exactly() {
    let canonical = support::canonical_send(25);
    let registry = support::registry();
    let Ok(disclosure) = bind(&canonical, &registry) else {
        panic!("canonical send disclosure rejected");
    };
    assert_eq!(disclosure.activity_type.value(), 0x0001_0005);
    assert_eq!(disclosure.actor, b"did:layerx:alice");
    assert_eq!(disclosure.authority.len(), 32);
    assert_eq!(disclosure.counterparties.len(), 2);
    assert_eq!(disclosure.counterparties[0].role, CounterpartyRole::Payer);
    assert_eq!(disclosure.counterparties[0].account, [0x11; 32]);
    assert_eq!(
        disclosure.counterparties[1].role,
        CounterpartyRole::Recipient
    );
    assert_eq!(disclosure.counterparties[1].account, [0x22; 32]);
    assert_eq!(disclosure.amounts.len(), 1);
    assert_eq!(disclosure.amounts[0].role, AmountRole::Transfer);
    assert_eq!(disclosure.amounts[0].value, 25);
    assert_eq!(disclosure.asset, [0x33; 32]);
    assert_eq!(disclosure.fee_limit, 20);
    assert_eq!(
        disclosure.expiry,
        Expiry {
            not_before: 10,
            not_after: 100,
            payload_expires_at: 100,
        }
    );
    assert_eq!(disclosure.idempotency_key, [0x71; 32]);
    assert_eq!(disclosure.reencode(), Ok(canonical));
}

#[test]
fn bridge_deposit_credit_discloses_and_signs_the_exact_compiled_semantics() {
    let canonical = support::canonical_bridge_credit(25);
    let registry = support::bridge_registry();
    let Ok(disclosure) = bind(&canonical, &registry) else {
        panic!("canonical bridge credit disclosure rejected");
    };
    assert_eq!(disclosure.activity_type.value(), 0x0008_0001);
    assert_eq!(disclosure.counterparties[0].account, [0x11; 32]);
    assert_eq!(disclosure.counterparties[1].account, [0x22; 32]);
    assert_eq!(disclosure.asset, [0x33; 32]);
    assert_eq!(disclosure.amounts[0].value, 25);
    assert_eq!(disclosure.idempotency_key, [0x71; 32]);
    assert_eq!(disclosure.reencode(), Ok(canonical.clone()));
    let signer = LocalSigner::new([0xa5; 32]);
    assert!(ready(sign_disclosed(&signer, &canonical, &disclosure, &registry)).is_ok());
}

#[test]
fn bridge_withdrawal_discloses_and_signs_the_exact_compiled_semantics() {
    let canonical = support::canonical_bridge_withdraw(25);
    let registry = support::bridge_withdraw_registry();
    let Ok(disclosure) = bind(&canonical, &registry) else {
        panic!("canonical bridge withdrawal disclosure rejected");
    };
    assert_eq!(disclosure.activity_type.value(), 0x0008_0002);
    assert_eq!(disclosure.counterparties[0].account, [0x11; 32]);
    assert_eq!(disclosure.counterparties[1].account, [0x22; 32]);
    assert_eq!(disclosure.asset, [0x33; 32]);
    assert_eq!(disclosure.amounts[0].value, 25);
    assert_eq!(disclosure.idempotency_key, [0x71; 32]);
    assert_eq!(disclosure.reencode(), Ok(canonical.clone()));
    let signer = LocalSigner::new([0xa5; 32]);
    assert!(ready(sign_disclosed(&signer, &canonical, &disclosure, &registry)).is_ok());
}

#[test]
fn disclosed_signing_uses_the_exact_canonical_bytes() {
    let canonical = support::canonical_send(25);
    let registry = support::registry();
    let Ok(disclosure) = bind(&canonical, &registry) else {
        panic!("canonical send disclosure rejected");
    };
    let signer = LocalSigner::new([0xa5; 32]);
    let Ok(signature) = ready(sign_disclosed(&signer, &canonical, &disclosure, &registry)) else {
        panic!("matching disclosed activity was refused");
    };
    let Ok(message) =
        SignatureMessage::new(Domain::SignaturePreimage, PROTOCOL_VERSION, 17, &canonical)
    else {
        panic!("valid message scope rejected");
    };
    assert_eq!(
        ed25519::verify(&signer.public_key(), signature.as_bytes(), message),
        Ok(())
    );
}

#[test]
fn small_payment_disclosure_cannot_authorise_large_payment_bytes() {
    let canonical = support::canonical_send(9_000_000);
    let registry = support::registry();
    let Ok(mut disclosure) = bind(&canonical, &registry) else {
        panic!("canonical large send disclosure rejected");
    };
    disclosure.amounts[0].value = 1;
    let signer = LocalSigner::new([0xa5; 32]);
    let refusal = ready(sign_disclosed(&signer, &canonical, &disclosure, &registry));
    assert_eq!(refusal, Err(SignError::DisclosureMismatch("amounts")));
    let Err(error) = refusal else {
        panic!("mismatching disclosure unexpectedly signed");
    };
    assert!(error.to_string().contains("amounts"));
}

#[test]
fn byte_substitution_names_the_semantic_field_that_changed() {
    let small = support::canonical_send(1);
    let large = support::canonical_send(9_000_000);
    let registry = support::registry();
    let Ok(disclosure) = bind(&small, &registry) else {
        panic!("canonical small send disclosure rejected");
    };
    let signer = LocalSigner::new([0xa5; 32]);
    assert_eq!(
        ready(sign_disclosed(&signer, &large, &disclosure, &registry)),
        Err(SignError::DisclosureMismatch("amounts"))
    );
}

#[test]
fn payload_commitment_mismatch_is_fail_closed() {
    let mut corrupted = support::canonical_send(25);
    let last = corrupted.len().saturating_sub(1);
    corrupted[last] ^= 1;
    assert_eq!(
        bind(&corrupted, &support::registry()).err(),
        Some(DisclosureError::PayloadHash)
    );
}

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_crypto::payments::{Grant, Payment, ReceiverAuthorization};
use layerx_types::account::AccountId;
use layerx_wire::{encode::Encoder, hash::Domain};
use sha2::{Digest as _, Sha256};

pub struct SignedReceiveRequest<'a> {
    pub from: &'a AccountId,
    pub to: &'a AccountId,
    pub asset: [u8; 32],
    pub amount: u128,
    pub sequence: u64,
    pub idempotency_key: [u8; 32],
    pub network_id: u32,
    pub protocol_version: u16,
}

fn checked<T, E: std::fmt::Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("signed receive fixture: {error:?}"))
}

#[must_use]
pub fn signed_receive(request: &SignedReceiveRequest<'_>) -> Vec<u8> {
    let payer = SigningKey::from_bytes(&[0x71; 32]);
    let receiver = SigningKey::from_bytes(&[0x51; 32]);
    let from = checked(layerx_wire::hash::account_id_for_protocol(
        request.from,
        request.protocol_version,
    ));
    let to = checked(layerx_wire::hash::account_id_for_protocol(
        request.to,
        request.protocol_version,
    ));
    let purpose_hash = [0x64; 32];
    let mut grant_fields = Encoder::new(250);
    for field in [from, to, request.asset] {
        checked(grant_fields.fixed(&field));
    }
    checked(grant_fields.u128(request.amount));
    checked(grant_fields.u128(request.amount));
    checked(grant_fields.u8(0));
    checked(grant_fields.u64(0));
    checked(grant_fields.u64(10_000));
    checked(grant_fields.fixed(&purpose_hash));
    checked(grant_fields.u8(0));
    checked(grant_fields.fixed(&[0; 32]));
    checked(grant_fields.u64(0));
    checked(grant_fields.fixed(&payer.verifying_key().to_bytes()));
    let mut hasher = Sha256::new();
    hasher.update(Domain::AuthorityHash.tag());
    hasher.update(b"LXP:GRANT:v1");
    hasher.update(grant_fields.finish());
    let id: [u8; 32] = hasher.finalize().into();
    let grant = Grant {
        id,
        from,
        recipient: to,
        asset: request.asset,
        per_draw_maximum: request.amount,
        allowance: request.amount,
        recurring: false,
        window_length: 0,
        expiration: 10_000,
        purpose_hash,
        has_reference: false,
        reference_hash: [0; 32],
        revocation_sequence: 0,
        public_key: payer.verifying_key().to_bytes(),
        signature: payer.sign(&id).to_bytes(),
    };
    let mut context = Sha256::new();
    context.update(Domain::ContextHash.tag());
    context.update(purpose_hash);
    let context_hash: [u8; 32] = context.finalize().into();
    let mut preimage = Encoder::new(512);
    checked(preimage.fixed(b"LXP:RECEIVE:v1"));
    for field in [from, to, request.asset] {
        checked(preimage.fixed(&field));
    }
    checked(preimage.u128(request.amount));
    checked(preimage.fixed(&id));
    checked(preimage.u64(request.sequence));
    checked(preimage.fixed(&request.idempotency_key));
    checked(preimage.fixed(&context_hash));
    checked(preimage.u8(1));
    checked(preimage.fixed(&to));
    checked(preimage.fixed(&context_hash));
    checked(preimage.u32(request.network_id));
    checked(preimage.u16(request.protocol_version));
    let mut signed = Sha256::new();
    signed.update(Domain::SignaturePreimage.tag());
    signed.update(preimage.finish());
    let signature = receiver.sign(&signed.finalize()).to_bytes();
    let payment = Payment::Receive {
        from,
        to,
        asset: request.asset,
        amount: request.amount,
        grant: id,
        sequence: request.sequence,
        idempotency_key: request.idempotency_key,
        context_hash,
        receiver_authorization: ReceiverAuthorization {
            kind: 1,
            controller: to,
            public_key: receiver.verifying_key().to_bytes(),
            signature,
            signed_context_hash: context_hash,
            network_id: request.network_id,
            protocol_version: request.protocol_version,
        },
        payer_grant: Box::new(grant),
    };
    checked(payment.encode(b""))
}

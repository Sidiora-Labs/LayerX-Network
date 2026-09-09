use std::time::Duration;

use layerx_crypto::disclosure::{bind, Disclosure};
pub use layerx_crypto::payments::{Grant, Payment, Registration};
use layerx_crypto::signer::{sign_disclosed, Signer};
use layerx_types::activity::{
    Authority, EnvelopeBuilder, Signature, TimestampBound, UnsignedEnvelope,
};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry, Payload};
use layerx_wire::activity::{decode_signed, encode_signed_envelope, encode_unsigned_envelope};
use layerx_wire::hash::{activity_id, Domain};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::rpc::{Commitment, RpcClient, RpcError};
use crate::rpc_verification::{ReceiptPolicy, VerifiedRpcReceipt};

pub struct PaymentOptions {
    pub actor: String,
    pub idempotency_key: [u8; 32],
    pub fee_limit: u128,
    pub not_before: u64,
    pub not_after: u64,
    pub commitment: Commitment,
    pub wait_timeout: Duration,
}

pub struct Wallet<'a> {
    pub rpc: &'a RpcClient,
    pub signer: &'a dyn Signer,
    pub policy: &'a ReceiptPolicy,
    pub native_asset: [u8; 32],
}

pub struct PreparedPayment {
    envelope: UnsignedEnvelope,
    registry: ModuleRegistry,
    canonical: Vec<u8>,
    disclosure: Disclosure,
}

impl PreparedPayment {
    #[must_use]
    pub const fn disclosure(&self) -> &Disclosure {
        &self.disclosure
    }
    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }
}

impl Wallet<'_> {
    #[must_use]
    pub const fn token(&self) -> &Self {
        self
    }
    #[must_use]
    pub const fn grant(&self) -> &Self {
        self
    }
    /// # Errors
    /// Preserves unavailable enumeration and RPC errors.
    pub fn accounts(&self, did: &str) -> Result<Value, RpcError> {
        self.rpc.get_balances(did)
    }
    /// # Errors
    /// Rejects invalid selectors and preserves RPC read errors.
    pub fn balance(&self, did: &str, asset: [u8; 32]) -> Result<Value, RpcError> {
        self.rpc.wallet(self.native_asset).balance(did, asset)
    }
    /// # Errors
    /// Refuses malformed signed Send authorization, stale source sequence or signer rejection.
    pub async fn send(
        &self,
        canonical_send_payload: &[u8],
        options: &PaymentOptions,
    ) -> Result<VerifiedRpcReceipt, RpcError> {
        let prepared = self.prepare_payload(ModuleId::Asset, 5, canonical_send_payload, options)?;
        self.execute(prepared, options).await
    }
    /// # Errors
    /// Preserves prepare, signing, submission and verification refusals.
    pub async fn open_account(
        &self,
        asset: [u8; 32],
        options: &PaymentOptions,
    ) -> Result<VerifiedRpcReceipt, RpcError> {
        self.payment(Payment::OpenAccount { asset }, options).await
    }
    /// # Errors
    /// Refuses malformed registration and preserves execution errors.
    pub async fn create(
        &self,
        registration: Registration,
        options: &PaymentOptions,
    ) -> Result<VerifiedRpcReceipt, RpcError> {
        self.payment(Payment::Register(registration), options).await
    }
    /// # Errors
    /// Preserves canonical encoding, signing and native execution errors.
    pub async fn mint(
        &self,
        asset: [u8; 32],
        to: [u8; 32],
        amount: u128,
        options: &PaymentOptions,
    ) -> Result<VerifiedRpcReceipt, RpcError> {
        self.payment(Payment::Mint { asset, to, amount }, options)
            .await
    }
    /// # Errors
    /// Preserves canonical encoding, signing and native execution errors.
    pub async fn burn(
        &self,
        asset: [u8; 32],
        from: [u8; 32],
        amount: u128,
        options: &PaymentOptions,
    ) -> Result<VerifiedRpcReceipt, RpcError> {
        self.payment(
            Payment::Burn {
                asset,
                from,
                amount,
            },
            options,
        )
        .await
    }
    /// # Errors
    /// Refuses malformed grants and preserves signing and execution errors.
    pub async fn issue(
        &self,
        grant: Grant,
        options: &PaymentOptions,
    ) -> Result<VerifiedRpcReceipt, RpcError> {
        self.payment(Payment::IssueGrant(grant), options).await
    }
    /// # Errors
    /// Preserves prepare, signing and native execution errors.
    pub async fn revoke(
        &self,
        grant: [u8; 32],
        revocation_sequence: u64,
        options: &PaymentOptions,
    ) -> Result<VerifiedRpcReceipt, RpcError> {
        self.payment(
            Payment::RevokeGrant {
                grant,
                revocation_sequence,
            },
            options,
        )
        .await
    }
    /// # Errors
    /// Requires the canonical Receive variant and preserves prepare/sign/execute errors.
    pub async fn draw(
        &self,
        receive: Payment,
        options: &PaymentOptions,
    ) -> Result<VerifiedRpcReceipt, RpcError> {
        if !matches!(receive, Payment::Receive { .. }) {
            return Err(RpcError::InvalidRequest);
        }
        self.payment(receive, options).await
    }
    /// # Errors
    /// Returns pending or refuses any missing or invalid commitment evidence.
    pub fn wait_for(
        &self,
        activity: [u8; 32],
        commitment: Commitment,
        timeout: Duration,
    ) -> Result<VerifiedRpcReceipt, RpcError> {
        self.rpc
            .wait_for(activity, commitment, self.policy, timeout)
    }

    /// # Errors
    /// Refuses malformed payment payloads and propagates preparation, signing and verification errors.
    pub async fn payment(
        &self,
        payment: Payment,
        options: &PaymentOptions,
    ) -> Result<VerifiedRpcReceipt, RpcError> {
        let (module, ordinal) = payment.activity_type();
        let payload = payment
            .encode(options.actor.as_bytes())
            .map_err(|_| RpcError::InvalidRequest)?;
        let prepared = self.prepare_payload(module, ordinal, &payload, options)?;
        self.execute(prepared, options).await
    }

    /// # Errors
    /// Requires canonical payload disclosure and fresh source and identity sequences.
    pub fn prepare_payload(
        &self,
        module: ModuleId,
        ordinal: u16,
        payload: &[u8],
        options: &PaymentOptions,
    ) -> Result<PreparedPayment, RpcError> {
        if options.wait_timeout > Duration::from_secs(300) {
            return Err(RpcError::InvalidRequest);
        }
        if options.commitment == Commitment::Finalised
            && self.policy.trusted_checkpoint_context_digest.is_none()
        {
            return Err(RpcError::MissingFinalityTrust);
        }
        let snapshot = self.rpc.get_sequence(&options.actor)?;
        if snapshot["did"] != options.actor
            || snapshot["verification"] != "authenticated_node_snapshot"
        {
            return Err(RpcError::InvalidResponse);
        }
        let sequence = decimal(&snapshot, "next_sequence")?;
        let prepared = prepare_payload(
            module,
            ordinal,
            payload,
            options,
            self.policy,
            self.signer.public_key(),
            sequence,
        )?;
        if let Some(payload_sequence) = prepared
            .disclosure
            .payload_sequence()
            .map_err(|_| RpcError::Verification)?
        {
            let source = if let Some(Payment::Receive { to, .. }) = &prepared.disclosure.payment {
                *to
            } else {
                prepared
                    .disclosure
                    .counterparties
                    .first()
                    .ok_or(RpcError::InvalidRequest)?
                    .account
            };
            if source_sequence(self.rpc, source)? != payload_sequence {
                return Err(RpcError::StaleSourceSequence);
            }
        }
        Ok(prepared)
    }

    /// # Errors
    /// Refuses signing and submission failures and incomplete receipt evidence.
    pub async fn execute(
        &self,
        prepared: PreparedPayment,
        options: &PaymentOptions,
    ) -> Result<VerifiedRpcReceipt, RpcError> {
        let signature = sign_disclosed(
            self.signer,
            &prepared.canonical,
            &prepared.disclosure,
            &prepared.registry,
        )
        .await
        .map_err(RpcError::Signing)?;
        let signed = prepared.envelope.attach_signature(
            Signature::new(signature.as_bytes()).map_err(|_| RpcError::InvalidRequest)?,
        );
        let canonical = encode_signed_envelope(&signed).map_err(|_| RpcError::InvalidRequest)?;
        let decoded =
            decode_signed(&canonical, &prepared.registry).map_err(|_| RpcError::Verification)?;
        let id = activity_id(&decoded).map_err(|_| RpcError::Verification)?;
        match self.rpc.send_activity(&canonical, options.commitment) {
            Ok(_)
            | Err(
                RpcError::Transport
                | RpcError::InvalidResponse
                | RpcError::Remote {
                    code: -32001 | -32005,
                    ..
                },
            ) => {}
            Err(error) => return Err(error),
        }
        self.wait_for(id, options.commitment, options.wait_timeout)
    }
}

fn source_sequence(rpc: &RpcClient, account: [u8; 32]) -> Result<u64, RpcError> {
    let id = crate::rpc::encode_hex(&account);
    let value = rpc.get_account(&id)?;
    if value["account_id"] != id {
        return Err(RpcError::InvalidResponse);
    }
    decimal(&value, "next_sequence")
}
fn decimal(value: &Value, key: &str) -> Result<u64, RpcError> {
    let text = value[key].as_str().ok_or(RpcError::InvalidResponse)?;
    let number: u64 = text.parse().map_err(|_| RpcError::InvalidResponse)?;
    if number.to_string() != text {
        return Err(RpcError::InvalidResponse);
    }
    Ok(number)
}

fn prepare_payload(
    module: ModuleId,
    ordinal: u16,
    payload: &[u8],
    options: &PaymentOptions,
    policy: &ReceiptPolicy,
    public_key: [u8; 32],
    identity_sequence: u64,
) -> Result<PreparedPayment, RpcError> {
    let kind = ActivityType::new(module, ordinal).map_err(|_| RpcError::InvalidRequest)?;
    let registered =
        ModuleRegistration::new(module, &[kind]).map_err(|_| RpcError::InvalidRequest)?;
    let registry = ModuleRegistry::new(&[registered]).map_err(|_| RpcError::InvalidRequest)?;
    let mut hash = Sha256::new();
    hash.update(Domain::PayloadHash.tag());
    hash.update(payload);
    let mut builder = EnvelopeBuilder::new();
    builder
        .protocol_version(policy.protocol_version)
        .map_err(|_| RpcError::InvalidRequest)?;
    builder
        .network_id(policy.network_id)
        .map_err(|_| RpcError::InvalidRequest)?;
    builder
        .activity_type(kind)
        .map_err(|_| RpcError::InvalidRequest)?;
    builder
        .actor_did(Did::new(options.actor.as_bytes()).map_err(|_| RpcError::InvalidRequest)?)
        .map_err(|_| RpcError::InvalidRequest)?;
    builder
        .authority(Authority::owner(&public_key).map_err(|_| RpcError::InvalidRequest)?)
        .map_err(|_| RpcError::InvalidRequest)?;
    builder
        .account_sequence(identity_sequence)
        .map_err(|_| RpcError::InvalidRequest)?;
    builder
        .timestamp_bound(
            TimestampBound::new(options.not_before, options.not_after)
                .map_err(|_| RpcError::InvalidRequest)?,
        )
        .map_err(|_| RpcError::InvalidRequest)?;
    builder
        .idempotency_key(IdempotencyKey::new(options.idempotency_key))
        .map_err(|_| RpcError::InvalidRequest)?;
    builder
        .fee_limit(Amount::from_u128(options.fee_limit))
        .map_err(|_| RpcError::InvalidRequest)?;
    builder
        .payload_hash(hash.finalize().into())
        .map_err(|_| RpcError::InvalidRequest)?;
    builder
        .payload(Payload::new(&registry, kind, payload).map_err(|_| RpcError::InvalidRequest)?)
        .map_err(|_| RpcError::InvalidRequest)?;
    let envelope = builder.build().map_err(|_| RpcError::InvalidRequest)?;
    let canonical = encode_unsigned_envelope(&envelope).map_err(|_| RpcError::InvalidRequest)?;
    let disclosure = bind(&canonical, &registry).map_err(|_| RpcError::Verification)?;
    Ok(PreparedPayment {
        envelope,
        registry,
        canonical,
        disclosure,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use layerx_proof::inclusion::SequencerAuthorization;
    #[test]
    fn prepared_receive_retains_independent_identity_and_source_domains() -> Result<(), RpcError> {
        let policy = ReceiptPolicy {
            protocol_version: 3,
            network_id: 17,
            sequencer: SequencerAuthorization::new([1; 32], [2; 32], 1, 10),
            trusted_checkpoint_context_digest: None,
        };
        let options = PaymentOptions {
            actor: "did:layerx:alice".into(),
            idempotency_key: [0x71; 32],
            fee_limit: 20,
            not_before: 10,
            not_after: 100,
            commitment: Commitment::Executed,
            wait_timeout: Duration::from_secs(1),
        };
        let payload =
            include_str!("../../layerx-crypto/tests/fixtures/payments/1-6-source-sequence-23.hex")
                .trim()
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    let text = std::str::from_utf8(pair).map_err(|_| RpcError::InvalidRequest)?;
                    u8::from_str_radix(text, 16).map_err(|_| RpcError::InvalidRequest)
                })
                .collect::<Result<Vec<_>, _>>()?;
        let prepared =
            prepare_payload(ModuleId::Asset, 6, &payload, &options, &policy, [9; 32], 7)?;
        assert_eq!(prepared.disclosure().envelope_sequence(), 7);
        assert_eq!(
            prepared
                .disclosure()
                .payload_sequence()
                .map_err(|_| RpcError::Verification)?,
            Some(23)
        );
        assert_eq!(
            prepared
                .disclosure()
                .reencode()
                .map_err(|_| RpcError::Verification)?,
            prepared.canonical_bytes()
        );
        let mint = Payment::Mint {
            asset: [3; 32],
            to: [2; 32],
            amount: 6,
        }
        .encode(options.actor.as_bytes())
        .map_err(|_| RpcError::InvalidRequest)?;
        let prepared_mint =
            prepare_payload(ModuleId::Asset, 10, &mint, &options, &policy, [9; 32], 8)?;
        assert_eq!(prepared_mint.disclosure().envelope_sequence(), 8);
        assert_eq!(
            prepared_mint
                .disclosure()
                .payload_sequence()
                .map_err(|_| RpcError::Verification)?,
            None
        );
        Ok(())
    }
}

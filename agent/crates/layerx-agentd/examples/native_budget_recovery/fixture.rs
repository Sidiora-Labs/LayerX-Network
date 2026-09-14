use super::{checked, Result};
use ed25519_dalek::{Signer as _, SigningKey};
use layerx_agentd::budget::{NativeBudgetBinding, NativeBudgetScope};
use layerx_agentd::prepare::{
    prepare_activity_for_protocol, PreparationDefaults, PrepareRequest,
    ProductionCorePreparationBoundary,
};
use layerx_agentd::protocol_evidence::EvidenceAuthority;
use layerx_agentd::sign::{attach_external_signature, verify_before_submit, VerifiedSubmission};
use layerx_client::client::{ClientConfig, ReconnectPolicy};
use layerx_client::evidence::{CheckpointSelector, ProofBundleSelector, SignedHeader};
use layerx_client::lni::{
    handshake::{self, HandshakeConfig},
    schema::{encode_envelope, Envelope, Version},
    transport::{ConnectionGate, FrameTransport, Limits, Uds},
};
use layerx_client::Client;
use layerx_programs::hex;
use layerx_types::clock::Deadline;
use layerx_types::{
    account::AccountId,
    activity::Authority,
    amount::Amount,
    ids::{Did, IdempotencyKey},
    payload::{ActivityType, ModuleRegistry},
};
use layerx_wire::{encode::Encoder, hash::account_id_for_protocol};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Fixture {
    pub client: Client,
    pub registry: ModuleRegistry,
    pub authority: EvidenceAuthority,
    pub directory: PathBuf,
    pub did: Did,
    pub public: [u8; 32],
    pub asset: [u8; 32],
    socket: PathBuf,
    key: SigningKey,
    request: u64,
    registered_batch: u64,
    fee_authority: super::fees::Authority,
    clock: std::sync::Arc<layerx_client::runtime_clock::RuntimeClock>,
}

fn configuration(socket: &Path) -> ClientConfig {
    ClientConfig {
        endpoint: socket.to_owned(),
        handshake: HandshakeConfig {
            built_interface_version: Version::V1_5,
            expected_protocol_version: 3,
            expected_network_id: 77,
        },
        limits: Limits {
            maximum_frame_bytes: 16_777_216,
            maximum_connections: 1,
            maximum_streams: 1,
            maximum_queued_bytes: 16_777_216,
            deadline: Duration::from_secs(8),
        },
        reconnect: ReconnectPolicy {
            maximum_attempts: 4,
            base_delay: Duration::from_millis(10),
            maximum_delay: Duration::from_millis(100),
            jitter_percent: 10,
        },
    }
}

pub fn result_code(bytes: &[u8]) -> Result<i32> {
    Ok(checked(layerx_wire::receipt::decode(bytes))?
        .protocol()
        .ok_or("native protocol receipt missing")?
        .result_code())
}

pub fn account(name: &str) -> Result<[u8; 32]> {
    checked(account_id_for_protocol(
        &checked(AccountId::parse(name))?,
        3,
    ))
}

impl Fixture {
    pub fn open() -> Result<Self> {
        let socket = PathBuf::from(std::env::var("LAYERX_TEST_NATIVE_BUDGET_SOCKET")?);
        let directory = PathBuf::from(std::env::var("LAYERX_TEST_NATIVE_BUDGET_WORK")?);
        let asset = checked(hex::decode(&std::env::var(
            "LAYERX_TEST_NATIVE_BUDGET_ASSET",
        )?))?
        .try_into()
        .map_err(|_| "fixture asset length")?;
        let key = SigningKey::from_bytes(&[0x11; 32]);
        let public = key.verifying_key().to_bytes();
        assert_eq!(
            std::env::var("LAYERX_TEST_NATIVE_BUDGET_ACTOR_PUBLIC_KEY")?,
            hex::encode(&public)
        );
        let did = checked(Did::new(
            format!("did:layerx:{}", hex::encode(&public)).as_bytes(),
        ))?;
        let mut client = checked(Client::connect(configuration(&socket)))?;
        let initial = checked(client.preparation_state(&did, 8000))?;
        let registry = initial.module_registry;
        let authority = checked(EvidenceAuthority::native_budget_authority(
            3,
            77,
            Path::new(&std::env::var("LAYERX_TEST_NATIVE_BUDGET_AUTHORITY")?),
        ))?;
        let fee_authority = super::fees::authority(
            Path::new(&std::env::var("LAYERX_TEST_NATIVE_BUDGET_AUTHORITY")?),
            client.handshake().node().authorised_sequencer_key,
            initial.kernel_epoch,
        )?;
        let clock = layerx_client::runtime_clock::RuntimeClock::from_environment()?;
        Ok(Self {
            client,
            registry,
            authority,
            directory,
            did,
            public,
            asset,
            socket,
            key,
            request: 0,
            registered_batch: 0,
            fee_authority,
            clock,
        })
    }

    pub fn replace_signer(&mut self, key: SigningKey) {
        self.public = key.verifying_key().to_bytes();
        self.key = key;
    }

    pub fn signed(&mut self, kind: u32, id: u8, payload: Vec<u8>) -> Result<VerifiedSubmission> {
        checked(self.client.reconnect())?;
        let funding = super::fees::funding(
            &mut self.client,
            &self.fee_authority,
            &self.did,
            self.public,
        )?;
        let mut boundary = checked(ProductionCorePreparationBoundary::new(
            &mut self.client,
            8001,
        ))?;
        let prepared = checked(prepare_activity_for_protocol(
            &mut boundary,
            PreparationDefaults {
                timestamp_span: 30_000,
                fee_limit: Amount::from_u128(funding.balance),
                maximum_payload_bytes: 4096,
            },
            PrepareRequest {
                actor: self.did.clone(),
                authority: checked(Authority::owner(&self.public))?,
                activity_type: checked(ActivityType::from_u32(kind))?,
                expected_account_sequence: None,
                timestamp_bound: None,
                fee_limit: None,
                idempotency_key: IdempotencyKey::new([id; 32]),
                payload,
                declared_payload_limit: 4096,
            },
            3,
        ))?;
        let state = boundary
            .last_state()
            .ok_or("preparation snapshot missing")?;
        assert_eq!(state.observed_head_sequence, funding.sequence);
        self.registry = state.module_registry.clone();
        if kind == 0x0003_0006 {
            let Some(layerx_crypto::disclosure::DisclosedNativeOperation::BudgetSpend(spend)) =
                &prepared.disclosure.native_operation
            else {
                return Err("typed Budget Spend disclosure missing".into());
            };
            assert_ne!(spend.budget_id, [0; 32]);
            assert_ne!(spend.budget_account, [0; 32]);
            assert!(spend.amount > 0);
            let mut changed = prepared.clone();
            changed.disclosure.amounts[0].value = spend
                .amount
                .checked_add(1)
                .ok_or("disclosure amount overflow")?;
            assert!(layerx_agentd::prepare::verify_disclosure_binding(&changed).is_err());
        }
        let signature = self.key.sign(&prepared.signing_preimage).to_bytes();
        let bytes = checked(attach_external_signature(&prepared, signature))?;
        checked(verify_before_submit(
            &bytes,
            &prepared,
            &self.public,
            &self.registry,
        ))
    }

    pub fn create(&mut self, id: u8) -> Result<NativeBudgetScope> {
        self.create_with_period(id, 3_600_000)
    }

    pub fn create_with_period(&mut self, id: u8, period_length: u64) -> Result<NativeBudgetScope> {
        self.create_with_lifetime(id, period_length, 3_600_000)
    }

    pub fn create_with_lifetime(
        &mut self,
        id: u8,
        period_length: u64,
        lifetime: u64,
    ) -> Result<NativeBudgetScope> {
        checked(self.client.reconnect())?;
        let start = checked(self.client.preparation_state(&self.did, 8002))?.protocol_timestamp;
        let did = std::str::from_utf8(self.did.as_bytes())?;
        let owner = account(&format!("agent:{did}:main"))?;
        let budget = account(&format!("agent:{did}:budget:{}", hex::encode(&[id; 32])))?;
        let end = start.checked_add(lifetime).ok_or("period overflow")?;
        let mut payload = Encoder::new(211);
        checked(payload.u16(1))?;
        for value in [[id; 32], budget, self.asset, [0x73; 32]] {
            checked(payload.fixed(&value))?;
        }
        for value in [100_u128, 0, 100] {
            checked(payload.u128(value))?;
        }
        for value in [period_length, start, end, 1] {
            checked(payload.u64(value))?;
        }
        checked(payload.u8(1))?;
        let signed = self.signed(0x0003_0001, id, payload.finish())?;
        let receipt = self.submit(signed.exact_bytes())?;
        assert_eq!(super::fixture::result_code(&receipt.0)?, 0);
        self.finalize(&receipt.1)?;
        Ok(NativeBudgetScope {
            network_id: 77,
            write_enabled: true,
            maximum: 100,
            maximum_lifetime_ms: 3_600_000,
            binding: NativeBudgetBinding {
                budget_id: [id; 32],
                owner_account: owner,
                budget_account: budget,
                asset: self.asset,
                owner_did: self.did.clone(),
                owner_public_key: self.public,
                period_start_ms: start,
                period_length_ms: period_length,
                expiry_ms: end,
            },
        })
    }

    pub fn owner_bytes(&mut self, kind: u32, id: u8, bytes: &[u8]) -> Result<Vec<u8>> {
        use layerx_types::activity::{EnvelopeBuilder, Signature, TimestampBound};
        use layerx_types::payload::Payload;
        checked(self.client.reconnect())?;
        let funding = super::fees::funding(
            &mut self.client,
            &self.fee_authority,
            &self.did,
            self.public,
        )?;
        let state = checked(self.client.preparation_state(&self.did, 8200))?;
        assert_eq!(state.observed_head_sequence, funding.sequence);
        assert_eq!(state.observed_state_root, funding.root);
        let kind = checked(ActivityType::from_u32(kind))?;
        let payload = checked(Payload::new(&state.module_registry, kind, bytes))?;
        let payload_hash = checked(layerx_wire::hash::payload_hash_for(&payload))?;
        let mut builder = EnvelopeBuilder::new();
        checked(
            builder
                .protocol_version(3)
                .and_then(|value| value.network_id(state.network_id))
                .and_then(|value| value.activity_type(kind))
                .and_then(|value| value.actor_did(self.did.clone()))
                .and_then(|value| value.authority(Authority::owner(&self.public)?))
                .and_then(|value| value.account_sequence(state.account_sequence))
                .and_then(|value| {
                    value.timestamp_bound(TimestampBound::new(
                        state.protocol_timestamp,
                        state.protocol_timestamp.checked_add(30_000).ok_or(
                            layerx_types::activity::ActivityBuildError::InvalidTimestampBound,
                        )?,
                    )?)
                })
                .and_then(|value| value.idempotency_key(IdempotencyKey::new([id; 32])))
                .and_then(|value| value.fee_limit(Amount::from_u128(funding.balance)))
                .and_then(|value| value.payload_hash(payload_hash))
                .and_then(|value| value.payload(payload)),
        )?;
        let unsigned = checked(builder.build())?;
        let preimage = checked(layerx_wire::sign::preimage_unsigned(&unsigned))?;
        let signature = self.key.sign(preimage.as_bytes()).to_bytes();
        let envelope = unsigned.attach_signature(checked(Signature::new(&signature))?);
        let exact = checked(layerx_wire::activity::encode_signed_envelope(&envelope))?;
        self.registry = state.module_registry;
        Ok(exact)
    }

    pub fn close(&mut self, scope: &NativeBudgetScope, id: u8) -> Result<()> {
        let mut encoded = Encoder::new(42);
        checked(encoded.u16(1))?;
        checked(encoded.fixed(&scope.binding.budget_id))?;
        checked(encoded.u64(2))?;
        let exact = self.owner_bytes(0x0003_0007, id, &encoded.finish())?;
        let result = self.submit(&exact)?;
        assert_eq!(result_code(&result.0)?, 0);
        self.finalize(&result.1)
    }

    pub fn spend(
        &mut self,
        scope: &NativeBudgetScope,
        id: u8,
        amount: u128,
        recipient: [u8; 32],
    ) -> Result<VerifiedSubmission> {
        let mut payload = Encoder::new(82);
        checked(payload.u16(1))?;
        checked(payload.fixed(&scope.binding.budget_id))?;
        checked(payload.fixed(&recipient))?;
        checked(payload.u128(amount))?;
        self.signed(0x0003_0006, id, payload.finish())
    }

    pub fn submit(&mut self, bytes: &[u8]) -> Result<(Vec<u8>, SignedHeader)> {
        let activity = checked(layerx_wire::activity::decode_signed(bytes, &self.registry))?;
        let id = checked(layerx_wire::hash::activity_id(&activity))?;
        let submitted =
            checked(
                self.client
                    .submit_signed(&self.registry, self.public, 8003, 1, bytes),
            )?;
        let layerx_client::submit::Submission::Acknowledged(ack) = submitted else {
            return Err("native admission was unknown".into());
        };
        assert_eq!(ack.activity_id(), id);
        self.receipt(id)
    }

    pub fn receipt(&mut self, id: [u8; 32]) -> Result<(Vec<u8>, SignedHeader)> {
        let mut deadline = Deadline::start(self.clock.as_ref(), Duration::from_secs(30))?;
        loop {
            checked(self.client.reconnect())?;
            match self
                .client
                .proof_bundle(ProofBundleSelector::Receipt(id), 8004, &self.registry)
            {
                Ok(value) => {
                    return Ok((
                        value.canonical_bytes().to_vec(),
                        value.signed_header().clone(),
                    ))
                }
                Err(error) => {
                    if deadline.remaining(self.clock.as_ref())?.is_zero() {
                        return Err(format!("native receipt deadline: {error:?}").into());
                    }
                    self.clock.wait(Duration::from_millis(25))?;
                }
            }
        }
    }

    pub fn drop_response(&self, bytes: &[u8]) -> Result<()> {
        let config = configuration(&self.socket);
        let mut transport = checked(Uds::connect(
            &self.socket,
            &ConnectionGate::new(1),
            config.limits,
        ))?;
        let handshake = checked(handshake::perform(&mut transport, &config.handshake, None))?;
        let request = checked(encode_envelope(Envelope {
            version: handshake.node().interface_version,
            message_tag: 3,
            correlation_id: 8100,
            canonical_payload: bytes,
            proof_material: &[],
        }))?;
        checked(transport.send(&request))?;
        drop(transport);
        Ok(())
    }

    pub fn finalize(&mut self, header: &SignedHeader) -> Result<()> {
        self.request = self.request.checked_add(1).ok_or("request overflow")?;
        if self.request > 16 {
            return Err("publication request count exceeded".into());
        }
        let batch = checked(header.batch_number())?;
        let request = self
            .directory
            .join("requests")
            .join(format!("{}.json", self.request));
        let pending = request.with_extension("pending");
        std::fs::write(
            &pending,
            serde_json::to_vec(
                &serde_json::json!({"version":1,"operation":"finalize","batch":batch}),
            )?,
        )?;
        std::fs::rename(pending, request)?;
        let response = self
            .directory
            .join("responses")
            .join(format!("{}.json", self.request));
        let mut deadline = Deadline::start(self.clock.as_ref(), Duration::from_secs(90))?;
        while !response.exists() {
            if deadline.remaining(self.clock.as_ref())?.is_zero() {
                return Err("actual checkpoint publication deadline".into());
            }
            self.clock.wait(Duration::from_millis(25))?;
        }
        if std::fs::metadata(&response)?.len() > 1_048_576 {
            return Err("publication response exceeded bound".into());
        }
        let response_bytes = std::fs::read(response)
            .map_err(|error| format!("checkpoint response read: {error}"))?;
        let response: serde_json::Value = serde_json::from_slice(&response_bytes)?;
        if response.get("error").is_some() {
            return Err(format!("publication refused: {response}").into());
        }
        assert_eq!(response["version"], 1);
        assert_eq!(response["batch"], batch);
        checked(self.client.reconnect())?;
        super::finality::register(
            &mut self.client,
            &self.directory,
            &response,
            &mut self.registered_batch,
            batch,
        )?;
        checked(self.client.reconnect())?;
        let checkpoint = checked(
            self.client
                .checkpoint_evidence(CheckpointSelector::Batch(batch), 8005),
        )?;
        assert_eq!(checkpoint.canonical_header(), header.canonical_bytes);
        assert_eq!(
            response["checkpoint_id"]
                .as_str()
                .ok_or("publication checkpoint missing")?,
            hex::encode(
                &checkpoint
                    .report()
                    .evidence()
                    .checkpoint_id()
                    .ok_or("verified checkpoint missing")?
            )
        );
        Ok(())
    }
}

use super::*;
use crate::custody::{Operation, SignAuthorization, SignRequest};
use crate::store::{PrincipalId, TenancyMap};
use layerx_intents::owner_activity::OwnerEnvelopeContext;
use layerx_types::payload::{ActivityType, ModuleRegistration, ModuleRegistry};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Prepare {
    email: String,
    display_name: String,
    idempotency_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryDocument {
    network_id: u32,
    protocol_version: u16,
    modules: Vec<RegistryModule>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryModule { module_id: u16, activity_types: Vec<u32> }

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    principal: String,
    did: String,
    public_key: [u8; 32],
    pending_key: [u8; 32],
    recovery_root: [u8; 32],
    recovery_threshold: u16,
    recovery_delay_seconds: u64,
    registration_action: [u8; 32],
    recovery_action: [u8; 32],
    recipient: [u8; 20],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipientRequest {
    principal: String,
    checkpoint: [u8; 32],
    asset: [u8; 32],
    recipient: [u8; 20],
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SigningRequest {
    principal: String,
    operation: String,
    sequence: u64,
    not_before_ms: u64,
    not_after_ms: u64,
    action_key: [u8; 32],
    fee_limit: u128,
    credit: Option<Vec<u8>>,
    effective_sequence: Option<u64>,
    policy_start_ms: Option<u64>,
    policy_end_ms: Option<u64>,
}

struct Runtime {
    store: Arc<Mutex<PrincipalStore>>,
    custody: CustodySigner,
    registry: ModuleRegistry,
    network: u32,
}

fn refused<E>(_: E) -> String { "onboarding sponsor authority or durable state refused".to_owned() }

/// # Errors
/// Refuses invalid input, unavailable production authorities, and changed provisioning retries.
pub fn onboarding_sponsor_command(operation: &str, input: &[u8]) -> Result<Vec<u8>, String> {
    if input.len() > 1_048_576 { return Err("sponsor request exceeds bound".to_owned()); }
    if operation == "initialize-store" {
        if !input.is_empty() { return Err("store initialization has no input body".to_owned()); }
        let root = absolute("LAYERX_HUMAN_STORE_ROOT")?;
        if root.exists() { return Err("store initialization requires an absent destination".to_owned()); }
        let map = TenancyMap::new([]).map_err(refused)?;
        let digest = map.install(&root).map_err(refused)?;
        return serde_json::to_vec(&json!({"tenancy_digest": URL_SAFE_NO_PAD.encode(digest.bytes())})).map_err(refused);
    }
    let runtime = Runtime::open()?;
    let value = match operation {
        "prepare" => runtime.prepare(serde_json::from_slice(input).map_err(refused)?)?,
        "sign" => runtime.sign(serde_json::from_slice(input).map_err(refused)?)?,
        "settlement-recipient" => runtime.recipient(serde_json::from_slice(input).map_err(refused)?)?,
        _ => return Err("unknown sponsor operation".to_owned()),
    };
    serde_json::to_vec(&value).map_err(refused)
}

impl Runtime {
    fn open() -> Result<Self, String> {
        let network = number("LAYERX_HUMAN_NETWORK_ID")?;
        let document: RegistryDocument = serde_json::from_slice(&read_nonempty(&absolute(
            "LAYERX_HUMAN_ONBOARDING_REGISTRY_FILE")?)?).map_err(refused)?;
        if document.network_id != network || document.protocol_version != 3 { return Err(refused(())); }
        let modules = document.modules.into_iter().map(|module| {
            let id = ModuleId::from_u16(module.module_id).map_err(refused)?;
            let kinds = module.activity_types.into_iter().map(|kind|
                ActivityType::from_u32(kind).map_err(refused)).collect::<Result<Vec<_>, _>>()?;
            ModuleRegistration::new(id, &kinds).map_err(refused)
        }).collect::<Result<Vec<_>, String>>()?;
        let registry = ModuleRegistry::new(&modules).map_err(refused)?;
        let store = production_principal_store(absolute("LAYERX_HUMAN_STORE_ROOT")?,
            production_retention_config()?, secret32("LAYERX_HUMAN_TENANCY_DIGEST")?,
            principal_binding_configuration()?)?;
        let tls = mutual_tls(&absolute("LAYERX_HUMAN_KMS_ROOT_CERTIFICATE_DER")?,
            &absolute("LAYERX_HUMAN_KMS_CLIENT_CERTIFICATE_DER")?,
            &absolute("LAYERX_HUMAN_KMS_CLIENT_PRIVATE_KEY_DER")?)?;
        let limits = Limits {
            maximum_frame_bytes: number("LAYERX_HUMAN_KMS_MAX_FRAME_BYTES")?,
            maximum_connections: number("LAYERX_HUMAN_KMS_MAX_CONNECTIONS")?,
            maximum_streams: number("LAYERX_HUMAN_KMS_MAX_STREAMS")?,
            maximum_queued_bytes: number("LAYERX_HUMAN_KMS_MAX_QUEUED_BYTES")?,
            deadline: Duration::from_secs(number("LAYERX_HUMAN_KMS_DEADLINE_SECONDS")?),
        };
        let provider = RemoteKmsProvider::new(required("LAYERX_HUMAN_KMS_PROVIDER_REFERENCE")?,
            required("LAYERX_HUMAN_KMS_ENDPOINT")?.parse().map_err(refused)?,
            required("LAYERX_HUMAN_KMS_SERVER_NAME")?, tls, limits).map_err(refused)?;
        let keystore = Keystore::open_production(absolute("LAYERX_HUMAN_CUSTODY_ROOT")?, network, provider).map_err(refused)?;
        let custody = CustodySigner::new_shared(keystore, Arc::clone(&store), registry.clone(),
            SigningLimits::new(number("LAYERX_HUMAN_SIGNING_RATE_MAXIMUM")?,
                number("LAYERX_HUMAN_SIGNING_RATE_WINDOW_SECONDS")?).map_err(refused)?);
        Ok(Self { store, custody, registry, network })
    }

    fn prepare(&self, request: Prepare) -> Result<serde_json::Value, String> {
        let identity = RemoteIdentityProvider::new(IdentityProviderConfig {
            socket: absolute("LAYERX_HUMAN_IDENTITY_SOCKET")?,
            deadline: Duration::from_secs(number("LAYERX_HUMAN_IDENTITY_DEADLINE_SECONDS")?),
            maximum_frame_bytes: number("LAYERX_HUMAN_IDENTITY_MAX_FRAME_BYTES")?,
            peer_uid: number("LAYERX_HUMAN_IDENTITY_PEER_UID")?, peer_gid: number("LAYERX_HUMAN_IDENTITY_PEER_GID")?,
        }).map_err(refused)?;
        let observed = now().map_err(refused)?;
        let provisioned = identity.provision(&request.email, &request.display_name,
            &request.idempotency_key, observed).map_err(refused)?;
        let index = production_auth_index(absolute("LAYERX_HUMAN_AUTH_INDEX_ROOT")?,
            secret32("LAYERX_HUMAN_AUTH_INDEX_KEY")?)?;
        index.bind_account(&request.email, &provisioned.principal).map_err(refused)?;
        let mut store = self.store.lock().map_err(refused)?;
        let mut scope = store.principal(&provisioned.principal).map_err(refused)?;
        let mut journey = OnboardingJourney::start(&mut scope, &provisioned.onboarding, observed).map_err(refused)?;
        self.custody.resume_onboarding_local(&mut journey, &mut scope, observed).map_err(refused)?;
        identity_dispatch::update_profile(&mut scope, &json!({"display_name": request.display_name}), observed).map_err(refused)?;
        let (did, public_key, recovery) = journey.bootstrap_identity().map_err(refused)?;
        let pending_id = KeyId::new("human-owner-pending").map_err(refused)?;
        let keystore = self.custody.creation_keystore();
        match keystore.create(scope.principal(), &pending_id, KeyClass::HumanPrimary) {
            Ok(_) | Err(CustodyError::KeyExists) => (),
            Err(error) => return Err(refused(error)),
        }
        let pending = keystore.describe(scope.principal(), &pending_id).map_err(refused)?;
        if pending.class != KeyClass::HumanPrimary || pending.public_key == public_key { return Err(refused(())); }
        let owner = Owner { principal: scope.principal().as_str().to_owned(),
            did: std::str::from_utf8(did.as_bytes()).map_err(refused)?.to_owned(), public_key,
            pending_key: pending.public_key, recovery_root: recovery.root().bytes(),
            recovery_threshold: recovery.threshold().value(), recovery_delay_seconds: recovery.challenge_delay_secs(),
            registration_action: journey.bootstrap_action(crate::onboarding::ProtocolStage::DidRegistration),
            recovery_action: journey.bootstrap_action(crate::onboarding::ProtocolStage::RecoveryRegistration),
            recipient: self.custody.evm_wallet(scope.principal(), &KeyId::new("human-primary").map_err(refused)?).map_err(refused)? };
        let row = RowKey::new("onboarding-sponsor-owner").map_err(refused)?;
        let bytes = serde_json::to_vec(&owner).map_err(refused)?;
        if let Some(prior) = scope.get(Table::Journeys, &row) {
            if prior.bytes() != bytes { return Err(refused(())); }
        } else { scope.put(Table::Journeys, row, observed, bytes).map_err(refused)?; }
        serde_json::to_value(owner).map_err(refused)
    }

    fn recipient(&self, request: RecipientRequest) -> Result<serde_json::Value, String> {
        let principal = PrincipalId::new(&request.principal).map_err(refused)?;
        let mut store = self.store.lock().map_err(refused)?;
        let mut scope = store.principal(&principal).map_err(refused)?;
        let key = KeyId::new("human-primary").map_err(refused)?;
        let trace = TraceId::mint(request.checkpoint[..16].try_into().map_err(refused)?);
        let signature = self.custody.settlement_recipient_in_scope(&mut scope, &key,
            crate::custody::SettlementRecipientRequest { checkpoint: request.checkpoint,
                asset: request.asset, recipient: request.recipient }, &trace, now().map_err(refused)?).map_err(refused)?;
        let journey = OnboardingJourney::load(&scope).map_err(refused)?.ok_or_else(|| refused(()))?;
        let did = journey.did().map_err(refused)?;
        let did_text = std::str::from_utf8(did.as_bytes()).map_err(refused)?;
        let account = AccountId::parse(&format!("agent:{did_text}:main")).map_err(refused)?;
        let public_key = self.custody.creation_keystore().describe(&principal, &key).map_err(refused)?.public_key;
        Ok(json!({"network_id": self.network, "principal": principal.as_str(), "did": did_text,
            "account": hex_bytes(&layerx_intents::canonical::account_id_for_protocol(&account, 3).map_err(refused)?),
            "public_key": hex_bytes(&public_key), "asset": hex_bytes(&request.asset),
            "checkpoint": hex_bytes(&request.checkpoint), "recipient": hex_bytes(&request.recipient),
            "signature": hex_bytes(&signature)}))
    }

    fn sign(&self, request: SigningRequest) -> Result<serde_json::Value, String> {
        let principal = PrincipalId::new(&request.principal).map_err(refused)?;
        let mut store = self.store.lock().map_err(refused)?;
        let mut scope = store.principal(&principal).map_err(refused)?;
        let owner = scope.get(Table::Journeys, &RowKey::new("onboarding-sponsor-owner").map_err(refused)?)
            .ok_or_else(|| refused(()))?;
        let owner: Owner = serde_json::from_slice(owner.bytes()).map_err(refused)?;
        if owner.principal != request.principal { return Err(refused(())); }
        let did = Did::new(owner.did.as_bytes()).map_err(refused)?;
        let compiled = bootstrap_intent(&owner, &request, &self.registry)?;
        let context = OwnerEnvelopeContext { actor: did, owner_public_key: owner.public_key,
            network_id: self.network, account_sequence: request.sequence,
            not_before_ms: request.not_before_ms, not_after_ms: request.not_after_ms,
            action_key: request.action_key, fee_limit: request.fee_limit };
        let (unsigned, disclosure) = layerx_intents::owner_activity::unsigned_native(&compiled, &context, &self.registry).map_err(refused)?;
        let row = RowKey::new(format!("onboarding-sponsor-{}", request.operation)).map_err(refused)?;
        if let Some(prior) = scope.get(Table::Journeys, &row) {
            let signed = layerx_intents::owner_activity::verify(prior.bytes(), &self.registry).map_err(refused)?;
            if layerx_intents::canonical::unsigned_activity_bytes(&signed).map_err(refused)? != unsigned { return Err(refused(())); }
            return Ok(json!({"activity": hex_bytes(prior.bytes()), "activity_id": hex_bytes(&layerx_intents::canonical::activity_id(&signed).map_err(refused)?)}));
        }
        let key = KeyId::new("human-primary").map_err(refused)?;
        let trace = TraceId::mint(request.action_key[..16].try_into().map_err(refused)?);
        let signature = super::super::poll_once_ready(self.custody.sign_in_scope(&mut scope,
            SignRequest::new(&principal, &key, &trace, SignAuthorization::new(Operation::ProtocolMutation, None),
                &unsigned, &disclosure, now().map_err(refused)?))).map_err(refused)?.map_err(refused)?;
        let signed = layerx_intents::owner_activity::attach_signature(&unsigned, *signature.signature(),
            signature.signer_public_key(), &self.registry).map_err(refused)?;
        let activity = layerx_intents::owner_activity::verify(&signed, &self.registry).map_err(refused)?;
        scope.put(Table::Journeys, row, request.not_before_ms / 1_000, signed.clone()).map_err(refused)?;
        Ok(json!({"activity":hex_bytes(&signed), "activity_id":hex_bytes(&layerx_intents::canonical::activity_id(&activity).map_err(refused)?)}))
    }
}

fn bootstrap_intent(owner: &Owner, request: &SigningRequest, registry: &ModuleRegistry) -> Result<layerx_intents::CompiledIntent, String> {
    use layerx_intents::NativeOwnerBootstrap;
    use layerx_types::intent::{ApprovalThreshold, PublicKey, RecoveryRoot};
    let did = Did::new(owner.did.as_bytes()).map_err(refused)?;
    if request.operation == "identity" && request.action_key != owner.registration_action
        || request.operation == "recovery" && request.action_key != owner.recovery_action { return Err(refused(())); }
    if request.operation == "credit" {
        if request.effective_sequence.is_some() || request.policy_start_ms.is_some()
            || request.policy_end_ms.is_some() { return Err(refused(())); }
        let raw: &[u8; 427] = request.credit.as_deref().ok_or_else(|| refused(()))?.try_into().map_err(refused)?;
        if raw[139..171] != owner.public_key { return Err(refused(())); }
        let credit = layerx_intents::NativeCustodyCredit::new(raw,
            AccountId::parse("system:paxeer-reserve").map_err(refused)?,
            AccountId::parse(&format!("agent:{}:main", owner.did)).map_err(refused)?).map_err(refused)?;
        if credit.nullifier() != request.action_key { return Err(refused(())); }
        let intent = Intent::v1(IntentKind::NativeCustodyCredit(credit));
        let compiled = layerx_intents::compile(&intent, registry).map_err(refused)?;
        layerx_intents::DisclosureCheck::verify(&intent, &compiled).map_err(refused)?;
        return Ok(compiled);
    }
    if request.credit.is_some() { return Err(refused(())); }
    let operation = match request.operation.as_str() {
        "identity" => NativeOwnerBootstrap::Identity { did, primary_key: PublicKey::new(owner.public_key) },
        "rotation" => NativeOwnerBootstrap::RotationPolicy { did, pending_key: PublicKey::new(owner.pending_key),
            challenge: layerx_types::activity::TimestampBound::new(
                request.policy_start_ms.ok_or_else(|| refused(()))?,
                request.policy_end_ms.ok_or_else(|| refused(()))?).map_err(refused)?,
            effective_sequence: request.effective_sequence.ok_or_else(|| refused(()))? },
        "recovery" => NativeOwnerBootstrap::RecoveryPolicy { did, root: RecoveryRoot::new(owner.recovery_root),
            threshold: ApprovalThreshold::new(owner.recovery_threshold).map_err(refused)?,
            minimum_delay: owner.recovery_delay_seconds, maximum_delay: owner.recovery_delay_seconds },
        _ => return Err(refused(())),
    };
    if request.operation != "rotation" && (request.effective_sequence.is_some() || request.policy_start_ms.is_some() || request.policy_end_ms.is_some()) { return Err(refused(())); }
    operation.compile(registry).map_err(refused)
}

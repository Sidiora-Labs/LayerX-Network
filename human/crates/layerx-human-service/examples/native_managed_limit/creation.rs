use super::{checked, fixture::Fixture, Result};
use ed25519_dalek::SigningKey;
use layerx_agentd::session_keys::SessionKeyRegistry;
use layerx_client::evidence::SignedHeader;
use layerx_crypto::session::{
    issue_session_key, IssuedSessionKey, SessionKeyRequest, SessionPurpose,
};
use layerx_intents::{Intent, IntentKind, NativeBudgetCreate, SessionGrant};
use layerx_types::{account::AccountId, payload::ActivityType, verify::VerificationLevel};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use std::os::unix::fs::PermissionsExt as _;

pub const SESSION_ACTION: [u8; 32] = [0xbc; 32];
pub struct Receipt {
    pub signed: Vec<u8>,
    pub kind: u32,
    pub bytes: Vec<u8>,
    pub header: SignedHeader,
}
impl Receipt {
    pub fn reference(&self) -> Result<Value> {
        let decoded = checked(layerx_wire::receipt::decode(&self.bytes))?;
        let receipt = decoded.protocol().ok_or("native receipt missing")?;
        let kind = checked(ActivityType::from_u32(self.kind))?;
        let module = checked(layerx_types::payload::ModuleRegistration::new(
            kind.module(),
            &[kind],
        ))?;
        let registry = checked(layerx_types::payload::ModuleRegistry::new(&[module]))?;
        let activity = checked(layerx_wire::activity::decode_signed(
            &self.signed,
            &registry,
        ))?;
        assert_eq!(
            checked(layerx_wire::hash::activity_id(&activity))?,
            receipt.activity_id()
        );

        Ok(
            json!({"activity_id":layerx_programs::hex::encode(&receipt.activity_id()),
            "receipt_digest":layerx_programs::hex::encode(&checked(layerx_wire::hash::receipt_digest(&checked(layerx_wire::receipt::encode_unsigned(&decoded))?))?)}),
        )
    }
    pub fn identity(&self) -> Result<Vec<u8>> {
        let decoded = checked(layerx_wire::receipt::decode(&self.bytes))?;
        let effects: Vec<_> = decoded
            .protocol()
            .ok_or("native receipt missing")?
            .effects()
            .iter()
            .filter(|effect| effect.module_id() == 7 && effect.event_type() == 0x7110)
            .collect();
        if effects.len() != 1 {
            return Err("committed identity effect missing".into());
        }
        let bytes = effects[0].body().to_vec();
        if bytes.len() != 223 || !bytes.starts_with(b"LXGI1") {
            return Err("committed native identity shape".into());
        }
        Ok(bytes)
    }
}

pub fn submit(fixture: &mut Fixture, kind: u32, id: u8, payload: Vec<u8>) -> Result<Receipt> {
    let signed = fixture.signed(kind, id, payload)?.exact_bytes().to_vec();
    let (bytes, header) = fixture.submit(&signed)?;
    assert_eq!(super::fixture::result_code(&bytes)?, 0);
    fixture.finalize(&header)?;
    Ok(Receipt {
        signed,
        kind,
        bytes,
        header,
    })
}

pub struct Created {
    pub provider: super::identity::Identity,
    pub budget: NativeBudgetCreate,
    pub granted: IssuedSessionKey,
    pub session_seed: [u8; 32],
    pub session_keys: Option<SessionKeyRegistry>,
    pub operator_secret: zeroize::Zeroizing<Vec<u8>>,
    pub session_expiry_sequence: u64,
    pub recovery_root: [u8; 32],
    pub sponsor: Receipt,
    pub funding: Receipt,
    pub identity: Receipt,
    pub rotation: Receipt,
    pub recovery: Receipt,
    pub creation: Receipt,
    pub grant: Receipt,
}

pub fn authorization(header: &SignedHeader) -> layerx_proof::inclusion::SequencerAuthorization {
    layerx_proof::inclusion::SequencerAuthorization::new(
        header.sequencer_id,
        header.public_key,
        header.first_batch_number,
        header.last_batch_number,
    )
}
fn number(bytes: &[u8], offset: usize) -> Result<u64> {
    Ok(u64::from_be_bytes(
        bytes
            .get(offset..offset + 8)
            .ok_or("identity field missing")?
            .try_into()?,
    ))
}
fn policy(receipt: &Receipt, recovery: bool) -> Result<Value> {
    let bytes = receipt.identity()?;
    let (revision, low, high) = if recovery {
        (175, 183, 191)
    } else {
        (167, 199, 207)
    };
    let mut minimum = number(&bytes, low)?;
    let mut maximum = number(&bytes, high)?;
    if !recovery {
        minimum = minimum.checked_add(999).ok_or("rotation time overflow")? / 1000;
        maximum /= 1000;
    }
    assert!(minimum > 0 && maximum >= minimum);
    let decoded = checked(layerx_wire::receipt::decode(&receipt.bytes))?;
    Ok(
        json!({"policy_revision":number(&bytes,revision)?,"required_delay_seconds":minimum,
        "maximum_delay_seconds":maximum,"effective_sequence":decoded.protocol().ok_or("native receipt")?.global_sequence(),
        "evidence":receipt.reference()?}),
    )
}
impl Created {
    pub fn references(&self) -> Result<Vec<Value>> {
        [
            &self.sponsor,
            &self.identity,
            &self.funding,
            &self.rotation,
            &self.recovery,
            &self.creation,
            &self.grant,
        ]
        .into_iter()
        .map(Receipt::reference)
        .collect()
    }

    pub fn policy(&self, fixture: &Fixture) -> Result<Value> {
        let identity = self.recovery.identity()?;
        let public = layerx_programs::hex::encode(&fixture.public);
        let grant = layerx_programs::hex::encode(&self.granted.grant_id);
        Ok(
            json!({"principals":[{"tenant":"native-managed-limit","principal":"owner",
            "account_id":layerx_programs::hex::encode(&checked(layerx_wire::hash::account_id_for_protocol(&self.budget.source_account,3))?),
            "asset_id":layerx_programs::hex::encode(&fixture.asset),"activities":[],"budgets":[],
            "maximum_age_seconds":3600,"maximum_age_sequences":10000,
            "identities":[{"did":std::str::from_utf8(fixture.did.as_bytes())?,
                "authorities":[{"kind":"primary_key","id":public}],
                "revocation_sequence":number(&identity,69)?,"frozen":false,
                "evidence":self.recovery.reference()?,"rotation":policy(&self.rotation,false)?,
                "recovery":policy(&self.recovery,true)?,
                "capabilities":[{"authority":public,"action_key":layerx_programs::hex::encode(&SESSION_ACTION),
                    "capability_id":grant,"activity_types":[6],"counterparties":[],"assets":[],
                    "amount_ceiling":"0","expiry_sequence":self.session_expiry_sequence,
                    "enforceable_dimensions":[],"evidence":self.grant.reference()?}]}]}]}),
        )
    }
}

struct Governance {
    sponsor: Receipt,
    owner: super::onboarding::Bound,
    rotation: Receipt,
    recovery: Receipt,
    root: [u8; 32],
}
fn governance(fixture: &mut Fixture, provider: &super::identity::Identity) -> Result<Governance> {
    let did = checked(layerx_wire::hash::did_id_for_protocol(&fixture.did, 3))?;
    let mut identity = vec![0x71, 1, 0, 2];
    identity.extend(did);
    identity.extend(fixture.public);
    let sponsor = submit(fixture, 0x0007_0001, 0xb8, identity)?;
    let owner = super::onboarding::bind(fixture, provider)?;
    let did = checked(layerx_wire::hash::did_id_for_protocol(&fixture.did, 3))?;
    checked(fixture.client.reconnect())?;
    let state = checked(fixture.client.preparation_state(&fixture.did, 8600))?;
    let rotation = layerx_crypto::rotation::OwnerRotation::Announce {
        owner: fixture.did.clone(),
        pending_public_key: SigningKey::from_bytes(&[0x44; 32])
            .verifying_key()
            .to_bytes(),
        begin: state
            .protocol_timestamp
            .checked_add(3_600_000)
            .ok_or("rotation bound")?,
        end: state
            .protocol_timestamp
            .checked_add(7_200_000)
            .ok_or("rotation bound")?,
        effective_sequence: state
            .observed_head_sequence
            .checked_add(3)
            .ok_or("rotation sequence")?,
    };
    let rotation = submit(fixture, 0x0007_0002, 0xb9, checked(rotation.payload())?)?;
    let root = provider.recovery_root;
    let mut recovery = vec![0x71, 3, 0, 5];
    recovery.extend(did);
    recovery.extend(root);
    recovery.extend(1_u16.to_be_bytes());
    recovery.extend(60_u64.to_be_bytes());
    recovery.extend(60_u64.to_be_bytes());
    let recovery = submit(fixture, 0x0007_0003, 0xba, recovery)?;
    Ok(Governance {
        sponsor,
        owner,
        rotation,
        recovery,
        root,
    })
}

fn budget(fixture: &mut Fixture, recovery: &Receipt) -> Result<NativeBudgetCreate> {
    checked(fixture.client.reconnect())?;
    let state = checked(fixture.client.preparation_state(&fixture.did, 8601))?;
    let started = state.protocol_timestamp / 1000 * 1000;
    let owner = std::str::from_utf8(fixture.did.as_bytes())?;
    let source_account = checked(AccountId::parse(&format!("agent:{owner}:main")))?;
    let account = checked(layerx_wire::hash::account_id_for_protocol(
        &source_account,
        3,
    ))?;
    let proof = checked(fixture.client.account(
        account,
        VerificationLevel::CHECKPOINT_FINALISED,
        8602,
        authorization(&recovery.header),
    ))?;
    let source = checked(layerx_proof::state::decode_account_value(
        account,
        proof.canonical_bytes(),
    ))?;
    assert_eq!(source.authority_key, Some(fixture.public));
    Ok(NativeBudgetCreate {
        budget_id: [0xbb; 32],
        budget_account: checked(AccountId::parse(&format!(
            "agent:{owner}:budget:{}",
            layerx_programs::hex::encode(&[0xbb; 32])
        )))?,
        asset: fixture.asset,
        purpose: Sha256::digest(b"native-managed-limit").into(),
        per_period_limit: 100,
        carry_cap: 0,
        initial_amount: 100,
        period_length_ms: 3_600_000,
        period_start_ms: started,
        expiry_ms: started.checked_add(3_600_000).ok_or("Budget expiry")?,
        revocation_sequence: number(&recovery.identity()?, 69)?,
        rollover: 1,
        source_account,
        source_sequence: source.next_sequence,
    })
}

pub fn create(fixture: &mut Fixture) -> Result<Created> {
    let provider = super::identity::start(fixture)?;
    let Governance {
        sponsor,
        owner,
        rotation,
        recovery,
        root: recovery_root,
    } = governance(fixture, &provider)?;
    let budget = budget(fixture, &recovery)?;
    let creation = submit(fixture, 0x0003_0001, 0xbb, checked(budget.payload())?)?;
    let directory = fixture.directory.join("managed-session-keys");
    std::fs::create_dir(&directory)?;
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
    let mut secret = vec![0; 32];
    getrandom::fill(&mut secret)?;
    let session_keys = checked(SessionKeyRegistry::open(
        directory,
        secret.clone(),
        77,
        4021,
    ))?;
    let seed =
        checked(session_keys.prepare_seed(&SESSION_ACTION, Sha256::digest(SESSION_ACTION).into()))?;
    let session_seed = *seed;
    checked(fixture.client.reconnect())?;
    let current = checked(fixture.client.preparation_state(&fixture.did, 8603))?;
    let session_expiry_sequence = current
        .observed_head_sequence
        .checked_add(10000)
        .ok_or("session sequence")?;
    let granted = checked(issue_session_key(&SessionKeyRequest {
        grantor: checked(layerx_wire::hash::did_id_for_protocol(&fixture.did, 3))?,
        session_public_key: SigningKey::from_bytes(&session_seed)
            .verifying_key()
            .to_bytes(),
        not_before: current.protocol_timestamp,
        expires_at: Some(budget.expiry_ms),
        permitted_activity_types: vec![checked(ActivityType::from_u32(0x0003_0006))?],
        revocation_sequence: Some(budget.revocation_sequence),
        fee_budget: None,
        purpose: SessionPurpose::Activity,
    }))?;
    let grant = checked(SessionGrant::new(
        granted.registration_payload.clone(),
        session_expiry_sequence,
        SESSION_ACTION,
    ))?;
    let compiled = checked(layerx_intents::compile(
        &Intent::v3(IntentKind::SessionGrant(grant)),
        &fixture.registry,
    ))?;
    let grant = submit(
        fixture,
        0x0007_0005,
        0xbc,
        compiled.payload().as_bytes().to_vec(),
    )?;
    Ok(Created {
        sponsor,
        funding: owner.funding,
        provider,
        budget,
        granted,
        session_seed,
        session_keys: Some(session_keys),
        operator_secret: zeroize::Zeroizing::new(secret),
        session_expiry_sequence,
        recovery_root,
        identity: owner.registration,
        rotation,
        recovery,
        creation,
        grant,
    })
}

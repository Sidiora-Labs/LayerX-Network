use std::error::Error;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::time::{Duration, Instant};

use ed25519_dalek::SigningKey;
use layerx_agentd::read::NativeReadRoute;
use layerx_client::availability::RetrievalLimits;
use layerx_client::client::{ClientConfig, ReconnectPolicy};
use layerx_client::evidence::{
    verification_label, verify_account_evidence_with_history, AccountEvidencePolicy, RootSelector,
};
use layerx_client::handover::SequencerHistory;
use layerx_client::lni::handshake::HandshakeConfig;
use layerx_client::lni::schema::Version;
use layerx_client::lni::transport::Limits;
use layerx_client::read::{HistoryKind, HistoryRange};
use layerx_client::Client;
use layerx_programs::hex;
use layerx_sdk::rpc_verification::{VerifiedRpcAccount, VerifiedRpcBalances};
use layerx_types::account::AccountId;
use layerx_types::ids::Did;
use layerx_types::verify::VerificationLevel;
use serde_json::json;

type Result<T> = std::result::Result<T, Box<dyn Error>>;
fn checked<T, E: std::fmt::Debug>(value: std::result::Result<T, E>) -> Result<T> {
    value.map_err(|error| format!("{error:?}").into())
}

fn configuration(socket: &Path, network: u32) -> ClientConfig {
    ClientConfig {
        endpoint: socket.to_owned(),
        handshake: HandshakeConfig {
            built_interface_version: Version::V1_5,
            expected_protocol_version: 3,
            expected_network_id: network,
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

fn verify_account(
    client: &mut Client,
    history: &SequencerHistory,
    stale: &SequencerHistory,
    did: &str,
    account: [u8; 32],
) -> Result<()> {
    checked(client.reconnect())?;
    let value = checked(client.account_with_history(
        account,
        VerificationLevel::BATCH_INCLUDED,
        600,
        history,
    ))?;
    let evidence = checked(verify_account_evidence_with_history(
        value.canonical_bytes(),
        value.proof_material(),
        account,
        None,
        AccountEvidencePolicy {
            expected_protocol_version: 3,
            expected_network_id: history.network_id(),
            handshake_sequencer_key: client.handshake().node().authorised_sequencer_key,
            root_selector: RootSelector::Latest,
        },
        history,
    ))?;
    let proven = evidence.account();
    let result = json!({"account_id": hex::encode(&account), "name": std::str::from_utf8(&proven.name)?,
        "canonical_value": hex::encode(value.canonical_bytes()), "proof_material": hex::encode(value.proof_material()),
        "asset_id": hex::encode(&proven.asset_id()), "balance": proven.balance().to_string(),
        "next_sequence": proven.next_sequence.to_string(), "frozen": proven.frozen,
        "batch_number": evidence.batch_number().to_string(), "verification": verification_label(evidence.level())});
    let verified = checked(VerifiedRpcAccount::from_rpc_result_with_history(
        &result, account, history,
    ))?;
    assert_eq!(verified.canonical_bytes(), value.canonical_bytes());
    assert!(VerifiedRpcAccount::from_rpc_result_with_history(&result, account, stale).is_err());
    for (field, replacement) in [
        ("balance", json!("0")),
        ("next_sequence", json!("999999")),
        ("batch_number", json!("1")),
        ("name", json!("agent:did:layerx:unrelated:main")),
        ("verification", json!("settlement_anchored")),
    ] {
        let mut changed = result.clone();
        assert_ne!(changed[field], replacement);
        changed[field] = replacement;
        assert!(
            VerifiedRpcAccount::from_rpc_result_with_history(&changed, account, history).is_err()
        );
    }
    let mut changed = result.clone();
    let mut proof = value.proof_material().to_vec();
    let end = proof.len() - 1;
    proof[end] ^= 1;
    changed["proof_material"] = json!(hex::encode(&proof));
    assert!(VerifiedRpcAccount::from_rpc_result_with_history(&changed, account, history).is_err());
    let listing = json!({"did": did, "accounts": [result], "verification": verification_label(evidence.level())});
    assert_eq!(
        checked(VerifiedRpcBalances::from_rpc_result_with_history(
            &listing, did, history
        ))?
        .accounts()
        .len(),
        1
    );
    assert!(VerifiedRpcBalances::from_rpc_result_with_history(
        &listing,
        "did:layerx:unrelated",
        history
    )
    .is_err());
    Ok(())
}

fn verify_budget(
    client: &mut Client,
    history: &SequencerHistory,
    stale: &SequencerHistory,
    account: [u8; 32],
    budget_id: [u8; 32],
) -> Result<()> {
    let key = [b"budget:".as_slice(), &budget_id].concat();
    checked(client.reconnect())?;
    let value = checked(client.module_state_with_history(
        3,
        &key,
        VerificationLevel::CHECKPOINT_FINALISED,
        700,
        history,
    ))?;
    let checkpoint = client.head().finalised_checkpoint;
    assert_ne!(checkpoint, [0; 32]);
    let policy = AccountEvidencePolicy {
        expected_protocol_version: 3,
        expected_network_id: history.network_id(),
        handshake_sequencer_key: client.handshake().node().authorised_sequencer_key,
        root_selector: RootSelector::Checkpoint(checkpoint),
    };
    let verify = |value: &[u8], proof: &[u8], key: &[u8], history: &SequencerHistory| {
        layerx_client::evidence::verify_module_evidence_with_history(
            value, proof, 3, key, policy, history,
        )
    };
    let module = checked(verify(
        value.canonical_bytes(),
        value.proof_material(),
        &key,
        history,
    ))?;
    assert_eq!(module.checkpoint_id(), Some(checkpoint));
    assert_eq!(
        value.canonical_bytes().get(2..34),
        Some(budget_id.as_slice())
    );
    let header = checked(history.verify_header(
        &module.signed_header().canonical_bytes,
        &module.signed_header().signature,
    ))?;
    assert_eq!(header.header().epoch(), 2);
    assert!(verify(value.canonical_bytes(), value.proof_material(), &key, stale).is_err());
    assert!(verify(
        value.canonical_bytes(),
        value.proof_material(),
        b"budget:other",
        history
    )
    .is_err());
    let mut altered = value.canonical_bytes().to_vec();
    let end = altered.len() - 1;
    altered[end] ^= 1;
    assert!(verify(&altered, value.proof_material(), &key, history).is_err());
    checked(client.reconnect())?;
    let account_value = checked(client.account_with_history(
        account,
        VerificationLevel::CHECKPOINT_FINALISED,
        701,
        history,
    ))?;
    let account = checked(verify_account_evidence_with_history(
        account_value.canonical_bytes(),
        account_value.proof_material(),
        account,
        None,
        policy,
        history,
    ))?;
    assert_eq!(module.state_root(), account.state_root());
    assert_eq!(
        module.signed_header().canonical_bytes,
        account.signed_header().canonical_bytes
    );
    assert_eq!(
        module.signed_header().signature,
        account.signed_header().signature
    );
    assert!(module.level() >= VerificationLevel::CHECKPOINT_FINALISED);
    assert!(account.level() >= VerificationLevel::CHECKPOINT_FINALISED);
    Ok(())
}

fn verify_route(
    config: ClientConfig,
    actor: Did,
    artifact: &Path,
    account: [u8; 32],
    history: &SequencerHistory,
    activities: &[[u8; 32]],
) -> Result<()> {
    let finality = artifact.with_file_name("handover-finality.conf");
    let mut route = checked(NativeReadRoute::new(
        checked(Client::connect(config.clone()))?,
        actor.clone(),
        "native-handover-history-cursor-bound".to_owned(),
        Instant::now,
    ))?;
    route = checked(route.with_protected_finality(&finality))?;
    route = checked(route.with_protected_genesis(artifact))?;
    let invalid_path = artifact.with_file_name("unprotected-genesis.bin");
    std::fs::copy(artifact, &invalid_path)?;
    std::fs::set_permissions(&invalid_path, std::fs::Permissions::from_mode(0o644))?;
    let invalid = checked(NativeReadRoute::new(
        checked(Client::connect(config.clone()))?,
        actor.clone(),
        "native-handover-history-cursor-bound".to_owned(),
        Instant::now,
    ))?;
    let invalid = checked(invalid.with_protected_finality(&finality))?;
    assert!(invalid.with_protected_genesis(&invalid_path).is_err());
    std::fs::set_permissions(&invalid_path, std::fs::Permissions::from_mode(0o600))?;
    let mut invalid_bytes = std::fs::read(artifact)?;
    invalid_bytes.push(0);
    std::fs::write(&invalid_path, invalid_bytes)?;
    let invalid = checked(NativeReadRoute::new(
        checked(Client::connect(config.clone()))?,
        actor.clone(),
        "native-handover-history-cursor-bound".to_owned(),
        Instant::now,
    ))?;
    let invalid = checked(invalid.with_protected_finality(&finality))?;
    assert!(invalid.with_protected_genesis(&invalid_path).is_err());
    let head = history.verified_head().ok_or("verified head")?.header();
    for batch in [1, head.batch_number()] {
        let value = checked(route.read(&format!("/v1/reads/availability/{batch}")))?;
        assert_eq!(value["complete"], true);
        let header = checked(hex::decode(value["header_hex"].as_str().ok_or("header")?))?;
        let signature: [u8; 64] = checked(hex::decode(
            value["header_signature"].as_str().ok_or("signature")?,
        ))?
        .try_into()
        .map_err(|_| "signature width")?;
        checked(history.verify_header(&header, &signature))?;
    }
    for activity in [
        activities.first().ok_or("old activity")?,
        activities.last().ok_or("new activity")?,
    ] {
        for kind in ["receipt", "proof"] {
            let value =
                checked(route.read(&format!("/v1/reads/{kind}/{}", hex::encode(activity))))?;
            assert_eq!(value["complete"], true);
            assert_eq!(value["activity_id"], hex::encode(activity));
        }
    }
    let mut cursor = None;
    let mut scanned = 0_u64;
    let mut pages = 0;
    loop {
        let path = cursor.as_ref().map_or_else(
            || format!("/v1/reads/history/{}?limit=5", hex::encode(&account)),
            |cursor| {
                format!(
                    "/v1/reads/history/{}?limit=5&cursor={cursor}",
                    hex::encode(&account)
                )
            },
        );
        let value = checked(route.read(&path))?;
        scanned += value["scanned_items"].as_u64().ok_or("scanned history")?;
        pages += 1;
        assert!(pages <= head.last_sequence());
        if value["complete"] == true {
            assert!(value["cursor"].is_null());
            break;
        }
        let next = value["cursor"]
            .as_str()
            .ok_or("history continuation")?
            .to_owned();
        assert_ne!(cursor.as_ref(), Some(&next));
        cursor = Some(next);
    }
    assert_eq!(scanned, head.last_sequence());
    let reopened = checked(NativeReadRoute::new(
        checked(Client::connect(config))?,
        actor,
        "native-handover-history-cursor-bound".to_owned(),
        Instant::now,
    ))?;
    let reopened = checked(reopened.with_protected_finality(&finality))?;
    checked(reopened.with_protected_genesis(artifact))?;
    Ok(())
}

struct HistoryFacts {
    activities: Vec<[u8; 32]>,
    budget_id: [u8; 32],
}

fn history_facts(
    client: &mut Client,
    history: &SequencerHistory,
    registry: &layerx_types::payload::ModuleRegistry,
    count: u64,
) -> Result<HistoryFacts> {
    let head = history.verified_head().ok_or("verified head")?.header();
    assert_eq!(head.batch_number(), count);
    assert_eq!(head.epoch(), 2);
    checked(client.reconnect())?;
    let page = checked(client.history_with_history(
        HistoryRange {
            start_sequence: 1,
            end_sequence: head.last_sequence(),
            page_bound: 256,
            cursor: None,
        },
        VerificationLevel::BATCH_INCLUDED,
        500,
        history,
    ))?;
    assert!(page.cursor.is_none());
    assert_eq!(u64::try_from(page.items.len())?, head.last_sequence());
    let mut activities = Vec::new();
    let mut maintenance = 0;
    let mut budget_id = None;
    for item in page.items {
        match item.kind {
            HistoryKind::Activity => {
                let activity = checked(layerx_wire::activity::decode_signed(
                    item.canonical_bytes(),
                    registry,
                ))?;
                if activity.activity_type().value() == 0x0003_0001 {
                    assert!(budget_id.is_none());
                    budget_id = Some(
                        activity
                            .payload()
                            .get(2..34)
                            .ok_or("budget creation ID")?
                            .try_into()
                            .map_err(|_| "budget ID width")?,
                    );
                }
                activities.push(checked(layerx_wire::hash::activity_id(&activity))?);
            }
            HistoryKind::Receipt => {
                let receipt = checked(layerx_wire::batch_maintenance::decode_maintenance(
                    item.canonical_bytes(),
                ))?;
                assert_eq!(receipt.occupancy().global_sequence, item.global_sequence);
                maintenance += 1;
            }
            HistoryKind::Event => return Err("unexpected event history".into()),
        }
    }
    assert_eq!(maintenance, count);
    Ok(HistoryFacts {
        activities,
        budget_id: budget_id.ok_or("executed budget creation")?,
    })
}

fn main() -> Result<()> {
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.len() != 4 {
        return Err(
            "usage: native_handover_reads SOCKET PUBLIC_EXPORT_DIRECTORY BATCH_COUNT".into(),
        );
    }
    let artifact = Path::new(&arguments[2]).join("handover-genesis.bin");
    let bytes = std::fs::read(&artifact)?;
    let genesis = checked(layerx_wire::handover::decode_genesis_trust(&bytes))?;
    let count: u64 = arguments[3].parse()?;
    let finality_policy = checked(layerx_client::handover::decode_finality_policy(
        &std::fs::read(artifact.with_file_name("handover-finality.conf"))?
    ))?;
    let finality = checked(layerx_paxeer_verifier::PaxeerCheckpointVerifier::new(finality_policy))?;
    let mut history = checked(SequencerHistory::from_genesis_artifact(
        &bytes,
        genesis.network_id,
        genesis.canonical_state_root,
        genesis.initial_sequencer_key,
    ))?;
    let config = configuration(Path::new(&arguments[1]), genesis.network_id);
    let mut client = checked(Client::connect(config.clone()))?;
    let mut stale = None;
    for batch in 1..=count {
        checked(client.reconnect())?;
        checked(client.advance_sequencer_history_with_finality(
            &mut history,
            batch * 4,
            RetrievalLimits {
                maximum_bytes: 16_777_216,
                maximum_chunks: 4096,
                deadline: Duration::from_secs(8),
            },
            Some(&finality),
        ))?;
        if batch == 1 {
            stale = Some(history.clone());
        }
    }
    let facts = history_facts(&mut client, &history, &genesis.registry, count)?;
    let public = SigningKey::from_bytes(&[0x11; 32])
        .verifying_key()
        .to_bytes();
    let did = format!("did:layerx:{}", hex::encode(&public));
    let actor = checked(Did::new(did.as_bytes()))?;
    let account = checked(layerx_wire::hash::account_id_for_protocol(
        &checked(AccountId::parse(&format!("agent:{did}:main")))?,
        3,
    ))?;
    verify_account(
        &mut client,
        &history,
        stale.as_ref().ok_or("initial history")?,
        &did,
        account,
    )?;
    verify_budget(
        &mut client,
        &history,
        stale.as_ref().ok_or("initial history")?,
        account,
        facts.budget_id,
    )?;
    verify_route(
        config,
        actor,
        &artifact,
        account,
        &history,
        &facts.activities,
    )?;
    println!("real native history, Agent reads and SDK account evidence verified across replacement and restart; stale and substituted evidence refused");
    Ok(())
}

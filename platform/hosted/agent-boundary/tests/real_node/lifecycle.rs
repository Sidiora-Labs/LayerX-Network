#[path = "lifecycle/post_upgrade.rs"]
mod post_upgrade;

use super::*;
use layerx_programs_runtime::{
    derive_program_account, AccessDeclaration, AccessMode, AccessSet, AccountAccess, Capability,
    CapabilitySet, KeyAccess, ProgramId as RuntimeProgramId, StorageAccess, StorageNamespace,
};
use layerx_proof::inclusion::{verify_receipt, SequencerAuthorization};
use layerx_proof::merkle::Proof;
use layerx_proof::program::{
    verify_authorized_program_execution, AuthorizedProgramExecutionExpectation,
};
use layerx_proof::receipt::{verify_program_state, AuthorizedBatch};
use layerx_types::intent::ProgramId;
use layerx_types::program_call::{NativeProgramCall, Resources};
use layerx_types::program_lifecycle::{
    NativeProgramDeploy, NativeProgramUpgrade, NativeProgramWindDown, ProgramUpgradePolicy,
    ProgramWindDownOperation,
};
use layerx_wire::hash::{receipt_digest, receipt_execution_batch_id};
use layerx_wire::receipt::{decode_batch_header, decode_merkle_proof, encode_unsigned};

const FEE_LIMIT: u128 = 1_000_000_000_000;
const DEPOSIT: u128 = 100;
const SEED: &[u8] = b"boundary-escrow";

#[test]
fn native_c_lifecycle_fixtures_bind_payload_signature_and_idempotency() {
    for (name, ordinal) in [
        ("deploy", 1),
        ("upgrade", 2),
        ("wind-down-route", 7),
        ("wind-down-deprecate", 7),
        ("wind-down-tombstone", 7),
        ("wind-down-exit", 7),
    ] {
        let path = repository_root()
            .join("platform/sdk/conformance/fixtures")
            .join(format!("native-program-{name}-v3.json"));
        let fixture: serde_json::Value = must(
            serde_json::from_slice(&must(fs::read(&path), "parent-generated C fixture")),
            "C fixture JSON",
        );
        let signed = unhex(field(&fixture, "signed_activity_hex"));
        let payload = unhex(field(&fixture, "payload_hex"));
        let activity = must(
            decode_signed(&signed, &program_registry()),
            "C signed activity",
        );
        assert_eq!(activity.protocol_version(), 3);
        assert_eq!(activity.network_id(), 7);
        assert_eq!(activity.actor_did(), b"did:lxp:native-lifecycle-fixture");
        assert_eq!(activity.fee_limit(), 1000);
        assert_eq!(activity.activity_type().module(), ModuleId::Programs);
        assert_eq!(activity.activity_type().ordinal(), ordinal);
        assert_eq!(activity.payload(), payload);
        assert_eq!(
            hex(&activity.idempotency_key()),
            field(&fixture, "idempotency_key_hex")
        );
        assert_eq!(
            hex(&must(activity_id(&activity), "C activity id")),
            field(&fixture, "activity_id_hex")
        );
        assert_eq!(
            must(
                layerx_wire::activity::encode_signed(&activity),
                "canonical signed encoding"
            ),
            signed
        );
        let public_key = must(
            <[u8; 32]>::try_from(unhex(field(&fixture, "public_key_hex"))),
            "C signer",
        );
        assert_eq!(activity.authority(), public_key);
        let signature = must(
            ed25519_dalek::Signature::from_slice(
                activity
                    .signature()
                    .unwrap_or_else(|| panic!("C signature missing")),
            ),
            "C Ed25519 signature",
        );
        let digest = domain_hash(
            Domain::SignaturePreimage,
            &must(
                layerx_wire::activity::encode_unsigned(&activity),
                "unsigned C activity",
            ),
        );
        let verifying_key = must(
            ed25519_dalek::VerifyingKey::from_bytes(&public_key),
            "C verifying key",
        );
        must(
            verifying_key.verify_strict(&digest, &signature),
            "real C signature verification",
        );
        let mut changed_digest = digest;
        changed_digest[0] ^= 1;
        assert!(verifying_key
            .verify_strict(&changed_digest, &signature)
            .is_err());
        let encoded = match ordinal {
            1 => must(
                must(NativeProgramDeploy::decode(&payload), "C deploy").encode(),
                "deploy re-encode",
            ),
            2 => must(
                must(NativeProgramUpgrade::decode(&payload), "C upgrade").encode(),
                "upgrade re-encode",
            ),
            7 => must(
                must(NativeProgramWindDown::decode(&payload), "C wind-down").encode(),
                "wind-down re-encode",
            ),
            _ => panic!("unexpected lifecycle ordinal"),
        };
        assert_eq!(encoded, payload);
    }
}

fn program_registry() -> ModuleRegistry {
    let activities: Vec<_> = [1, 2, 3, 6, 7]
        .into_iter()
        .map(|ordinal| {
            must(
                ActivityType::new(ModuleId::Programs, ordinal),
                "Programs ordinal",
            )
        })
        .collect();
    let registration = must(
        ModuleRegistration::new(ModuleId::Programs, &activities),
        "Programs registration",
    );
    must(ModuleRegistry::new(&[registration]), "Programs registry")
}

fn escrow_wasm() -> Vec<u8> {
    let path = repository_root().join(
        "programs/sdk/rust/examples/escrow/target/wasm32-unknown-unknown/release/layerx_reference_escrow.wasm",
    );
    let wasm = must(
        fs::read(&path),
        &format!("parent-built escrow required at {}", path.display()),
    );
    assert!(wasm.starts_with(b"\0asm\x01\0\0\0"));
    assert!(wasm.len() > 8);
    assert!(wasm.len() + 108 + 952 < 1_048_576);
    wasm
}

fn deploy_payload(cluster: &Cluster, program: [u8; 32], wasm: &[u8]) -> Vec<u8> {
    must(
        NativeProgramDeploy {
            program_id: ProgramId::new(program),
            guest_abi: 2,
            policy: ProgramUpgradePolicy::Authority(cluster.actor.source),
            new_hash: Sha256::digest(wasm).into(),
            interface: None,
            wasm,
        }
        .encode(),
        "escrow deploy payload",
    )
}

fn upgrade_payload(program: [u8; 32], previous: &[u8], wasm: &[u8]) -> Vec<u8> {
    must(
        NativeProgramUpgrade {
            program_id: ProgramId::new(program),
            guest_abi: 2,
            old_hash: Sha256::digest(previous).into(),
            new_hash: Sha256::digest(wasm).into(),
            migration_hook: &[],
            clear_interface: false,
            interface: None,
            wasm,
        }
        .encode(),
        "escrow upgrade payload",
    )
}

fn wind_down_payload(program: [u8; 32], operation: ProgramWindDownOperation<'_>) -> Vec<u8> {
    must(
        NativeProgramWindDown {
            program_id: ProgramId::new(program),
            operation,
        }
        .encode(),
        "wind-down payload",
    )
}

fn derived_account(program: [u8; 32]) -> [u8; 32] {
    must(
        derive_program_account(
            must(RuntimeProgramId::new(program), "runtime program"),
            SEED,
        ),
        "derived escrow account",
    )
    .bytes()
}

fn escrow_access(cluster: &Cluster, program: [u8; 32], account: [u8; 32]) -> Vec<u8> {
    let mut key = b"lx.ref.escrow/".to_vec();
    key.extend_from_slice(&account);
    let namespace =
        StorageNamespace::shared(must(RuntimeProgramId::new(program), "runtime program"));
    let storage = [AccessMode::Read, AccessMode::Write].map(|mode| {
        must(
            StorageAccess::new(
                namespace,
                mode,
                must(KeyAccess::exact(&key), "escrow storage key"),
            ),
            "escrow storage access",
        )
    });
    let accounts = [cluster.actor.source, account].map(|identifier| {
        must(
            AccountAccess::new(identifier, cluster.asset, AccessMode::Write),
            "escrow account access",
        )
    });
    let accesses = must(
        AccessSet::new(storage, accounts),
        "escrow explicit accesses",
    );
    must(
        AccessDeclaration::explicit(accesses).canonical_bytes(),
        "runtime access encoder",
    )
}

fn escrow_resources() -> Resources {
    let budget = layerx_programs_runtime::ResourceBudget::declared();
    must(
        layerx_programs_runtime::DeclaredBudget::new(
            budget.cpu_fuel(),
            budget.memory_bytes(),
            budget.storage_read_bytes(),
            budget.storage_write_bytes(),
            budget.output_values(),
            budget.output_bytes(),
            budget.table_elements(),
        ),
        "runtime resource admission",
    );
    Resources([
        budget.cpu_fuel(),
        budget.memory_bytes(),
        budget.storage_read_bytes(),
        budget.storage_write_bytes(),
        u64::from(budget.output_values()),
        budget.output_bytes(),
        u64::from(budget.table_elements()),
    ])
}

fn escrow_open(cluster: &Cluster, program: [u8; 32], account: [u8; 32]) -> Vec<u8> {
    let mut calldata = vec![1, 1];
    calldata
        .extend_from_slice(&must(u16::try_from(SEED.len()), "escrow seed length").to_be_bytes());
    calldata.extend_from_slice(SEED);
    for identifier in [
        account,
        cluster.asset,
        cluster.actor.source,
        cluster.actor.source,
    ] {
        calldata.extend_from_slice(&identifier);
    }
    calldata.extend_from_slice(&DEPOSIT.to_be_bytes());
    calldata.extend_from_slice(&[0x41; 32]);
    calldata.extend_from_slice(&[0x42; 32]);
    let capabilities = must(
        CapabilitySet::new([
            Capability::EmitEvent,
            Capability::Transfer402 {
                asset: cluster.asset,
                to: account,
                maximum_amount: DEPOSIT,
            },
            Capability::SharedStorageRead,
            Capability::SharedStorageWrite,
        ]),
        "escrow capabilities",
    )
    .canonical_encoding();
    let access = escrow_access(cluster, program, account);
    must(
        NativeProgramCall {
            program_id: ProgramId::new(program),
            guest_abi: 2,
            entrypoint: b"layerx_call",
            calldata: &calldata,
            capabilities: &capabilities,
            access_declaration: &access,
            response_capacity: 1024,
            resources: escrow_resources(),
        }
        .encode(),
        "escrow OPEN",
    )
}

fn verify_lifecycle_receipt(
    cluster: &Cluster,
    signed: &[u8],
    ordinal: u16,
    result: &serde_json::Value,
) {
    let activity = must(
        decode_signed(signed, &program_registry()),
        "signed Programs activity",
    );
    assert_eq!(activity.activity_type().module(), ModuleId::Programs);
    assert_eq!(activity.activity_type().ordinal(), ordinal);
    assert_eq!(activity.protocol_version(), PROTOCOL_VERSION);
    assert_eq!(activity.network_id(), NETWORK_ID);
    let expected_id = must(activity_id(&activity), "Programs activity id");
    assert_eq!(field(result, "activity_id"), hex(&expected_id));
    let bytes = unhex(field(result, "receipt"));
    let receipt = must(
        verify_sequencer_signature(&bytes, cluster.sequencer_key),
        "offline receipt signature",
    );
    let protocol = receipt
        .protocol()
        .unwrap_or_else(|| panic!("protocol receipt required"));
    assert_eq!(protocol.activity_id(), expected_id);
    assert_eq!(protocol.protocol_version(), PROTOCOL_VERSION);
    assert_eq!(protocol.module_id(), 9);
    assert_eq!(protocol.module_version(), 4);
    assert_eq!(protocol.operation(), if ordinal == 3 { 3 } else { 0 });
    let authority = verify_lifecycle_batch(cluster, &receipt, &bytes);
    if ordinal == 3 {
        let call = must(
            NativeProgramCall::decode(activity.payload()),
            "call identity",
        );
        must(
            verify_authorized_program_execution(
                &bytes,
                &unhex(field(result, "terminal_payload")),
                &unhex(field(result, "call_graph")),
                AuthorizedProgramExecutionExpectation {
                    authority,
                    activity_id: expected_id,
                    program_id: call.program_id.bytes(),
                    guest_abi_version: 2,
                },
            ),
            "offline escrow execution artifacts",
        );
    } else {
        must(
            verify_program_state(&bytes, &authority),
            "offline lifecycle state receipt",
        );
        assert!(result.get("program_id").is_none());
        assert_eq!(field(result, "terminal_payload"), "");
        assert_eq!(field(result, "call_graph"), "");
    }
    let terminal = unhex(field(result, "terminal_payload"));
    assert_eq!(
        protocol.result_code(),
        0,
        "Programs ordinal {ordinal} must succeed; global_sequence={}, signed_account_sequence={}; authenticated terminal={} ({:?})",
        protocol.global_sequence(),
        activity.account_sequence(),
        hex(&terminal),
        String::from_utf8_lossy(&terminal)
    );
    assert_eq!(
        field(result, "state"),
        if ordinal == 6 {
            "completed"
        } else {
            "executed"
        }
    );
}

fn verify_lifecycle_batch(
    cluster: &Cluster,
    receipt: &layerx_wire::receipt::Receipt,
    bytes: &[u8],
) -> AuthorizedBatch {
    let protocol = receipt
        .protocol()
        .unwrap_or_else(|| panic!("protocol receipt required"));
    let digest = must(
        receipt_digest(&must(encode_unsigned(receipt), "unsigned receipt")),
        "receipt digest",
    );
    let evidence = http_get(
        cluster.program_port,
        &format!(
            "/v1/batches/{}/receipt-authority?receipt_digest={}",
            hex(&protocol.batch_id()),
            hex(&digest),
        ),
        &cluster.program_token,
    );
    assert_eq!(evidence.status, 200, "{}", evidence.text());
    let document = evidence.json();
    assert_eq!(
        field(&document, "sequencer_public_key"),
        hex(&cluster.sequencer_key)
    );
    let evidence = &document["batch_evidence"];
    let header_bytes = unhex(field(evidence, "header_hex"));
    let header = must(decode_batch_header(&header_bytes), "batch header");
    assert_eq!(header.network_id(), NETWORK_ID);
    assert_eq!(header.protocol_version(), PROTOCOL_VERSION);
    assert!(
        (header.first_sequence()..=header.last_sequence()).contains(&protocol.global_sequence())
    );
    let signature = must(
        <[u8; 64]>::try_from(unhex(field(evidence, "header_signature"))),
        "header signature",
    );
    let wire_proof = must(
        decode_merkle_proof(&unhex(field(evidence, "receipt_proof_hex"))),
        "receipt proof",
    );
    let proof = must(
        Proof::new(
            wire_proof.leaf_index(),
            wire_proof.leaf_count(),
            wire_proof.siblings().to_vec(),
        ),
        "receipt inclusion proof",
    );
    let authorization = SequencerAuthorization::new(
        header.sequencer_id(),
        cluster.sequencer_key,
        FIRST_BATCH,
        LAST_BATCH,
    );
    must(
        verify_receipt(bytes, &proof, &header_bytes, &signature, &authorization),
        "offline receipt inclusion",
    );
    let mut changed_signature = signature;
    changed_signature[0] ^= 1;
    assert!(verify_receipt(
        bytes,
        &proof,
        &header_bytes,
        &changed_signature,
        &authorization
    )
    .is_err());
    let mut changed_receipt = bytes.to_vec();
    let last = changed_receipt.len() - 1;
    changed_receipt[last] ^= 1;
    assert!(verify_sequencer_signature(&changed_receipt, cluster.sequencer_key).is_err());
    let batch_id = must(
        receipt_execution_batch_id(protocol, &header),
        "receipt batch identity",
    );
    assert_eq!(protocol.batch_id(), batch_id);
    AuthorizedBatch::new(
        batch_id,
        protocol.asset(),
        header.previous_state_root(),
        header.resulting_state_root(),
        cluster.sequencer_key,
    )
}

fn submit_lifecycle(
    cluster: &Cluster,
    path: &str,
    signed: &[u8],
    ordinal: u16,
    key: &str,
) -> Submitted {
    let deadline = Instant::now() + Duration::from_secs(60);
    let answer = loop {
        let answer = cluster
            .client
            .call(&Call::submit(path, &cluster.gateway_token, key, signed));
        if answer.status != 202 || Instant::now() >= deadline {
            break answer;
        }
        thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(answer.status, 200, "{path}: {}", answer.text());
    let document = answer.json();
    let result = &document["result"];
    verify_lifecycle_receipt(cluster, signed, ordinal, result);
    assert_eq!(journal_record(cluster, key)["attempts"], 1);
    Submitted {
        key: key.to_owned(),
        activity_id: field(result, "activity_id").to_owned(),
        receipt: unhex(field(result, "receipt")),
        body: answer.text(),
    }
}

fn check_idempotency_lookup(cluster: &Cluster, signed: &[u8], submitted: &Submitted) {
    let activity = must(
        decode_signed(signed, &program_registry()),
        "idempotency binding",
    );
    let canonical = hex(&activity.idempotency_key());
    for key in [&submitted.key, &canonical] {
        let path = format!("/v1/programs/receipts/by-idempotency/{key}");
        let answer = cluster.client.get(&path, Some(&cluster.gateway_token));
        assert_eq!(answer.status, 200, "{path}: {}", answer.text());
        assert_eq!(
            answer.json()["result"]["activity_id"],
            submitted.activity_id
        );
        assert_eq!(answer.json()["result"]["receipt"], hex(&submitted.receipt));
        assert_refusal(&cluster.client.get(&path, None), 401, "identity_required");
        assert_refusal(
            &cluster.client.get(&path, Some(&cluster.registry_token)),
            403,
            "entitlement_denied",
        );
    }
}

#[test]
fn real_escrow_deploy_call_upgrade_deprecate_exit_receipts_and_durable_dedup() {
    let wasm = escrow_wasm();
    let (mut cluster, custody) = custody::start_funded_cluster();
    custody.verify_evidence();
    check_readiness(&cluster);
    let program = random32();
    let account = derived_account(program);
    let mut upgraded = wasm.clone();
    upgraded.extend_from_slice(&[0, 2, 1, b'u']);
    assert_ne!(Sha256::digest(&wasm), Sha256::digest(&upgraded));
    let operations = escrow_operations(&cluster, program, account, &wasm, &upgraded);
    let mut completed = Vec::new();
    for (index, (ordinal, path, payload)) in operations.iter().enumerate() {
        let sequence = must(
            u64::try_from(index + 2),
            "activity sequence after custody credit",
        );
        let signed =
            signed_program_operation(&cluster.actor, *ordinal, sequence, FEE_LIMIT, payload);
        let key = format!("escrow-{index}-{}", token());
        let submitted = submit_lifecycle(&cluster, path, &signed, *ordinal, &key);
        check_idempotency_lookup(&cluster, &signed, &submitted);
        let replay =
            cluster
                .client
                .call(&Call::submit(path, &cluster.gateway_token, &key, &signed));
        assert_eq!(replay.status, 200, "{}", replay.text());
        assert_eq!(replay.text(), submitted.body);
        let conflict =
            signed_program_operation(&cluster.actor, *ordinal, sequence, FEE_LIMIT, payload);
        assert_refusal(
            &cluster
                .client
                .call(&Call::submit(path, &cluster.gateway_token, &key, &conflict)),
            409,
            "idempotency_conflict",
        );
        completed.push((*ordinal, *path, signed, submitted));
        check_readiness(&cluster);
    }
    cluster.boundary.stop();
    cluster.boundary.start();
    check_readiness(&cluster);
    for (ordinal, path, signed, submitted) in &completed {
        let replay = cluster.client.call(&Call::submit(
            path,
            &cluster.gateway_token,
            &submitted.key,
            signed,
        ));
        assert_eq!(replay.status, 200, "{}", replay.text());
        assert_eq!(replay.text(), submitted.body);
        verify_lifecycle_receipt(&cluster, signed, *ordinal, &replay.json()["result"]);
        check_idempotency_lookup(&cluster, signed, submitted);
        assert_eq!(journal_record(&cluster, &submitted.key)["attempts"], 1);
    }
    cluster.sequencer.stop();
    cluster.replica.stop();
    for (_, path, signed, submitted) in &completed {
        let replay = cluster.client.call(&Call::submit(
            path,
            &cluster.gateway_token,
            &submitted.key,
            signed,
        ));
        assert_eq!(replay.status, 200, "{}", replay.text());
        assert_eq!(replay.text(), submitted.body);
        assert_eq!(journal_record(&cluster, &submitted.key)["attempts"], 1);
    }
}

fn assert_pre_submit_refusal(
    cluster: &Cluster,
    ordinal: u16,
    path: &str,
    payload: &[u8],
    code: &str,
) {
    let signed = signed_program_operation(&cluster.actor, ordinal, 1, FEE_LIMIT, payload);
    let key = format!("invalid-{}", token());
    let answer = cluster
        .client
        .call(&Call::submit(path, &cluster.gateway_token, &key, &signed));
    assert_refusal(&answer, 400, code);
    let journal = cluster
        .state_dir
        .join("journal")
        .join(format!("{:x}.json", Sha256::digest(key.as_bytes())));
    assert!(
        !journal.exists(),
        "pre-submit refusal must not enter the LNI submission journal"
    );
}

#[test]
fn lifecycle_routes_refuse_auth_ordinal_hash_policy_interface_and_size_before_submit() {
    let wasm = escrow_wasm();
    let cluster = start_cluster();
    check_readiness(&cluster);
    let program = random32();
    let deploy = deploy_payload(&cluster, program, &wasm);
    let upgrade = upgrade_payload(program, &wasm, &wasm);
    let wind_down = wind_down_payload(program, ProgramWindDownOperation::Tombstone);
    check_lifecycle_security(&cluster, program, &deploy, &upgrade, &wind_down);
    check_lifecycle_payload_refusals(&cluster, &wasm, &deploy, &upgrade);
    check_wind_down_refusals(&cluster, program);
}

fn check_lifecycle_security(
    cluster: &Cluster,
    program: [u8; 32],
    deploy: &[u8],
    upgrade: &[u8],
    wind_down: &[u8],
) {
    for (ordinal, path, payload) in [
        (1, "/v1/programs/deploy", deploy),
        (2, "/v1/programs/upgrade", upgrade),
        (7, "/v1/programs/wind-down", wind_down),
    ] {
        let signed = signed_program_operation(&cluster.actor, ordinal, 1, FEE_LIMIT, payload);
        let mut request = Call::submit(path, &cluster.gateway_token, "pre-submit", &signed);
        request.bearer = None;
        assert_refusal(&cluster.client.call(&request), 401, "identity_required");
        request.bearer = Some(&cluster.registry_token);
        assert_refusal(&cluster.client.call(&request), 403, "entitlement_denied");
        let bad_token = token();
        request.bearer = Some(&bad_token);
        assert_refusal(&cluster.client.call(&request), 401, "identity_required");
        request.bearer = Some(&cluster.gateway_token);
        request.idempotency = None;
        assert_refusal(
            &cluster.client.call(&request),
            400,
            "idempotency_key_required",
        );
        request.idempotency = Some("pre-submit");
        request.content_type = Some("application/json");
        assert_refusal(&cluster.client.call(&request), 400, "content_type_required");
        assert_pre_submit_refusal(
            cluster,
            3,
            path,
            &escrow_open(cluster, program, derived_account(program)),
            "wrong_program_operation",
        );
        let oversized = vec![0_u8; 1_048_577];
        request.content_type = Some("application/octet-stream");
        request.body = &oversized;
        assert_refusal(
            &cluster.client.call(&request),
            400,
            "invalid_activity_length",
        );
    }
}

fn check_lifecycle_payload_refusals(cluster: &Cluster, wasm: &[u8], deploy: &[u8], upgrade: &[u8]) {
    for (ordinal, path, original, malformed) in [
        (1, "/v1/programs/deploy", deploy, "malformed_program_deploy"),
        (
            2,
            "/v1/programs/upgrade",
            upgrade,
            "malformed_program_upgrade",
        ),
    ] {
        let mut bad_hash = original.to_vec();
        bad_hash[68] ^= 1;
        assert_pre_submit_refusal(
            cluster,
            ordinal,
            path,
            &bad_hash,
            "program_payload_hash_mismatch",
        );
        let mut reserved = original.to_vec();
        reserved[35] = 1;
        assert_pre_submit_refusal(cluster, ordinal, path, &reserved, malformed);
        let mut trailing = original.to_vec();
        trailing.push(0);
        assert_pre_submit_refusal(cluster, ordinal, path, &trailing, malformed);
        let fixed = if ordinal == 1 { 104 } else { 106 };
        let mut interface = original[..fixed].to_vec();
        interface.extend_from_slice(&953_u32.to_be_bytes());
        interface.extend_from_slice(&[0x61; 953]);
        interface.extend_from_slice(wasm);
        assert_pre_submit_refusal(cluster, ordinal, path, &interface, malformed);
        let mut wasm_length = original.to_vec();
        wasm_length[fixed - 4..fixed].copy_from_slice(&1_048_577_u32.to_be_bytes());
        assert_pre_submit_refusal(cluster, ordinal, path, &wasm_length, malformed);
    }
    for (policy, authority) in [
        (0, cluster.actor.source),
        (1, [0; 32]),
        (2, cluster.actor.source),
    ] {
        let mut invalid = deploy.to_vec();
        invalid[34] = policy;
        invalid[36..68].copy_from_slice(&authority);
        assert_pre_submit_refusal(
            cluster,
            1,
            "/v1/programs/deploy",
            &invalid,
            "malformed_program_deploy",
        );
    }
    let mut invalid_flags = upgrade.to_vec();
    invalid_flags[34] = 4;
    assert_pre_submit_refusal(
        cluster,
        2,
        "/v1/programs/upgrade",
        &invalid_flags,
        "malformed_program_upgrade",
    );
}

fn check_wind_down_refusals(cluster: &Cluster, program: [u8; 32]) {
    for operation in [
        ProgramWindDownOperation::Route {
            account: derived_account(program),
            asset: cluster.asset,
            destination: cluster.actor.source,
            seed: SEED,
        },
        ProgramWindDownOperation::Deprecate {
            exit_program: program,
            deadline_batch: LAST_BATCH,
        },
        ProgramWindDownOperation::Tombstone,
        ProgramWindDownOperation::Exit {
            account: derived_account(program),
        },
    ] {
        let mut invalid = wind_down_payload(program, operation);
        invalid.push(0);
        assert_pre_submit_refusal(
            cluster,
            7,
            "/v1/programs/wind-down",
            &invalid,
            "malformed_program_wind_down",
        );
        invalid.truncate(invalid.len() - 2);
        assert_pre_submit_refusal(
            cluster,
            7,
            "/v1/programs/wind-down",
            &invalid,
            "malformed_program_wind_down",
        );
    }
    let unknown = cluster.client.get(
        &format!("/v1/programs/receipts/by-idempotency/{}", hex(&random32())),
        Some(&cluster.gateway_token),
    );
    assert_refusal(&unknown, 404, "receipt_not_found");
}

#[test]
fn lifecycle_unknown_submissions_are_not_resubmitted_after_boundary_restart() {
    let wasm = escrow_wasm();
    let mut cluster = start_cluster();
    check_readiness(&cluster);
    let program = random32();
    let operations = [
        (
            1,
            "/v1/programs/deploy",
            deploy_payload(&cluster, program, &wasm),
        ),
        (
            2,
            "/v1/programs/upgrade",
            upgrade_payload(program, &wasm, &wasm),
        ),
        (
            7,
            "/v1/programs/wind-down",
            wind_down_payload(program, ProgramWindDownOperation::Tombstone),
        ),
    ];
    cluster.boundary.stop();
    let run_directory = cluster.root.join("run");
    must(
        fs::set_permissions(&run_directory, fs::Permissions::from_mode(0o700)),
        "deny boundary LNI access",
    );
    cluster.boundary.start();
    let mut pending = Vec::new();
    for (ordinal, path, payload) in operations {
        let signed = signed_program_operation(&cluster.actor, ordinal, 1, FEE_LIMIT, &payload);
        let key = format!("uncertain-lifecycle-{ordinal}-{}", token());
        let answer =
            cluster
                .client
                .call(&Call::submit(path, &cluster.gateway_token, &key, &signed));
        assert_refusal(&answer, 503, "node_unavailable");
        let record = journal_record(&cluster, &key);
        assert_eq!(record["state"], "submitting");
        assert_eq!(record["attempts"], 1);
        pending.push((path, signed, key, field(&record, "activity_id").to_owned()));
    }
    cluster.boundary.stop();
    must(
        fs::set_permissions(&run_directory, fs::Permissions::from_mode(0o750)),
        "restore boundary LNI access",
    );
    cluster.boundary.start();
    check_readiness(&cluster);
    for (path, signed, key, expected_id) in pending {
        let retry = cluster
            .client
            .call(&Call::submit(path, &cluster.gateway_token, &key, &signed));
        assert_eq!(retry.status, 202, "{}", retry.text());
        assert_eq!(retry.json()["state"], "unknown");
        assert_eq!(journal_record(&cluster, &key)["attempts"], 1);
        assert_eq!(journal_record(&cluster, &key)["state"], "submitting");
        let receipt = cluster.client.get(
            &format!("/v1/receipts/{expected_id}"),
            Some(&cluster.gateway_token),
        );
        assert_refusal(&receipt, 404, "receipt_not_found");
    }
}

fn escrow_account_registration(program: [u8; 32], asset: [u8; 32]) -> Vec<u8> {
    let mut payload = program.to_vec();
    payload.extend_from_slice(b"LXPA1");
    payload.extend_from_slice(&asset);
    payload
        .extend_from_slice(&must(u32::try_from(SEED.len()), "account seed length").to_be_bytes());
    payload.extend_from_slice(SEED);
    assert_eq!(payload.len(), 73 + SEED.len());
    payload
}

#[test]
fn real_escrow_requires_registered_destination_account() {
    let wasm = escrow_wasm();
    let (cluster, custody) = custody::start_funded_cluster();
    custody.verify_evidence();
    check_readiness(&cluster);
    let program = random32();
    let deploy = signed_program_operation(
        &cluster.actor,
        1,
        2,
        FEE_LIMIT,
        &deploy_payload(&cluster, program, &wasm),
    );
    submit_lifecycle(&cluster, "/v1/programs/deploy", &deploy, 1, &token());
    let call = signed_program_operation(
        &cluster.actor,
        3,
        3,
        FEE_LIMIT,
        &escrow_open(&cluster, program, derived_account(program)),
    );
    let answer = cluster.client.call(&Call::submit(
        "/v1/programs/call",
        &cluster.gateway_token,
        &token(),
        &call,
    ));
    assert_eq!(answer.status, 200, "{}", answer.text());
    let document = answer.json();
    let result = &document["result"];
    let bytes = unhex(field(result, "receipt"));
    let receipt = must(
        verify_sequencer_signature(&bytes, cluster.sequencer_key),
        "refusal signature",
    );
    let authority = verify_lifecycle_batch(&cluster, &receipt, &bytes);
    let activity = must(decode_signed(&call, &program_registry()), "refused call");
    let terminal = unhex(field(result, "terminal_payload"));
    must(
        verify_authorized_program_execution(
            &bytes,
            &terminal,
            &unhex(field(result, "call_graph")),
            AuthorizedProgramExecutionExpectation {
                authority,
                activity_id: must(activity_id(&activity), "refused activity id"),
                program_id: program,
                guest_abi_version: 2,
            },
        ),
        "authenticated unregistered-account refusal",
    );
    assert_eq!(
        receipt
            .protocol()
            .unwrap_or_else(|| panic!("protocol receipt required"))
            .result_code(),
        -736
    );
    assert_eq!(terminal, b"LXP/programs/settlement-failure/v1\0\x09");
    println!(
        "authenticated unregistered destination refusal: {}",
        hex(&terminal)
    );
}

fn escrow_operations(
    cluster: &Cluster,
    program: [u8; 32],
    account: [u8; 32],
    wasm: &[u8],
    upgraded: &[u8],
) -> [(u16, &'static str, Vec<u8>); 7] {
    [
        (
            1,
            "/v1/programs/deploy",
            deploy_payload(cluster, program, wasm),
        ),
        (
            6,
            "/v1/activities",
            escrow_account_registration(program, cluster.asset),
        ),
        (
            3,
            "/v1/programs/call",
            escrow_open(cluster, program, account),
        ),
        (
            2,
            "/v1/programs/upgrade",
            upgrade_payload(program, wasm, upgraded),
        ),
        (
            7,
            "/v1/programs/wind-down",
            wind_down_payload(
                program,
                ProgramWindDownOperation::Route {
                    account,
                    asset: cluster.asset,
                    destination: cluster.actor.source,
                    seed: SEED,
                },
            ),
        ),
        (
            7,
            "/v1/programs/wind-down",
            wind_down_payload(
                program,
                ProgramWindDownOperation::Deprecate {
                    exit_program: program,
                    deadline_batch: LAST_BATCH,
                },
            ),
        ),
        (
            7,
            "/v1/programs/wind-down",
            wind_down_payload(program, ProgramWindDownOperation::Exit { account }),
        ),
    ]
}

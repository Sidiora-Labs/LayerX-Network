mod required;
use required::Required;

use layerx_platform_core::hex_decode;
use layerx_proof::receipt::{verify_program_state, verify_sequencer_signature, AuthorizedBatch};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use std::{env, fs};

fn main() {
    let arguments = env::args().collect::<Vec<_>>();
    assert_eq!(arguments.len(), 2, "expected receipt file only");
    let bytes = fs::read(&arguments[1]).required("receipt bytes");
    let trusted = fs::read_to_string(
        env::var("LAYERX_TEST_SEQUENCER_KEY_FILE").required("trusted signer path"),
    )
    .required("trusted signer");
    let signer: [u8; 32] = hex_decode(trusted.trim())
        .required("signer hex")
        .try_into()
        .required("signer size");
    let receipt =
        verify_sequencer_signature(&bytes, signer).required("independent receipt signature");
    let protocol = receipt.protocol().required("protocol receipt");
    assert_eq!(protocol.protocol_version(), 3);
    assert_eq!(
        (
            protocol.module_id(),
            protocol.module_version(),
            protocol.operation()
        ),
        (9, 4, 0)
    );
    assert_eq!(protocol.result_code(), 0);
    assert!(protocol.program_outcome().is_none());
    verify_activity_binding(protocol.activity_id());
    verify_batch(&bytes, protocol, signer);
}

fn verify_activity_binding(activity_id: [u8; 32]) {
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(env::var("LAYERX_LIFECYCLE_MANIFEST").required("manifest path"))
            .required("manifest"),
    )
    .required("manifest JSON");
    let entry = manifest
        .as_array()
        .required("manifest array")
        .iter()
        .find(|entry| {
            entry["activity_id"]
                .as_str()
                .and_then(|value| hex_decode(value).ok())
                .is_some_and(|value| value == activity_id)
        })
        .required("receipt must bind a locally signed lifecycle activity");
    let ordinal = match entry["route"].as_str().required("route") {
        "deploy" => 1,
        "upgrade" => 2,
        "wind-down" => 7,
        _ => panic!("not a lifecycle route"),
    };
    let kind = ActivityType::new(ModuleId::Programs, ordinal).required("ordinal");
    let registry = ModuleRegistry::new(&[
        ModuleRegistration::new(ModuleId::Programs, &[kind]).required("module")
    ])
    .required("registry");
    let signed = fs::read(
        entry["signed_file"]
            .as_str()
            .required("signed activity path"),
    )
    .required("signed activity");
    let activity = layerx_wire::activity::decode_signed(&signed, &registry)
        .required("canonical signed activity");
    assert_eq!(activity.network_id(), 7332);
    assert_eq!(activity.protocol_version(), 3);
    assert_eq!(activity.activity_type(), kind);
    assert_eq!(
        layerx_wire::activity::encode_signed(&activity).required("canonical encoding"),
        signed
    );
    assert_eq!(
        layerx_wire::hash::activity_id(&activity).required("activity ID"),
        activity_id
    );
}

fn verify_batch(bytes: &[u8], protocol: &layerx_wire::receipt::ProtocolReceipt, signer: [u8; 32]) {
    let evidence = independent_evidence(bytes, signer);
    let sequencer_id: [u8; 32] =
        hex_decode(&env::var("LAYERX_TEST_SEQUENCER_ID").required("pinned sequencer ID"))
            .required("sequencer hex")
            .try_into()
            .required("sequencer ID size");
    let authorization =
        layerx_proof::inclusion::SequencerAuthorization::new(sequencer_id, signer, 1, u64::MAX);
    let proof = layerx_proof::merkle::decode_proof(&evidence.receipt_proof)
        .required("receipt inclusion proof");
    let inclusion = layerx_proof::inclusion::verify_receipt(
        bytes,
        &proof,
        &evidence.header,
        &evidence.header_signature,
        &authorization,
    )
    .required("independent signed batch and receipt inclusion");
    let header = inclusion.header().header();
    assert_eq!(header.network_id(), 7332);
    assert_eq!(header.protocol_version(), 3);
    assert!(
        protocol.global_sequence() >= header.first_sequence()
            && protocol.global_sequence() <= header.last_sequence()
    );
    let batch_id = layerx_wire::hash::receipt_execution_batch_id(protocol, header)
        .required("header-derived execution batch identity");
    assert_eq!(protocol.batch_id(), batch_id);
    assert_eq!(protocol.previous_state_root(), header.previous_state_root());
    assert_eq!(
        protocol.resulting_state_root(),
        header.resulting_state_root()
    );
    let authority = AuthorizedBatch::new(
        batch_id,
        protocol.asset(),
        header.previous_state_root(),
        header.resulting_state_root(),
        signer,
    );
    verify_program_state(bytes, &authority).required("independent Programs state proof");
    let mut bad_header = evidence.header.clone();
    *bad_header.last_mut().required("header") ^= 1;
    assert!(
        layerx_proof::inclusion::verify_receipt(
            bytes,
            &proof,
            &bad_header,
            &evidence.header_signature,
            &authorization
        )
        .is_err(),
        "mutated independent header must fail"
    );
    let mut corrupted = bytes.to_vec();
    *corrupted.last_mut().required("nonempty receipt") ^= 1;
    assert!(
        verify_program_state(&corrupted, &authority).is_err(),
        "tampered evidence must fail"
    );
    assert!(
        layerx_proof::inclusion::verify_receipt(
            &corrupted,
            &proof,
            &evidence.header,
            &evidence.header_signature,
            &authorization
        )
        .is_err(),
        "mutated receipt inclusion must fail"
    );
}

fn independent_evidence(
    bytes: &[u8],
    signer: [u8; 32],
) -> layerx_platform_authority::BatchEvidence {
    use layerx_platform_authority::{parse_replica_evidence, receipt_locator};
    use std::{path::PathBuf, process::Command};
    let locator = receipt_locator(bytes).required("receipt locator, not authority");
    let digest = layerx_platform_core::hex_encode(&locator.receipt_digest);
    let directory =
        PathBuf::from(env::var_os("LAYERX_TEST_EVIDENCE_DIR").required("evidence directory"));
    let path = directory.join(format!("{digest}.json"));
    if !path.exists() {
        let base = env::var("LAYERX_TEST_AUTHORITY_URL")
            .required("offline evidence absent; real authority URL required");
        assert!(
            base.starts_with("https://localhost:"),
            "only local TLS authority permitted"
        );
        let url = format!(
            "{base}/v1/batches/{}/receipt-authority?receipt_digest={digest}",
            layerx_platform_core::hex_encode(&locator.batch_id)
        );
        let output = Command::new("/usr/bin/curl")
            .arg("--config")
            .arg(
                env::var_os("LAYERX_TEST_AUTHORITY_CURL_CONFIG")
                    .required("private local TLS configuration"),
            )
            .arg("--url")
            .arg(url)
            .output()
            .required("real authority evidence fetch");
        assert!(
            output.status.success(),
            "authority evidence fetch failed; diagnostics redacted"
        );
        fs::write(&path, output.stdout).required("retain independent authority evidence");
        fs::write(directory.join(format!("{digest}.receipt")), bytes)
            .required("retain receipt for offline replay");
    }
    let replica_id: [u8; 32] =
        hex_decode(&env::var("LAYERX_TEST_REPLICA_ID").required("pinned replica ID"))
            .required("replica hex")
            .try_into()
            .required("replica ID size");
    parse_replica_evidence(
        &fs::read(path).required("independent evidence file"),
        replica_id,
        signer,
    )
    .required("pinned replica and sequencer evidence")
}

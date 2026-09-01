#[allow(dead_code)]
#[path = "../src/main.rs"]
mod generator;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use generator::{
    agent_sdk_drift_gate, agent_sdk_generator, programs_golden_drift_gate, write_programs_golden,
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

fn directory(label: &str) -> PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-sdk-gen-{label}-{}-{sequence}",
        std::process::id()
    ))
}

fn schema() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schema/agent-api")
}

fn write(generated: &generator::Generated, root: &Path) {
    for (relative, source) in &generated.files {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap_or_else(|| panic!("parent missing")))
            .unwrap_or_else(|error| panic!("create output: {error}"));
        fs::write(&path, source).unwrap_or_else(|error| panic!("write output: {error}"));
    }
}

fn copy_schema(root: &Path) {
    fs::create_dir_all(root).unwrap_or_else(|error| panic!("create schema: {error}"));
    for name in [
        "v1.kvx",
        "identity.kvx",
        "write.kvx",
        "read.kvx",
        "stream.kvx",
        "errors.kvx",
        "approval.kvx",
        "programs.kvx",
    ] {
        fs::copy(schema().join(name), root.join(name))
            .unwrap_or_else(|error| panic!("copy {name}: {error}"));
    }
}

#[test]
fn golden_schema_generation_is_byte_deterministic() {
    let first =
        agent_sdk_generator(&schema()).unwrap_or_else(|error| panic!("first generation: {error}"));
    let second =
        agent_sdk_generator(&schema()).unwrap_or_else(|error| panic!("second generation: {error}"));
    assert_eq!(first, second);
    assert_eq!(first.files.len(), 6);
    let typescript = first
        .files
        .get(Path::new("typescript/src/generated/client.ts"))
        .unwrap_or_else(|| panic!("TypeScript output missing"));
    assert!(typescript.contains("export type Amount = bigint"));
    assert!(!typescript.contains("export type Amount = number"));
    assert!(typescript.contains("APPROVAL_CONTRACT_INTRODUCED = \"1.1\""));
    for operation in [
        "approval.list",
        "approval.get",
        "approval.approve",
        "approval.reject",
        "program.discover",
        "program.interface",
        "program.simulate",
        "program.call",
        "program.receipt",
        "program.activity",
    ] {
        assert!(typescript.contains(operation));
    }
    for event in ["Created", "Granted", "Rejected", "Expired", "Defective"] {
        assert!(typescript.contains(event));
    }
    for outcome in ["AlreadyDecided", "Conflict"] {
        assert!(typescript.contains(outcome));
    }
    let python = first
        .files
        .get(Path::new("python/layerx_sdk/generated/client.py"))
        .unwrap_or_else(|| panic!("Python output missing"));
    assert!(python.contains("Amount = int"));
    assert!(!python.contains("Amount = float"));
    assert!(python.contains("class SubmissionUnknown"));
    assert!(python.contains("def require_verified"));
    assert!(python.contains("class ApprovalLifecycleEvent"));
    assert!(python.contains("def approval_approve"));
    assert!(python.contains("program.discover"));
    assert!(python.contains("program.interface"));
    assert!(python.contains("program.simulate"));
    assert!(python.contains("program.call"));
    assert!(python.contains("program.receipt"));
    assert!(python.contains("program.activity"));
    let python_stub = first
        .files
        .get(Path::new("python/layerx_sdk/generated/client.pyi"))
        .unwrap_or_else(|| panic!("Python type stub missing"));
    assert!(python_stub.contains("Amount: TypeAlias = int"));
    let compatibility = first
        .files
        .get(Path::new("COMPATIBILITY.md"))
        .unwrap_or_else(|| panic!("SDK compatibility matrix missing"));
    assert!(compatibility.contains("| `0.1.x` | `1.x` | `1.x` | `0.1.x` | supported |"));
    assert!(compatibility.contains("Bypassing the daemon bypasses them"));
    assert!(compatibility.contains("layerx-proof --example offline_verify"));
    assert!(compatibility.contains("approval module was introduced additively in contract `1.1`"));
    for output in [
        "typescript/src/generated/guarantees.md",
        "python/layerx_sdk/generated/guarantees.md",
    ] {
        let guarantees = first
            .files
            .get(&PathBuf::from(output))
            .unwrap_or_else(|| panic!("missing generated guarantees {output}"));
        assert!(guarantees.contains("`ApprovalHold` | `daemon_enforced`"));
        assert!(guarantees.contains("confers no protocol authority"));
    }
}

#[test]
fn programs_golden_is_generated_from_the_canonical_schema() {
    let root = directory("programs-golden");
    copy_schema(&root);
    write_programs_golden(&root).unwrap_or_else(|error| panic!("write Programs golden: {error}"));
    programs_golden_drift_gate(&root)
        .unwrap_or_else(|error| panic!("Programs golden should match: {error}"));

    fs::write(
        root.join("golden/programs.kvx"),
        "[module]\nname = \"stale\"\n",
    )
    .unwrap_or_else(|error| panic!("replace Programs golden: {error}"));
    assert!(programs_golden_drift_gate(&root).is_err());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn drift_gate_detects_a_hand_edit() {
    let root = directory("drift");
    let generated =
        agent_sdk_generator(&schema()).unwrap_or_else(|error| panic!("generation: {error}"));
    write(&generated, &root);
    assert_eq!(agent_sdk_drift_gate(&generated, &root), Ok(()));
    let path = root.join("typescript/src/generated/client.ts");
    let mut edited =
        fs::read_to_string(&path).unwrap_or_else(|error| panic!("read generated output: {error}"));
    edited.push_str("// hand edit\n");
    fs::write(&path, edited).unwrap_or_else(|error| panic!("edit generated output: {error}"));
    let error = match agent_sdk_drift_gate(&generated, &root) {
        Ok(()) => panic!("hand edit passed drift gate"),
        Err(error) => error,
    };
    assert!(error.contains("generated SDK drift"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn drift_gate_detects_a_python_hand_edit() {
    let root = directory("python-drift");
    let generated =
        agent_sdk_generator(&schema()).unwrap_or_else(|error| panic!("generation: {error}"));
    write(&generated, &root);
    let path = root.join("python/layerx_sdk/generated/client.py");
    let mut edited =
        fs::read_to_string(&path).unwrap_or_else(|error| panic!("read generated output: {error}"));
    edited.push_str("# hand edit\n");
    fs::write(&path, edited).unwrap_or_else(|error| panic!("edit generated output: {error}"));
    let error = match agent_sdk_drift_gate(&generated, &root) {
        Ok(()) => panic!("Python hand edit passed drift gate"),
        Err(error) => error,
    };
    assert!(error.contains("python/layerx_sdk/generated/client.py"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn lossy_consensus_integer_mapping_is_rejected() {
    let root = directory("lossy");
    copy_schema(&root);
    let schema_path = root.join("v1.kvx");
    let source = fs::read_to_string(&schema_path)
        .unwrap_or_else(|error| panic!("read copied schema: {error}"));
    let lossy = source.replacen("typescript = \"bigint\"", "typescript = \"number\"", 1);
    fs::write(&schema_path, lossy).unwrap_or_else(|error| panic!("write copied schema: {error}"));
    let Err(error) = agent_sdk_generator(&root) else {
        panic!("lossy schema passed generation");
    };
    assert!(error.contains("lossy language boundary"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn modified_approval_schema_fixture_trips_the_drift_gate() {
    let output = directory("approval-baseline");
    let fixture = directory("approval-schema");
    let baseline =
        agent_sdk_generator(&schema()).unwrap_or_else(|error| panic!("generation: {error}"));
    write(&baseline, &output);
    copy_schema(&fixture);
    let approval_path = fixture.join("approval.kvx");
    let source = fs::read_to_string(&approval_path)
        .unwrap_or_else(|error| panic!("read approval fixture: {error}"));
    let modified = source.replace(
        "variants = [\"Created\",\"Granted\",\"Rejected\",\"Expired\",\"Defective\"]",
        "variants = [\"Created\",\"Granted\",\"Rejected\",\"Expired\",\"Defective\",\"Escalated\"]",
    );
    assert_ne!(source, modified, "approval fixture was not modified");
    fs::write(&approval_path, modified)
        .unwrap_or_else(|error| panic!("write approval fixture: {error}"));
    let regenerated = agent_sdk_generator(&fixture)
        .unwrap_or_else(|error| panic!("modified generation: {error}"));
    assert_ne!(baseline, regenerated);
    let error = agent_sdk_drift_gate(&regenerated, &output)
        .expect_err("modified approval schema passed committed-output drift gate");
    assert!(error.contains("generated SDK drift"));
    let _ = fs::remove_dir_all(output);
    let _ = fs::remove_dir_all(fixture);
}

#[test]
fn generated_docs_keep_daemon_guarantees_honest() {
    let generated =
        agent_sdk_generator(&schema()).unwrap_or_else(|error| panic!("generation: {error}"));
    let documentation = generated
        .files
        .get(Path::new("typescript/src/generated/guarantees.md"))
        .unwrap_or_else(|| panic!("guarantee documentation missing"));
    assert!(documentation.contains("`ProtocolBudget` | `protocol_enforced`"));
    assert!(documentation.contains("`DaemonLimit` | `daemon_enforced`"));
    assert!(documentation.contains("Bypassing the daemon bypasses this limit"));
    assert!(documentation.contains("`ApprovalHold` | `daemon_enforced`"));
    assert!(documentation.contains("confers no protocol authority"));
    assert!(!documentation.contains("DaemonLimit` | `protocol_enforced`"));
    assert!(!documentation.contains("ApprovalHold` | `protocol_enforced`"));
}

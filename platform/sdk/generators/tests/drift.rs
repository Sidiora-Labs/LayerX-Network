#[allow(dead_code)]
#[path = "../src/main.rs"]
mod pipeline;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use pipeline::{capture, check, parse_lock, render, write_lock};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

fn directory(label: &str) -> PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-platform-sdkgen-{label}-{}-{sequence}",
        std::process::id()
    ))
}

fn place(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    let parent = path
        .parent()
        .unwrap_or_else(|| panic!("no parent for {relative}"));
    fs::create_dir_all(parent).unwrap_or_else(|error| panic!("create {relative}: {error}"));
    fs::write(&path, contents).unwrap_or_else(|error| panic!("write {relative}: {error}"));
}

fn repo_fixture(label: &str) -> PathBuf {
    let root = directory(label);
    place(
        &root,
        "platform/sdk/generators/receipt.kvx",
        "[receipt]\nprogram_outcome = \"optional\"\nprograms_module_id = 9\nprogram_outcome_tags = [\"50524731\", \"50524732\", \"50524733\"]\nrequired_nonzero = [\"global-sequence\", \"module-id\", \"module-version\", \"timestamp\", \"activity-id\", \"resulting-state-root\"]\nfailure_checks = [\"decode\", \"canonical-encoding\", \"receipt-shape\", \"missing-signature\", \"protocol-version\", \"result-code\", \"operation\", \"activity-id\", \"global-sequence\", \"module-id\", \"module-version\", \"timestamp\", \"batch-id\", \"asset\", \"previous-state-root\", \"resulting-state-root\", \"debit-balance\", \"credit-balance\", \"program-outcome\", \"sequencer-signature\"]\n",
    );
    place(&root, "agent/schema/agent-api/v1.kvx", "[schema]\nincludes = [\"errors.kvx\",\"approval.kvx\",\"stream.kvx\",\"programs.kvx\"]\n\n[scalar.Amount]\nrust = \"u128\"\n");
    place(&root, "agent/schema/agent-api/errors.kvx", "[operation.agent.register]\nrequest = \"Register\"\nresponse = \"Registered\"\n\n[mutation.agent.register]\nenvelope = \"IdempotentMutation\"\n\n[type.ErrorClass]\nvariants = [\"TransportFailure\"]\n\n[type.Retriability]\nvariants = [\"Terminal\"]\n");
    place(
        &root,
        "agent/schema/agent-api/approval.kvx",
        "[type.ApprovalLifecycleEvent]\nvariants = [\"Created\"]\n\n[type.ApprovalState]\nvariants = [\"Held\"]\n\n[type.ApprovalDecisionOutcome]\nvariants = [\"Granted\"]\n",
    );
    place(
        &root,
        "agent/schema/agent-api/stream.kvx",
        "[type.Delivery]\nvariants = [\"Event\"]\n",
    );
    place(
        &root,
        "agent/schema/agent-api/programs.kvx",
        "[operation.program.discover]\nrequest = \"ProgramSelector\"\nresponse = \"VerifiedProgramDiscovery\"\n\n[operation.program.interface]\nrequest = \"ProgramSelector\"\nresponse = \"VerifiedProgramInterface\"\n\n[operation.program.simulate]\nrequest = \"ProgramCallRequest\"\nresponse = \"ProgramSimulation\"\n\n[operation.program.call]\nrequest = \"ProgramCallRequest\"\nrequired = [\"idempotency_key\"]\nresponse = \"ProgramSubmission\"\n\n[operation.program.receipt]\nrequest = \"ProgramReceiptSelector\"\nresponse = \"ProgramSubmission\"\n\n[operation.program.activity]\nrequest = \"ProgramActivitySelector\"\nresponse = \"ProgramSubmission\"\n",
    );
    place(
        &root,
        "agent/schema/agent-api/golden/version-request.hex",
        "00ff\n",
    );
    place(&root, "human/schema/human-api/v1.kvx", "[schema]\nincludes = [\"errors.kvx\",\"journeys.kvx\",\"stream.kvx\"]\n\n[operation.version]\nmethod = \"GET\"\npath = \"/v1/version\"\nrequest = \"Empty\"\nresponse = \"VersionInfo\"\n");
    place(
        &root,
        "human/schema/human-api/errors.kvx",
        "[type.ErrorCode]\nvariants = [\"unavailable\"]\n\n[type.Retriability]\nvariants = [\"retriable\"]\n",
    );
    place(
        &root,
        "human/schema/human-api/journeys.kvx",
        "[type.JourneyKind]\nvariants = [\"onboarding\"]\n\n[type.JourneyState]\nvariants = [\"processing\"]\n\n[type.VerificationLevel]\nvariants = [\"unverified\"]\n\n[type.ApprovalState]\nvariants = [\"pending\"]\n",
    );
    place(
        &root,
        "human/schema/human-api/stream.kvx",
        "[type.StreamEventKind]\nvariants = [\"journey-progress\"]\n",
    );
    place(
        &root,
        "human/schema/human-api/golden/account.create.request.json",
        "{}\n",
    );
    place(
        &root,
        "agent/sdk/typescript/src/generated/client.ts",
        "export const generated = true;\n",
    );
    place(
        &root,
        "agent/sdk/typescript/src/generated/guarantees.md",
        "guarantees\n",
    );
    place(
        &root,
        "agent/sdk/python/layerx_sdk/generated/client.py",
        "GENERATED = True\n",
    );
    place(
        &root,
        "agent/sdk/python/layerx_sdk/generated/client.pyi",
        "GENERATED: bool\n",
    );
    place(
        &root,
        "agent/sdk/python/layerx_sdk/generated/guarantees.md",
        "guarantees\n",
    );
    place(&root, "agent/sdk/COMPATIBILITY.md", "compatibility\n");
    place(
        &root,
        "human/apps/web/src/api/generated/index.ts",
        "export const humanApi = true;\n",
    );
    place(
        &root,
        "agent/crates/layerx-agent-api/src/operation_generated.rs",
        "// generated Rust operations\n",
    );
    place(
        &root,
        "agent/crates/layerx-sdk/src/mirror_generated.rs",
        "// generated Rust mirror\n",
    );
    place(&root, "platform/sdk/go/generated.go", "package layerx\n");
    place(
        &root,
        "platform/sdk/go/mirror_generated.go",
        "package layerx\n",
    );
    for relative in pipeline::JVM_FILES {
        place(&root, &format!("platform/sdk/jvm/{relative}"), "jvm\n");
    }
    place(
        &root,
        "platform/sdk/conformance/jvm.kvx",
        "[sdk]\nname = \"jvm\"\n",
    );
    place(&root, "platform/sdk/conformance/run-jvm.sh", "#!/bin/sh\n");
    place(&root, "platform/sdk/conformance/mirror-v2.json", "{}\n");
    place(
        &root,
        "platform/sdk/schema/mirror-v2.kvx",
        "[schema]\nversion = 2\n",
    );
    place(
        &root,
        "platform/sdk/swift/Sources/LayerXSDK/Generated/OperationCatalog.swift",
        "// generated Swift\n",
    );
    place(
        &root,
        "platform/sdk/swift/Sources/LayerXSDK/Generated/MirrorSchema.swift",
        "// generated Swift mirror\n",
    );
    place(
        &root,
        "platform/sdk/dotnet/Generated/OperationCatalog.cs",
        "// generated C#\n",
    );
    place(
        &root,
        "platform/sdk/dotnet/Generated/MirrorSchema.cs",
        "// generated C# mirror\n",
    );
    place(
        &root,
        "platform/sdk/conformance/operations.json",
        "{\"schema\":1,\"operations\":[]}\n",
    );
    root
}

fn lock_path(root: &Path) -> PathBuf {
    root.join("platform/sdk/pipeline.kvx")
}

fn generate(root: &Path) {
    write_lock(root, &lock_path(root)).unwrap_or_else(|error| panic!("write lock: {error}"));
}

fn expect_failure(root: &Path, needle: &str) {
    let error = check(root, &lock_path(root))
        .err()
        .unwrap_or_else(|| panic!("drift gate passed but expected failure about {needle}"));
    assert!(
        error.contains(needle),
        "expected failure mentioning {needle}, got: {error}"
    );
}

fn cleanup(root: &Path) {
    fs::remove_dir_all(root).unwrap_or_else(|error| panic!("cleanup: {error}"));
}

#[test]
fn freshly_generated_pipeline_passes_the_gate() {
    let root = repo_fixture("fresh");
    generate(&root);
    check(&root, &lock_path(&root)).unwrap_or_else(|error| panic!("gate failed: {error}"));
    cleanup(&root);
}

#[test]
fn rust_operation_catalogue_is_derived_from_programs_schema() {
    let root = repo_fixture("rust-programs");
    generate(&root);
    let generated =
        fs::read_to_string(root.join("agent/crates/layerx-agent-api/src/operation_generated.rs"))
            .unwrap_or_else(|error| panic!("read generated Rust operation catalogue: {error}"));
    assert!(generated.contains("ProgramDiscover"));
    assert!(generated.contains("ProgramInterface"));
    assert!(generated.contains("ProgramSimulate"));
    assert!(generated.contains("ProgramCall"));
    assert!(generated.contains("ProgramReceipt"));
    assert!(generated.contains("ProgramActivity"));
    let mutation = generated
        .split("pub const fn mutating")
        .nth(1)
        .unwrap_or_else(|| panic!("generated mutation classifier missing"));
    assert!(mutation.contains("Self::ProgramCall"));
    assert!(!mutation.contains("Self::ProgramSimulate"));
    cleanup(&root);
}

#[test]
fn lock_round_trips_through_render_and_parse() {
    let root = repo_fixture("roundtrip");
    generate(&root);
    let live = capture(&root).unwrap_or_else(|error| panic!("capture: {error}"));
    let text = render(&live).unwrap_or_else(|error| panic!("render: {error}"));
    let parsed = parse_lock(&text).unwrap_or_else(|error| panic!("parse: {error}"));
    assert_eq!(parsed, live);
    cleanup(&root);
}

#[test]
fn missing_lock_fails_the_gate() {
    let root = repo_fixture("missing-lock");
    expect_failure(&root, "pipeline lock missing");
    cleanup(&root);
}

#[test]
fn schema_edit_fails_the_gate_as_stale() {
    let root = repo_fixture("stale-schema");
    generate(&root);
    place(
        &root,
        "agent/schema/agent-api/v1.kvx",
        "[schema]\nversion = \"2\"\n",
    );
    expect_failure(&root, "stale generated SDKs: schema agent-api");
    cleanup(&root);
}

#[test]
fn human_schema_edit_fails_the_gate_as_stale() {
    let root = repo_fixture("stale-human-schema");
    generate(&root);
    place(
        &root,
        "human/schema/human-api/golden/account.create.request.json",
        "{\"edited\":true}\n",
    );
    expect_failure(&root, "stale generated SDKs: schema human-api");
    cleanup(&root);
}

#[test]
fn hand_edited_typescript_output_fails_the_gate() {
    let root = repo_fixture("edit-ts");
    generate(&root);
    place(
        &root,
        "agent/sdk/typescript/src/generated/client.ts",
        "export const generated = false;\n",
    );
    expect_failure(&root, "agent/sdk/typescript/src/generated/client.ts");
    cleanup(&root);
}

#[test]
fn hand_edited_python_output_fails_the_gate() {
    let root = repo_fixture("edit-py");
    generate(&root);
    place(
        &root,
        "agent/sdk/python/layerx_sdk/generated/client.py",
        "GENERATED = False\n",
    );
    expect_failure(&root, "agent/sdk/python/layerx_sdk/generated/client.py");
    cleanup(&root);
}

#[test]
fn deleted_generated_file_fails_the_gate() {
    let root = repo_fixture("deleted");
    generate(&root);
    fs::remove_file(root.join("human/apps/web/src/api/generated/index.ts"))
        .unwrap_or_else(|error| panic!("remove: {error}"));
    expect_failure(&root, "human/apps/web/src/api/generated");
    cleanup(&root);
}

#[test]
fn untracked_file_in_a_generated_root_fails_the_gate() {
    let root = repo_fixture("untracked");
    generate(&root);
    place(
        &root,
        "agent/sdk/typescript/src/generated/extra.ts",
        "export const extra = true;\n",
    );
    expect_failure(&root, "untracked file in generated typescript root");
    cleanup(&root);
}

#[test]
fn lock_missing_an_output_fails_the_gate() {
    let root = repo_fixture("tampered-lock");
    generate(&root);
    let path = lock_path(&root);
    let text = fs::read_to_string(&path).unwrap_or_else(|error| panic!("read lock: {error}"));
    let tampered = if text.contains("\"platform-conformance\", ") {
        text.replace("\"platform-conformance\", ", "")
    } else {
        text.replace(", \"platform-conformance\"", "")
    };
    assert_ne!(tampered, text);
    fs::write(&path, tampered).unwrap_or_else(|error| panic!("tamper lock: {error}"));
    expect_failure(&root, "does not match the wired pipeline");
    cleanup(&root);
}

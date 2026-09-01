#[allow(dead_code)]
#[path = "../src/main.rs"]
mod gate;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use gate::workspace_gate;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

fn directory(label: &str) -> PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-platform-gate-{label}-{}-{sequence}",
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

fn manifest() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../workspace.kvx");
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn declared_directories(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| line.trim().strip_prefix("path = \""))
        .filter_map(|value| value.strip_suffix('"'))
        .map(str::to_owned)
        .collect()
}

fn repo_fixture(label: &str) -> PathBuf {
    let root = directory(label);
    let declared = declared_directories(&manifest());
    assert!(
        !declared.is_empty(),
        "committed manifest declares no workspace directory"
    );
    for relative in &declared {
        fs::create_dir_all(root.join(relative))
            .unwrap_or_else(|error| panic!("create {relative}: {error}"));
    }
    place(&root, "platform/Cargo.toml", "[workspace]\n");
    place(&root, "platform/Cargo.lock", "version = 4\n");
    place(&root, "platform/deny.toml", "[bans]\n");
    place(&root, "platform/clippy.toml", "msrv = \"1.91.1\"\n");
    place(
        &root,
        "platform/tools/dependency-policy.sh",
        "#!/bin/sh\nexit 0\n",
    );
    root
}

fn cleanup(root: &Path) {
    fs::remove_dir_all(root).unwrap_or_else(|error| panic!("cleanup: {error}"));
}

fn expect_refusal(root: &Path, source: &str, needle: &str) {
    let error = workspace_gate(root, source)
        .err()
        .unwrap_or_else(|| panic!("gate passed but expected refusal about {needle}"));
    assert!(
        error.contains(needle),
        "expected refusal mentioning {needle}, got: {error}"
    );
}

#[test]
fn committed_manifest_passes_on_a_complete_tree() {
    let root = repo_fixture("complete");
    workspace_gate(&root, &manifest()).unwrap_or_else(|error| panic!("gate failed: {error}"));
    cleanup(&root);
}

#[test]
fn missing_directory_is_refused() {
    let root = repo_fixture("missing-dir");
    fs::remove_dir_all(root.join("platform/emulator"))
        .unwrap_or_else(|error| panic!("remove: {error}"));
    expect_refusal(&root, &manifest(), "platform/emulator");
    cleanup(&root);
}

#[test]
fn missing_policy_file_is_refused() {
    let root = repo_fixture("missing-policy");
    fs::remove_file(root.join("platform/deny.toml"))
        .unwrap_or_else(|error| panic!("remove: {error}"));
    expect_refusal(&root, &manifest(), "platform/deny.toml");
    cleanup(&root);
}

#[test]
fn missing_lockfile_is_refused() {
    let root = repo_fixture("missing-lockfile");
    fs::remove_file(root.join("platform/Cargo.lock"))
        .unwrap_or_else(|error| panic!("remove: {error}"));
    expect_refusal(&root, &manifest(), "platform/Cargo.lock");
    cleanup(&root);
}

#[test]
fn undeclared_mandated_directory_is_refused() {
    let root = repo_fixture("undeclared");
    let start_marker = "[dir.emulator]\npath = \"platform/emulator\"\n";
    let source = manifest().replace(start_marker, "");
    expect_refusal(
        &root,
        &source,
        "mandated workspace directory undeclared: emulator",
    );
    cleanup(&root);
}

#[test]
fn undeclared_rust_ecosystem_is_refused() {
    let root = repo_fixture("no-rust");
    let source = manifest().replace("[ecosystem.rust]", "[ecosystem.zig]");
    expect_refusal(&root, &source, "rust ecosystem must be declared");
    cleanup(&root);
}

#[test]
fn unknown_section_is_refused() {
    let root = repo_fixture("unknown-section");
    let source = format!("{}\n[surprise]\nkey = \"value\"\n", manifest());
    expect_refusal(&root, &source, "unknown section surprise");
    cleanup(&root);
}

#[test]
fn unknown_declaration_is_refused() {
    let root = repo_fixture("unknown-declaration");
    let source = manifest().replace(
        "[ecosystem.rust]\nmanifest",
        "[ecosystem.rust]\nwarn_only = \"true\"\nmanifest",
    );
    expect_refusal(
        &root,
        &source,
        "unknown declaration ecosystem.rust.warn_only",
    );
    cleanup(&root);
}

#[test]
fn directory_escaping_the_workspace_root_is_refused() {
    let root = repo_fixture("escape");
    let source = manifest().replace("path = \"platform/middleware\"", "path = \"human/crates\"");
    expect_refusal(&root, &source, "escapes the workspace root");
    cleanup(&root);
}

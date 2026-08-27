use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("layerx-boundary-{}-{id}", std::process::id()));
        fs::create_dir_all(path.join("crates/sample/src")).expect("create fixture");
        fs::write(
            path.join("stable-abi-allowlist.toml"),
            "version = 1\npaths = []\n",
        )
        .expect("write allowlist");
        fs::write(
            path.join("unsafe-allowlist.toml"),
            "version = 1\nexceptions = []\n",
        )
        .expect("write unsafe allowlist");
        fs::write(
            path.join("nondeterminism-allowlist.toml"),
            "version = 1\nexceptions = []\n",
        )
        .expect("write nondeterminism allowlist");
        fs::write(
            path.join("crates/sample/Cargo.toml"),
            "[package]\nname = \"sample\"\nversion = \"0.1.0\"\n",
        )
        .expect("write manifest");
        fs::write(
            path.join("crates/sample/src/lib.rs"),
            "pub struct Value(u64);\n",
        )
        .expect("write source");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn run(&self) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_layerx-boundary-check"))
            .arg(self.path())
            .output()
            .expect("run boundary check")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn clean_crate_passes() {
    assert!(Fixture::new().run().status.success());
}

#[test]
fn forbidden_dependency_fails() {
    let fixture = Fixture::new();
    fs::write(
        fixture.path().join("crates/sample/Cargo.toml"),
        "[dependencies]\nrusqlite = \"1\"\n",
    )
    .expect("write violation");
    let output = fixture.run();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("forbidden-dependency"));
}

#[test]
fn private_node_path_fails() {
    let fixture = Fixture::new();
    fs::write(
        fixture.path().join("crates/sample/src/lib.rs"),
        "const DB: &str = \"projection.db\";\n",
    )
    .expect("write violation");
    let output = fixture.run();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("node-private-path"));
}

#[test]
fn mirrored_c_layout_fails() {
    let fixture = Fixture::new();
    fs::write(
        fixture.path().join("crates/sample/src/lib.rs"),
        "#[repr(C)]\npub struct Header { pub sequence: u64 }\n",
    )
    .expect("write violation");
    let output = fixture.run();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("mirrored-c-layout"));
}

#[test]
fn direct_c_binding_fails() {
    let fixture = Fixture::new();
    fs::write(
        fixture.path().join("crates/sample/src/lib.rs"),
        "include!(\"../../include/layerx/lxp_result.h\");\n",
    )
    .expect("write violation");
    let output = fixture.run();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("private-c-boundary"));
}

#[test]
fn unapproved_unsafe_fails() {
    let fixture = Fixture::new();
    fs::write(
        fixture.path().join("crates/sample/src/lib.rs"),
        "pub unsafe fn unchecked() {}\n",
    )
    .expect("write violation");
    let output = fixture.run();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unapproved-unsafe"));
}

#[test]
fn ambient_clock_outside_allowlist_fails() {
    let fixture = Fixture::new();
    fs::write(
        fixture.path().join("crates/sample/src/lib.rs"),
        "pub fn now() -> std::time::SystemTime { std::time::SystemTime::now() }\n",
    )
    .expect("write violation");
    let output = fixture.run();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("ambient-nondeterminism"));
}

#[test]
fn allowlisted_ambient_clock_passes() {
    let fixture = Fixture::new();
    fs::write(
        fixture.path().join("nondeterminism-allowlist.toml"),
        "version = 1\n\n[[exceptions]]\npath = \"crates/sample/src/lib.rs\"\n",
    )
    .expect("write nondeterminism allowlist");
    fs::write(
        fixture.path().join("crates/sample/src/lib.rs"),
        "pub fn now() -> std::time::SystemTime { std::time::SystemTime::now() }\n",
    )
    .expect("write source");
    assert!(fixture.run().status.success());
}

#[test]
fn direct_core_connection_outside_client_fails() {
    let fixture = Fixture::new();
    fs::write(
        fixture.path().join("crates/sample/src/lib.rs"),
        "use layerx_client::lni::transport::Uds;\n",
    )
    .expect("write violation");
    let output = fixture.run();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("direct-core-connection"));
}

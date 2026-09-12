use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct Directory(PathBuf);

impl Directory {
    fn create() -> Result<Self, Box<dyn Error>> {
        let path = std::env::temp_dir().join(format!(
            "explorer-authority-config-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn explorer() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_layerx-explorer-index"));
    command
        .env_clear()
        .env("LAYERX_EXPLORER_PROGRAM_LISTEN", "127.0.0.1:0")
        .env("LAYERX_EXPLORER_PROGRAM_BEARER_TOKEN", "e".repeat(32))
        .env("LAYERX_EXPLORER_NODE_BEARER_TOKEN", "n".repeat(32))
        .env("LAYERX_EXPLORER_AUTHORITY_BEARER_TOKEN", "a".repeat(32))
        .env("LAYERX_EXPLORER_PROGRAM_MAX_STALENESS_MS", "1000")
        .env("LAYERX_EXPLORER_NODE_ENDPOINT", "https://localhost:1")
        .env("LAYERX_EXPLORER_AUTHORITY_ENDPOINT", "https://localhost:2");
    command
}

fn refusal(ca: Option<&Path>, expected: &str) -> Result<(), Box<dyn Error>> {
    let mut command = explorer();
    if let Some(ca) = ca {
        command.env("LAYERX_EXPLORER_AUTHORITY_CA_DER", ca);
    }
    let output = command.output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr)?;
    assert_eq!(stderr.trim(), format!("layerx-explorer-index: {expected}"));
    Ok(())
}

#[test]
fn missing_unreadable_empty_oversized_and_malformed_ca_refuse_startup() -> Result<(), Box<dyn Error>>
{
    let directory = Directory::create()?;
    refusal(None, "LAYERX_EXPLORER_AUTHORITY_CA_DER is required")?;
    refusal(
        Some(&directory.0.join("missing.der")),
        "LAYERX_EXPLORER_AUTHORITY_CA_DER is unreadable",
    )?;
    for (name, bytes, error) in [
        ("empty.der", Vec::new(), "is empty"),
        (
            "oversized.der",
            vec![0; 65_537],
            "exceeds the certificate size limit",
        ),
        (
            "malformed.der",
            b"invalid DER certificate".to_vec(),
            "must contain a DER certificate",
        ),
    ] {
        let path = directory.0.join(name);
        fs::write(&path, bytes)?;
        refusal(
            Some(&path),
            &format!("LAYERX_EXPLORER_AUTHORITY_CA_DER {error}"),
        )?;
    }
    Ok(())
}

#[test]
fn generated_der_ca_is_accepted_and_truncation_is_refused() -> Result<(), Box<dyn Error>> {
    let directory = Directory::create()?;
    let ca = directory.0.join("ca.der");
    let output = Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-subj",
            "/CN=explorer-authority-config",
            "-addext",
            "basicConstraints=critical,CA:TRUE",
            "-addext",
            "keyUsage=critical,keyCertSign,cRLSign",
            "-days",
            "1",
            "-outform",
            "DER",
            "-out",
        ])
        .arg(&ca)
        .arg("-keyout")
        .arg(directory.0.join("key.pem"))
        .output()?;
    assert!(output.status.success(), "CA certificate generation failed");
    refusal(
        Some(&ca),
        "LAYERX_EXPLORER_AUTHORITY_REPLICA_ID is required",
    )?;
    let mut bytes = fs::read(&ca)?;
    assert!(!bytes.is_empty());
    bytes.pop();
    let truncated = directory.0.join("truncated.der");
    fs::write(&truncated, bytes)?;
    refusal(
        Some(&truncated),
        "LAYERX_EXPLORER_AUTHORITY_CA_DER must contain a DER certificate",
    )?;
    Ok(())
}

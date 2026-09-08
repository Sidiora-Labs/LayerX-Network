#![cfg(unix)]

use std::io::Write as _;
use std::process::{Command, Stdio};

#[test]
fn headless_binary_credentials_persist() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let run = |args: &[&str],
               input: Option<&str>|
     -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_layerx"))
            .args(["--json"])
            .args(args)
            .env("LAYERX_CONFIG", temp.path().join("config.json"))
            .env("LAYERX_CREDENTIAL_STORE", "file")
            .env(
                "LAYERX_CREDENTIAL_PASSPHRASE",
                "a long test-only passphrase",
            )
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                "unix:path=/nonexistent-layerx-test-bus",
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        if let Some(input) = input {
            child
                .stdin
                .take()
                .ok_or("missing stdin")?
                .write_all(input.as_bytes())?;
        } else {
            drop(child.stdin.take());
        }
        let output = child.wait_with_output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(serde_json::from_slice(&output.stdout)?)
    };
    run(&["key", "create", "quickstart"], None)?;
    let keys = run(&["key", "list"], None)?;
    assert!(keys.to_string().contains("quickstart"));
    run(&["auth", "set"], Some("initial-test-token"))?;
    assert_eq!(run(&["auth", "status"], None)?["data"]["configured"], true);
    run(&["auth", "set"], Some("rotated-test-token"))?;
    run(&["auth", "delete"], None)?;
    assert_eq!(run(&["auth", "status"], None)?["data"]["configured"], false);
    run(&["key", "delete", "quickstart"], None)?;
    assert!(!run(&["key", "list"], None)?
        .to_string()
        .contains("quickstart"));
    Ok(())
}

#[test]
fn selection_is_explicit_and_passphrase_is_required() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    for selection in [None, Some("unknown"), Some("file")] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_layerx"));
        command
            .args(["--json", "key", "create", "refused"])
            .env("LAYERX_CONFIG", temp.path().join("config.json"))
            .env_remove("LAYERX_CREDENTIAL_STORE")
            .env_remove("LAYERX_CREDENTIAL_PASSPHRASE")
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                "unix:path=/nonexistent-layerx-test-bus",
            );
        if let Some(selection) = selection {
            command.env("LAYERX_CREDENTIAL_STORE", selection);
        }
        let output = command.output()?;
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("LAYERX_CREDENTIAL_STORE=file"));
        assert!(!temp.path().join("config.json").exists());
        assert!(!temp.path().join("credentials/vault").exists());
    }
    Ok(())
}

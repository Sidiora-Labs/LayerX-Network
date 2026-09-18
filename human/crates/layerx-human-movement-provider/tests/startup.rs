use std::process::Command;

#[test]
fn executable_refuses_missing_configuration_without_exposing_environment() -> std::io::Result<()> {
    let result = Command::new(env!("CARGO_BIN_EXE_layerx-human-movement-provider"))
        .env_clear()
        .output()?;
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    assert_eq!(
        result.stderr,
        b"movement provider refused configuration or encountered an integrity/transport failure\n"
    );
    Ok(())
}

#[test]
fn probe_refuses_incomplete_transport_configuration_and_an_absent_listener() -> std::io::Result<()>
{
    let unconfigured = Command::new(env!("CARGO_BIN_EXE_layerx-human-movement-provider"))
        .arg("probe")
        .env_clear()
        .output()?;
    assert_eq!(unconfigured.status.code(), Some(1));
    assert!(unconfigured.stdout.is_empty());
    let absent = Command::new(env!("CARGO_BIN_EXE_layerx-human-movement-provider"))
        .arg("probe")
        .env_clear()
        .env(
            "LAYERX_HUMAN_MOVEMENT_PROVIDER_SOCKET",
            "/var/empty/layerx-human-movement-probe.sock",
        )
        .env("LAYERX_HUMAN_MOVEMENT_PROVIDER_DEADLINE_SECONDS", "2")
        .env("LAYERX_HUMAN_MOVEMENT_PROVIDER_MAX_FRAME_BYTES", "1048576")
        .env("LAYERX_HUMAN_MOVEMENT_PROVIDER_PROTOCOL_VERSION", "2")
        .output()?;
    assert_eq!(absent.status.code(), Some(1));
    assert!(absent.stdout.is_empty());
    Ok(())
}

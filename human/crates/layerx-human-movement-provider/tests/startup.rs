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

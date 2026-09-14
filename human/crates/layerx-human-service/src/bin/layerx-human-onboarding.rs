use std::io::{Read as _, Write as _};

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(_) => {
            eprintln!("Human onboarding sponsor request refused");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let operation = args.next().ok_or("sponsor operation required")?;
    if args.next().is_some() {
        return Err("unexpected sponsor arguments".into());
    }
    let mut input = Vec::new();
    std::io::stdin().take(1_048_577).read_to_end(&mut input)?;
    let output = layerx_human_service::server::production_components::onboarding_sponsor_command(
        &operation, &input,
    )?;
    std::io::stdout().write_all(&output)?;
    std::io::stdout().write_all(b"\n")?;
    Ok(())
}

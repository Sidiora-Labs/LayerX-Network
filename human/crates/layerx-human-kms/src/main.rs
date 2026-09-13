fn main() -> std::process::ExitCode {
    match layerx_client::runtime_clock::RuntimeClock::from_environment()
        .map_err(|error| error.to_string())
        .and_then(|clock| layerx_human_kms::run_from_environment(clock))
    {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Human KMS refused: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

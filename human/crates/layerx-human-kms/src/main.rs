fn main() -> std::process::ExitCode {
    match layerx_human_kms::run_from_environment() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Human KMS refused: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

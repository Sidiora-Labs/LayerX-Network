//! Reference mirror signer daemon.
//!
//! Loads the Ethereum and Solana publisher keys of the `layerx-mirror-signer`
//! secret and answers `interop/deploy/mirror/signer-protocol.md` on an
//! owner-only Unix domain socket the co-located publisher container reaches.

use std::process::ExitCode;

use layerx_mirror_signer::{Options, SignerListener};

fn main() -> ExitCode {
    let options = match Options::parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("layerx-mirror-signer: {error}");
            return ExitCode::FAILURE;
        }
    };
    let listener = match SignerListener::bind(&options) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("layerx-mirror-signer: {error}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!(
        "layerx-mirror-signer: listening on {} for {} (secp256k1) and {} (ed25519)",
        listener.socket().display(),
        options.ethereum_key_handle,
        options.solana_key_handle
    );
    listener.serve()
}

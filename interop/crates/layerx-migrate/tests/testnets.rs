use std::env;
use std::error::Error;
use std::fs;

use layerx_interop_gateway::trace::TraceId;
use layerx_migrate::ethereum::{EthereumConfig, EthereumVerifier};
use layerx_migrate::solana::{SolanaConfig, SolanaVerifier};
use layerx_migrate::{ExternalProvenance, SourceChain, SourceEvidence, SourceVerifier};
use serde::de::DeserializeOwned;

const SECRET_DIRECTORY_MARKER: &str = "${LAYERX_MIGRATION_SECRET_DIR}";

fn load_config<T: DeserializeOwned>(variable: &str) -> Result<T, Box<dyn Error>> {
    let path = env::var(variable)?;
    let secret_directory = env::var("LAYERX_MIGRATION_SECRET_DIR")?;
    let encoded = fs::read_to_string(path)?;
    Ok(serde_json::from_str(
        &encoded.replace(SECRET_DIRECTORY_MARKER, &secret_directory),
    )?)
}

fn load_evidence(variable: &str) -> Result<SourceEvidence, Box<dyn Error>> {
    Ok(SourceEvidence::new(fs::read(env::var(variable)?)?)?)
}

fn trace(entropy: u8) -> TraceId {
    TraceId::mint([entropy; 16])
}

#[test]
#[ignore = "requires the protected Ethereum migration testnet environment"]
fn ethereum_account_mapping_uses_live_quorum_and_wallet_signature() -> Result<(), Box<dyn Error>> {
    let config: EthereumConfig = load_config("LAYERX_ETHEREUM_CONFIG")?;
    let expected_chain = config.chain_id;
    let verifier = EthereumVerifier::new(config)?;
    let evidence = load_evidence("LAYERX_ETHEREUM_OWNERSHIP_EVIDENCE")?;
    let ownership = verifier.verify_ownership(&evidence, &trace(0x31))?;
    assert_eq!(
        ownership.chain(),
        SourceChain::Ethereum {
            chain_id: expected_chain
        }
    );
    assert_eq!(ownership.evidence_digest(), evidence.digest());
    Ok(())
}

#[test]
#[ignore = "requires the protected Ethereum migration testnet environment"]
fn ethereum_asset_claim_uses_live_custody_event_and_finality() -> Result<(), Box<dyn Error>> {
    let config: EthereumConfig = load_config("LAYERX_ETHEREUM_CONFIG")?;
    let expected_chain = config.chain_id;
    let verifier = EthereumVerifier::new(config)?;
    let evidence = load_evidence("LAYERX_ETHEREUM_ASSET_EVIDENCE")?;
    let finality = verifier.verify_asset_finality(&evidence, &trace(0x32))?;
    assert_eq!(
        finality.chain(),
        SourceChain::Ethereum {
            chain_id: expected_chain
        }
    );
    assert_eq!(finality.evidence_digest(), evidence.digest());
    Ok(())
}

#[test]
#[ignore = "requires the protected Ethereum migration testnet environment"]
fn ethereum_history_is_live_external_provenance() -> Result<(), Box<dyn Error>> {
    let verifier = EthereumVerifier::new(load_config("LAYERX_ETHEREUM_CONFIG")?)?;
    let evidence = load_evidence("LAYERX_ETHEREUM_HISTORY_EVIDENCE")?;
    let page = verifier.verify_history(&evidence, &trace(0x33))?;
    assert_eq!(page.evidence_digest(), evidence.digest());
    assert!(page
        .records()
        .iter()
        .all(|record| record.provenance() == ExternalProvenance::Ethereum));
    Ok(())
}

#[test]
#[ignore = "requires the protected Solana migration testnet environment"]
fn solana_account_mapping_uses_live_quorum_and_wallet_signature() -> Result<(), Box<dyn Error>> {
    let config: SolanaConfig = load_config("LAYERX_SOLANA_CONFIG")?;
    let expected_genesis = config.genesis_hash;
    let verifier = SolanaVerifier::new(config)?;
    let evidence = load_evidence("LAYERX_SOLANA_OWNERSHIP_EVIDENCE")?;
    let ownership = verifier.verify_ownership(&evidence, &trace(0x41))?;
    assert_eq!(
        ownership.chain(),
        SourceChain::Solana {
            genesis_hash: expected_genesis,
        }
    );
    assert_eq!(ownership.evidence_digest(), evidence.digest());
    Ok(())
}

#[test]
#[ignore = "requires the protected Solana migration testnet environment"]
fn solana_asset_claim_uses_live_program_and_finality() -> Result<(), Box<dyn Error>> {
    let config: SolanaConfig = load_config("LAYERX_SOLANA_CONFIG")?;
    let expected_genesis = config.genesis_hash;
    let verifier = SolanaVerifier::new(config)?;
    let evidence = load_evidence("LAYERX_SOLANA_ASSET_EVIDENCE")?;
    let finality = verifier.verify_asset_finality(&evidence, &trace(0x42))?;
    assert_eq!(
        finality.chain(),
        SourceChain::Solana {
            genesis_hash: expected_genesis,
        }
    );
    assert_eq!(finality.evidence_digest(), evidence.digest());
    Ok(())
}

#[test]
#[ignore = "requires the protected Solana migration testnet environment"]
fn solana_history_is_live_external_provenance() -> Result<(), Box<dyn Error>> {
    let verifier = SolanaVerifier::new(load_config("LAYERX_SOLANA_CONFIG")?)?;
    let evidence = load_evidence("LAYERX_SOLANA_HISTORY_EVIDENCE")?;
    let page = verifier.verify_history(&evidence, &trace(0x43))?;
    assert_eq!(page.evidence_digest(), evidence.digest());
    assert!(page
        .records()
        .iter()
        .all(|record| record.provenance() == ExternalProvenance::Solana));
    Ok(())
}

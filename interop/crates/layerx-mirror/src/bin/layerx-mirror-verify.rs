#![deny(unsafe_code)]

use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;

use layerx_mirror::ethereum::EthereumMirrorReadConfig;
use layerx_mirror::rpc::RpcQuorumConfig;
use layerx_mirror::solana::SolanaMirrorReadConfig;
use layerx_mirror::source::{
    MirrorLocator, MirrorReadPolicy, MirrorSource, MirrorSourceId, MirrorSources,
};
use layerx_mirror::{ArchiveCommitment, MirrorVerifier, SignedHeaderTrust};
use layerx_proof::merkle::Proof;
use layerx_wire::receipt::decode_merkle_proof;
use serde::Deserialize;
use serde_json::{json, Value};

const MAX_REQUEST_BYTES: u64 = 40 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    layerx_network_id: u32,
    sequencer_id_hex: String,
    sequencer_public_key_hex: String,
    first_batch_number: u64,
    last_batch_number: u64,
    sources: Vec<SourceConfig>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum SourceConfig {
    Ethereum {
        id: String,
        rpc: RpcQuorumConfig,
        chain_id: u64,
        genesis_hash_hex: String,
        archive_contract_hex: String,
        archive_code_hash_hex: String,
        publisher_hex: String,
    },
    Solana {
        id: String,
        rpc: RpcQuorumConfig,
        genesis_hash_hex: String,
        archive_program_hex: String,
        upgradeable_loader_hex: String,
        program_data_account_hex: String,
        program_code_hash_hex: String,
        publisher_hex: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    batch_number: String,
    evidence: EvidenceRequest,
    policy: PolicyRequest,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum EvidenceRequest {
    Receipt {
        canonical_hex: String,
    },
    State {
        canonical_hex: String,
        proof_hex: String,
    },
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum PolicyRequest {
    Exact {
        candidate: Candidate,
    },
    OrderedPreference {
        candidates: Vec<Candidate>,
    },
    Agreement {
        candidates: Vec<Candidate>,
        minimum: usize,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Candidate {
    source: usize,
    commitment_hex: String,
}

fn main() {
    let result = execute();
    let response = match result {
        Ok(value) => json!({ "ok": true, "verification": value }),
        Err(code) => json!({ "ok": false, "error": code }),
    };
    println!("{response}");
}

fn execute() -> Result<Value, &'static str> {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("configuration")?;
    let metadata = fs::metadata(&path).map_err(|_| "configuration")?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err("configuration");
    }
    let config: Config = serde_json::from_slice(&fs::read(path).map_err(|_| "configuration")?)
        .map_err(|_| "configuration")?;
    let mut request_bytes = Vec::new();
    io::stdin()
        .take(MAX_REQUEST_BYTES + 1)
        .read_to_end(&mut request_bytes)
        .map_err(|_| "malformed")?;
    if request_bytes.len() as u64 > MAX_REQUEST_BYTES {
        return Err("bounds");
    }
    let request: Request = serde_json::from_slice(&request_bytes).map_err(|_| "malformed")?;
    let trust = SignedHeaderTrust {
        sequencer_id: fixed_hex(&config.sequencer_id_hex)?,
        sequencer_public_key: fixed_hex(&config.sequencer_public_key_hex)?,
        first_batch_number: config.first_batch_number,
        last_batch_number: config.last_batch_number,
    };
    let mut sources = Vec::with_capacity(config.sources.len());
    for source in config.sources {
        sources.push(match source {
            SourceConfig::Ethereum {
                id,
                rpc,
                chain_id,
                genesis_hash_hex,
                archive_contract_hex,
                archive_code_hash_hex,
                publisher_hex,
            } => MirrorSource::ethereum(
                MirrorSourceId::new(id).map_err(|_| "configuration")?,
                EthereumMirrorReadConfig {
                    rpc,
                    chain_id,
                    genesis_hash: fixed_hex(&genesis_hash_hex)?,
                    archive_contract: fixed_hex(&archive_contract_hex)?,
                    archive_code_hash: fixed_hex(&archive_code_hash_hex)?,
                    publisher: fixed_hex(&publisher_hex)?,
                },
            )
            .map_err(|_| "source-unavailable")?,
            SourceConfig::Solana {
                id,
                rpc,
                genesis_hash_hex,
                archive_program_hex,
                upgradeable_loader_hex,
                program_data_account_hex,
                program_code_hash_hex,
                publisher_hex,
            } => MirrorSource::solana(
                MirrorSourceId::new(id).map_err(|_| "configuration")?,
                SolanaMirrorReadConfig {
                    rpc,
                    genesis_hash: fixed_hex(&genesis_hash_hex)?,
                    archive_program: fixed_hex(&archive_program_hex)?,
                    upgradeable_loader: fixed_hex(&upgradeable_loader_hex)?,
                    program_data_account: fixed_hex(&program_data_account_hex)?,
                    program_code_hash: fixed_hex(&program_code_hash_hex)?,
                    publisher: fixed_hex(&publisher_hex)?,
                },
            )
            .map_err(|_| "source-unavailable")?,
        });
    }
    let sources =
        MirrorSources::new(config.layerx_network_id, sources).map_err(|_| "configuration")?;
    let policy = policy(request.policy)?;
    let batch_number = canonical_u64(&request.batch_number)?;
    let archive = sources
        .read(batch_number, &policy)
        .map_err(|error| match error {
            layerx_mirror::source::MirrorSourceError::Divergent => "divergent",
            layerx_mirror::source::MirrorSourceError::InsufficientAgreement => {
                "insufficient-agreement"
            }
            layerx_mirror::source::MirrorSourceError::Missing => "missing",
            layerx_mirror::source::MirrorSourceError::RateLimited { .. } => "rate-limited",
            layerx_mirror::source::MirrorSourceError::RpcDivergent => "rpc-divergent",
            _ => "source-unavailable",
        })?;
    let verifier = MirrorVerifier::from_source(archive, trust).map_err(|_| "verification")?;
    match request.evidence {
        EvidenceRequest::Receipt { canonical_hex } => {
            let canonical = hex(&canonical_hex)?;
            let verified = verifier.receipt(&canonical).map_err(|_| "verification")?;
            let observation = verified.observation().cloned().ok_or("verification")?;
            let digest = verified
                .value()
                .evidence()
                .receipt_digest()
                .ok_or("verification")?;
            Ok(report(
                observation,
                verified.batch_number(),
                verified.signed_header_digest(),
                digest,
                format!("{:?}", verified.level()),
            ))
        }
        EvidenceRequest::State {
            canonical_hex,
            proof_hex,
        } => {
            let canonical = hex(&canonical_hex)?;
            let encoded = hex(&proof_hex)?;
            let decoded = decode_merkle_proof(&encoded).map_err(|_| "proof")?;
            let proof = Proof::new(
                decoded.leaf_index(),
                decoded.leaf_count(),
                decoded.siblings().to_vec(),
            )
            .map_err(|_| "proof")?;
            let verified = verifier
                .state(&canonical, &proof)
                .map_err(|_| "verification")?;
            let observation = verified.observation().cloned().ok_or("verification")?;
            Ok(report(
                observation,
                verified.batch_number(),
                verified.signed_header_digest(),
                [0; 32],
                format!("{:?}", verified.level()),
            ))
        }
    }
}

fn report(
    observation: layerx_mirror::source::MirrorObservation,
    batch_number: u64,
    header: [u8; 32],
    evidence: [u8; 32],
    level: String,
) -> Value {
    json!({
        "level": level,
        "batchNumber": batch_number.to_string(),
        "headerDigest": encode_hex(&header),
        "evidenceDigest": encode_hex(&evidence),
        "sourceId": observation.source.as_str(),
        "target": target(&observation.target),
        "canonicalPosition": position(observation.position),
        "provenance": format!("{:?}", observation.provenance),
        "latestBatch": observation.freshness.latest_batch.map(|value| value.to_string()),
        "batchLag": format!("{:?}", observation.freshness.batch_lag),
        "failoverCount": observation.failover_count,
        "agreeingSources": observation.agreeing_sources,
        "checkpointLevel": "unavailable"
    })
}

fn policy(value: PolicyRequest) -> Result<MirrorReadPolicy, &'static str> {
    match value {
        PolicyRequest::Exact { candidate } => Ok(MirrorReadPolicy::Exact(locator(candidate)?)),
        PolicyRequest::OrderedPreference { candidates } => Ok(MirrorReadPolicy::OrderedPreference(
            candidates
                .into_iter()
                .map(locator)
                .collect::<Result<_, _>>()?,
        )),
        PolicyRequest::Agreement {
            candidates,
            minimum,
        } => Ok(MirrorReadPolicy::Agreement {
            candidates: candidates
                .into_iter()
                .map(locator)
                .collect::<Result<_, _>>()?,
            minimum,
        }),
    }
}

fn locator(value: Candidate) -> Result<MirrorLocator, &'static str> {
    Ok(MirrorLocator {
        source_index: value.source,
        commitment: ArchiveCommitment::from_bytes(fixed_hex(&value.commitment_hex)?),
    })
}

fn fixed_hex<const N: usize>(value: &str) -> Result<[u8; N], &'static str> {
    hex(value)?.try_into().map_err(|_| "configuration")
}

fn hex(value: &str) -> Result<Vec<u8>, &'static str> {
    let value = value.strip_prefix("0x").unwrap_or(value);
    if value.len() % 2 != 0 {
        return Err("malformed");
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).map_err(|_| "malformed"))
        .collect()
}

fn encode_hex(value: &[u8]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn target(value: &layerx_mirror::source::MirrorTargetIdentity) -> String {
    match value {
        layerx_mirror::source::MirrorTargetIdentity::Ethereum {
            chain_id,
            genesis_hash,
            contract,
            code_hash,
            publisher,
        } => format!(
            "ethereum:chain={chain_id};genesis={};contract={};code={};publisher={}",
            encode_hex(genesis_hash),
            encode_hex(contract),
            encode_hex(code_hash),
            encode_hex(publisher)
        ),
        layerx_mirror::source::MirrorTargetIdentity::Solana {
            genesis_hash,
            program,
            program_data,
            code_hash,
            publisher,
        } => format!(
            "solana:genesis={};program={};program_data={};code={};publisher={}",
            encode_hex(genesis_hash),
            encode_hex(program),
            encode_hex(program_data),
            encode_hex(code_hash),
            encode_hex(publisher)
        ),
    }
}

fn position(value: layerx_mirror::source::MirrorCanonicalPosition) -> String {
    match value {
        layerx_mirror::source::MirrorCanonicalPosition::Ethereum {
            block_number,
            block_hash,
            reference_head_number,
            reference_head_hash,
        } => format!(
            "ethereum:block={block_number};hash={};reference={reference_head_number};reference_hash={}",
            encode_hex(&block_hash),
            encode_hex(&reference_head_hash)
        ),
        layerx_mirror::source::MirrorCanonicalPosition::Solana {
            rooted_slot,
            rooted_blockhash,
        } => format!(
            "solana:slot={rooted_slot};hash={}",
            encode_hex(&rooted_blockhash)
        ),
    }
}

fn canonical_u64(value: &str) -> Result<u64, &'static str> {
    if value.is_empty()
        || value == "0"
        || value.starts_with('0')
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err("malformed");
    }
    value.parse().map_err(|_| "malformed")
}

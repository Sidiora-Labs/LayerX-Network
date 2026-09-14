use std::collections::BTreeMap;
use std::time::Duration;

use layerx_paxeer_verifier::{
    EndpointConfig, EndpointTransport, PaxeerCheckpointPolicy, PaxeerCheckpointVerifier,
};
use layerx_types::intent::EvmAddress;

use super::HistoryError;

const FIELDS: [&str; 12] = [
    "version",
    "url",
    "transport",
    "trust_anchor_der",
    "chain_id",
    "request_timeout_ms",
    "registry",
    "guarantor_bond",
    "protocol_version",
    "network_id",
    "canonical_genesis_root",
    "confirmations",
];

/// Decodes an explicitly trusted finality policy without defaults or ambient configuration.
///
/// # Errors
/// Refuses duplicate or unknown fields, noncanonical values and insecure endpoint policies.
pub fn decode_finality_policy(bytes: &[u8]) -> Result<PaxeerCheckpointPolicy, HistoryError> {
    if bytes.is_empty() || bytes.len() > 1_048_576 || !bytes.is_ascii() {
        return Err(HistoryError::Finality);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| HistoryError::Finality)?;
    let mut fields = BTreeMap::new();
    for line in text.lines() {
        let (key, value) = line.split_once('=').ok_or(HistoryError::Finality)?;
        if !FIELDS.contains(&key)
            || fields.insert(key, value).is_some()
            || value
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        {
            return Err(HistoryError::Finality);
        }
    }
    if fields.len() != FIELDS.len() || fields["version"] != "1" {
        return Err(HistoryError::Finality);
    }
    let transport = match (fields["transport"], fields["trust_anchor_der"]) {
        ("local-emulator", "") => EndpointTransport::LocalEmulator,
        ("pinned-tls", value) if !value.is_empty() => EndpointTransport::PinnedTls {
            trust_anchor_der: hexadecimal(value)?,
        },
        _ => return Err(HistoryError::Finality),
    };
    let timeout = integer(fields["request_timeout_ms"])?;
    if timeout > 60_000 {
        return Err(HistoryError::Finality);
    }
    let policy = PaxeerCheckpointPolicy {
        endpoint: EndpointConfig {
            url: fields["url"].to_owned(),
            request_timeout: Duration::from_millis(timeout),
            transport,
            expected_chain_id: integer(fields["chain_id"])?,
        },
        registry: EvmAddress::new(fixed_hex(fields["registry"])?),
        guarantor_bond: EvmAddress::new(fixed_hex(fields["guarantor_bond"])?),
        protocol_version: integer(fields["protocol_version"])?
            .try_into()
            .map_err(|_| HistoryError::Finality)?,
        network_id: integer(fields["network_id"])?
            .try_into()
            .map_err(|_| HistoryError::Finality)?,
        canonical_genesis_root: fixed_hex(fields["canonical_genesis_root"])?,
        confirmations: integer(fields["confirmations"])?,
    };
    PaxeerCheckpointVerifier::new(policy.clone()).map_err(|_| HistoryError::Finality)?;
    Ok(policy)
}

fn integer(value: &str) -> Result<u64, HistoryError> {
    if value.is_empty()
        || value.starts_with('0')
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(HistoryError::Finality);
    }
    value.parse().map_err(|_| HistoryError::Finality)
}

fn fixed_hex<const N: usize>(value: &str) -> Result<[u8; N], HistoryError> {
    hexadecimal(value)?
        .try_into()
        .map_err(|_| HistoryError::Finality)
}

fn hexadecimal(value: &str) -> Result<Vec<u8>, HistoryError> {
    if !value.len().is_multiple_of(2)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(HistoryError::Finality);
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).map_err(|_| HistoryError::Finality)?;
            u8::from_str_radix(text, 16).map_err(|_| HistoryError::Finality)
        })
        .collect()
}

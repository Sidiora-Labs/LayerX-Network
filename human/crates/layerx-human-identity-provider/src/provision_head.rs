use std::io::{self, Read, Write};

use layerx_proof::inclusion::{verify_receipt, SequencerAuthorization};
use layerx_proof::merkle::Proof;
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    head: Head,
    network_id: u32,
    sequencer_id: String,
    public_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Head {
    current: bool,
    receipt_hex: String,
    receipt_digest: String,
    state_root: String,
    observed_sequence: u64,
    observed_at: u64,
    batch_evidence: Batch,
}

#[derive(Deserialize)]
struct Batch {
    header_hex: String,
    header_signature: String,
    receipt_proof_hex: String,
}

fn refused() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "account-state head refused")
}

fn hex(value: &str) -> io::Result<Vec<u8>> {
    if !value.len().is_multiple_of(2)
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(refused());
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).map_err(|_| refused())?;
            u8::from_str_radix(text, 16).map_err(|_| refused())
        })
        .collect()
}

fn native_proof(bytes: &[u8]) -> io::Result<Proof> {
    let proof = layerx_intents::canonical::decode_merkle_proof(bytes).map_err(|_| refused())?;
    Proof::new(
        proof.leaf_index(),
        proof.leaf_count(),
        proof.siblings().to_vec(),
    )
    .map_err(|_| refused())
}

fn authenticated_digest(
    bytes: &[u8],
    proof: &Proof,
    header: &layerx_intents::canonical::BatchHeader,
    authorization: &SequencerAuthorization,
    observed_at: u64,
) -> io::Result<[u8; 32]> {
    if bytes.starts_with(b"LXP/programs/occupancy-receipt/v2\0") {
        let receipt = layerx_intents::canonical::decode_occupancy_maintenance(bytes)
            .map_err(|_| refused())?;
        if header.first_sequence() == 0
            || header.last_sequence() <= header.first_sequence()
            || header.last_sequence().checked_sub(header.first_sequence())
                != Some(u64::from(proof.leaf_index()))
            || receipt.batch_number != header.batch_number()
            || receipt.global_sequence != header.last_sequence()
            || receipt.resulting_state_root != header.resulting_state_root()
            || proof.leaf_index().checked_add(1) != Some(proof.leaf_count())
            || observed_at != header.timestamp_ms()
        {
            return Err(refused());
        }
        return Ok(Sha256::digest(bytes).into());
    }
    let receipt =
        layerx_proof::receipt::verify_sequencer_signature(bytes, authorization.public_key())
            .map_err(|_| refused())?;
    layerx_intents::canonical::receipt_digest(
        &layerx_intents::canonical::unsigned_receipt_bytes(&receipt).map_err(|_| refused())?,
    )
    .map_err(|_| refused())
}

pub(super) fn run() -> io::Result<()> {
    let mut bytes = Vec::new();
    io::stdin().take(1_048_577).read_to_end(&mut bytes)?;
    if bytes.len() > 1_048_576 {
        return Err(refused());
    }
    let input: Input = serde_json::from_slice(&bytes)?;
    let head = input.head;
    let authorization =
        SequencerAuthorization::from_config(&input.sequencer_id, &input.public_key, "1", "1")
            .map_err(|_| refused())?;
    let signature: [u8; 64] = hex(&head.batch_evidence.header_signature)?
        .try_into()
        .map_err(|_| refused())?;
    let proof = native_proof(&hex(&head.batch_evidence.receipt_proof_hex)?)?;
    let receipt_bytes = hex(&head.receipt_hex)?;
    let verified = verify_receipt(
        &receipt_bytes,
        &proof,
        &hex(&head.batch_evidence.header_hex)?,
        &signature,
        &authorization,
    )
    .map_err(|_| refused())?;
    let header = verified.header().header();
    let digest = authenticated_digest(
        &receipt_bytes,
        &proof,
        header,
        &authorization,
        head.observed_at,
    )?;
    if !head.current
        || header.network_id() != input.network_id
        || header.protocol_version() != 3
        || header.batch_number() != 1
        || header.last_sequence() != head.observed_sequence
        || header.resulting_state_root().as_slice() != hex(&head.state_root)?
        || head.observed_at == 0
        || digest.as_slice() != hex(&head.receipt_digest)?
    {
        return Err(refused());
    }
    io::stdout().write_all(b"{\"consumed\":0}\n")
}

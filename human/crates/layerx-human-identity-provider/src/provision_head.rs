use std::io::{self, Read, Write};

use layerx_proof::inclusion::{verify_receipt, SequencerAuthorization};
use layerx_proof::merkle::decode_proof;
use serde::Deserialize;

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
    let proof =
        decode_proof(&hex(&head.batch_evidence.receipt_proof_hex)?).map_err(|_| refused())?;
    let verified = verify_receipt(
        &hex(&head.receipt_hex)?,
        &proof,
        &hex(&head.batch_evidence.header_hex)?,
        &signature,
        &authorization,
    )
    .map_err(|_| refused())?;
    let header = verified.header().header();
    if !head.current
        || header.network_id() != input.network_id
        || header.protocol_version() != 3
        || header.batch_number() != 1
        || header.last_sequence() != head.observed_sequence
        || header.resulting_state_root().as_slice() != hex(&head.state_root)?
        || head.observed_at == 0
        || hex(&head.receipt_digest)?.len() != 32
    {
        return Err(refused());
    }
    io::stdout().write_all(b"{\"consumed\":0}\n")
}

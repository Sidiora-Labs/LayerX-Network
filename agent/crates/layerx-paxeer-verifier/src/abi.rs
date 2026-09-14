use layerx_proof::checkpoint::Certificate;
use layerx_wire::receipt::BatchHeader;
use sha3::{Digest as _, Keccak256};

pub(crate) const ATTESTATION_TYPE: &str =
    "(uint16,uint32,uint64,address,uint64,bytes32,bytes32,bytes32,uint64,bytes32,bool,bool,uint8,uint64,address,bytes32,bytes32,uint8)[]";

pub(crate) fn word(value: u64) -> [u8; 32] {
    let mut result = [0; 32];
    result[24..].copy_from_slice(&value.to_be_bytes());
    result
}

pub(crate) fn address(value: [u8; 20]) -> [u8; 32] {
    let mut result = [0; 32];
    result[12..].copy_from_slice(&value);
    result
}

pub(crate) fn call(signature: &str, words: &[[u8; 32]]) -> Vec<u8> {
    let mut result = Keccak256::digest(signature.as_bytes())[..4].to_vec();
    for word in words {
        result.extend_from_slice(word);
    }
    result
}

fn attestations(certificate: &Certificate) -> Vec<u8> {
    let mut result = word(certificate.attestations().len() as u64).to_vec();
    for attestation in certificate.attestations() {
        let statement = attestation.canonical_statement();
        let mut offset = 0;
        for width in [2, 4, 8, 20, 8, 32, 32, 32, 8, 32, 1, 1, 1, 8] {
            let mut field = [0; 32];
            field[32 - width..].copy_from_slice(&statement[offset..offset + width]);
            result.extend_from_slice(&field);
            offset += width;
        }
        result.extend_from_slice(&address(attestation.signer()));
        result.extend_from_slice(&attestation.signature());
        result.extend_from_slice(&word(u64::from(attestation.signature_v())));
    }
    result
}

pub(crate) fn registered_certificate(
    certificate: &Certificate,
    header: &BatchHeader,
    identifier: [u8; 32],
) -> Vec<u8> {
    let mut result = call(
        &format!(
            "verifyRegisteredCertificate(bytes32,bytes32,uint64,uint64,bytes32,{ATTESTATION_TYPE})"
        ),
        &[
            identifier,
            header.resulting_state_root(),
            word(header.epoch()),
            word(header.batch_number()),
            header.data_availability_root(),
            word(6 * 32),
        ],
    );
    result.extend_from_slice(&attestations(certificate));
    result
}

pub(crate) fn registration(certificate: &Certificate, header: &BatchHeader) -> Vec<u8> {
    let proof = certificate.checkpoint().validity_proof();
    let mut proof_bytes = word(proof.len() as u64).to_vec();
    proof_bytes.extend_from_slice(proof);
    proof_bytes.resize(proof_bytes.len().div_ceil(32) * 32, 0);
    let mut result = call(
        &format!("registerCheckpoint((uint16,uint32,uint64,uint64,uint64,uint64,bytes32,bytes32,bytes32,bytes32,bytes32,bytes32,bytes32,uint64,bytes32),bytes,{ATTESTATION_TYPE})"),
        &[
            word(u64::from(header.protocol_version())),
            word(u64::from(header.network_id())),
            word(header.epoch()),
            word(header.batch_number()),
            word(header.first_sequence()),
            word(header.last_sequence()),
            header.previous_state_root(),
            header.resulting_state_root(),
            header.activity_merkle_root(),
            header.receipt_merkle_root(),
            header.event_merkle_root(),
            header.data_availability_root(),
            header.oracle_root(),
            word(header.timestamp_ms()),
            header.sequencer_id(),
            word(17 * 32),
            word(17 * 32 + proof_bytes.len() as u64),
        ],
    );
    result.extend_from_slice(&proof_bytes);
    result.extend_from_slice(&attestations(certificate));
    result
}

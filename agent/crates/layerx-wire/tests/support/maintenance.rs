use layerx_wire::receipt::BatchHeader;
use sha2::{Digest as _, Sha256};

pub fn maintenance_bytes(header: &BatchHeader) -> Vec<u8> {
    let mut schedule = 1_u32.to_be_bytes().to_vec();
    for price in 1_u64..=7 {
        schedule.extend_from_slice(&price.to_be_bytes());
    }
    let mut evidence = b"LXP/storage-occupancy-settlement/v3\0".to_vec();
    evidence.extend_from_slice(&header.batch_number().to_be_bytes());
    evidence.extend_from_slice(&schedule);
    evidence.extend_from_slice(&[0; 64]);
    evidence.extend_from_slice(&0_u32.to_be_bytes());
    schedule.extend_from_slice(&[1; 32]);
    let mut preimage = b"LXP/v1/context-hash\0".to_vec();
    preimage.extend_from_slice(&schedule);
    let commitment = Sha256::digest(&preimage);
    let mut bytes = b"LXP/programs/occupancy-receipt/v2\0".to_vec();
    bytes.extend_from_slice(&header.batch_number().to_be_bytes());
    bytes.extend_from_slice(&header.last_sequence().to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&schedule);
    bytes.extend_from_slice(&[0; 64]);
    bytes.extend_from_slice(&0_u16.to_be_bytes());
    bytes.extend_from_slice(&commitment);
    let length = u32::try_from(evidence.len()).unwrap_or_else(|error| panic!("{error:?}"));
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(&evidence);
    bytes.extend_from_slice(&Sha256::digest(&evidence));
    bytes.extend_from_slice(&[1; 32]);
    bytes.extend_from_slice(&[0; 32]);
    bytes.extend_from_slice(&header.resulting_state_root());
    bytes.extend_from_slice(&header.resulting_state_root());
    bytes
}

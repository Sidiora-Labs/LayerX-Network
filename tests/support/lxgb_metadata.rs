use sha2::{Digest, Sha256};

pub fn append(request: &mut Vec<u8>, asset: &[u8; 32], issuer_public: &[u8; 32], salt: &[u8; 32]) {
    let public_hex: String = issuer_public.iter().map(|byte| format!("{byte:02x}")).collect();
    let did = format!("did:layerx:{public_hex}");
    let mut digest = Sha256::new();
    digest.update(b"LXP/v1/did-id\0");
    digest.update((did.len() as u16).to_be_bytes());
    digest.update(did.as_bytes());
    let issuer: [u8; 32] = digest.finalize().into();
    let mut record = 3_u16.to_be_bytes().to_vec();
    record.extend_from_slice(asset);
    record.extend_from_slice(b"\x03TST\x06\x01\x00\x00\x00\x0dCustody token");
    record.extend_from_slice(&0_u128.to_be_bytes());
    record.extend_from_slice(&issuer);
    record.push(2);
    record.extend_from_slice(&0_u128.to_be_bytes());
    record.extend_from_slice(salt);
    request.extend_from_slice(&1_u16.to_be_bytes());
    request.extend_from_slice(&(record.len() as u16).to_be_bytes());
    request.extend_from_slice(&record);
    let mut schedule = 2_u16.to_be_bytes().to_vec();
    for value in [0_u128; 5] {
        schedule.extend_from_slice(&value.to_be_bytes());
    }
    schedule.extend_from_slice(&10000_u32.to_be_bytes());
    schedule.push(8);
    for value in [0_u128; 8] {
        schedule.extend_from_slice(&value.to_be_bytes());
    }
    assert_eq!(schedule.len(), 215);
    request.extend_from_slice(&(schedule.len() as u16).to_be_bytes());
    request.extend_from_slice(&schedule);
}

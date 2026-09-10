use sha2::{Digest, Sha256};

pub fn custody_reference(asset: &[u8; 32]) -> [u8; 32] {
    let mut reference = [0_u8; 32];
    reference[12..].copy_from_slice(&asset[12..]);
    assert!(
        reference.iter().any(|byte| *byte != 0),
        "paxeer custody reference must be non-zero"
    );
    reference
}

pub fn append(request: &mut Vec<u8>, asset: &[u8; 32], issuer_public: &[u8; 32], salt: &[u8; 32]) {
    let public_hex: String = issuer_public.iter().map(|byte| format!("{byte:02x}")).collect();
    let did = format!("did:layerx:{public_hex}");
    let mut digest = Sha256::new();
    digest.update(b"LXP/v1/did-id\0");
    digest.update((did.len() as u16).to_be_bytes());
    digest.update(did.as_bytes());
    let issuer: [u8; 32] = digest.finalize().into();
    let reference = custody_reference(asset);
    let reference_length = u16::try_from(reference.len()).expect("custody reference length");
    let mut record = 3_u16.to_be_bytes().to_vec();
    record.extend_from_slice(asset);
    record.extend_from_slice(b"\x03TST\x06\x02");
    record.extend_from_slice(&reference_length.to_be_bytes());
    record.extend_from_slice(&reference);
    record.extend_from_slice(b"\x00\x0dCustody token");
    record.extend_from_slice(&0_u128.to_be_bytes());
    record.extend_from_slice(&issuer);
    record.push(2);
    record.extend_from_slice(&0_u128.to_be_bytes());
    record.extend_from_slice(salt);
    assert_eq!(record.len(), 186);
    request.extend_from_slice(&1_u16.to_be_bytes());
    request.extend_from_slice(&(record.len() as u16).to_be_bytes());
    request.extend_from_slice(&record);
    let mut schedule = 2_u16.to_be_bytes().to_vec();
    for value in [0_u128; 5] {
        schedule.extend_from_slice(&value.to_be_bytes());
    }
    schedule.extend_from_slice(&10000_u32.to_be_bytes());
    schedule.push(10);
    for value in [0_u128; 10] {
        schedule.extend_from_slice(&value.to_be_bytes());
    }
    assert_eq!(schedule.len(), 247);
    request.extend_from_slice(&(schedule.len() as u16).to_be_bytes());
    request.extend_from_slice(&schedule);
}

#[cfg(test)]
mod tests {
    use super::append;
    use sha2::{Digest, Sha256};

    const VECTOR_LENGTH: usize = 439;
    const VECTOR_SHA256: &str = "38d5d09e3fc241bd6b6f924e4fe656f48954a61055ddec048f1eb636a72c8fe4";

    fn sequence(start: u8) -> [u8; 32] {
        let mut bytes = [0_u8; 32];
        let mut value = start;
        for byte in &mut bytes {
            *byte = value;
            value = value.wrapping_add(1);
        }
        bytes
    }

    #[test]
    fn emits_the_shared_vector() {
        let mut encoded = Vec::new();
        append(&mut encoded, &sequence(0), &sequence(32), &sequence(64));
        assert_eq!(encoded.len(), VECTOR_LENGTH, "lxgb metadata vector length");
        let digest: [u8; 32] = Sha256::digest(&encoded).into();
        let mut expected = [0_u8; 32];
        for (index, byte) in expected.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&VECTOR_SHA256[index * 2..index * 2 + 2], 16)
                .expect("pinned vector digest");
        }
        assert_eq!(
            digest, expected,
            "lxgb metadata emitters disagree; tests/support/lxgb_metadata.py pins the same vector"
        );
    }
}

use layerx_wire::encode::Encoder;

/// # Panics
/// Panics if a fixture field violates its original canonical encoder bound.
#[must_use]
pub fn security_program_receipt(
    activity_id: [u8; 32],
    batch_id: [u8; 32],
    batch_number: u64,
    timestamp: u64,
    resulting_state_root: [u8; 32],
    activity_root: [u8; 32],
    signature: Option<[u8; 64]>,
) -> Vec<u8> {
    let mut encoder = Encoder::new(4_096);
    assert_eq!(encoder.structure_header_version(0x5201, 2), Ok(()));
    assert_eq!(encoder.u16(2), Ok(()));
    assert_eq!(encoder.bytes(&activity_id, 32), Ok(()));
    assert_eq!(encoder.u64(batch_number), Ok(()));
    assert_eq!(encoder.bytes(&[0x11; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&resulting_state_root, 32), Ok(()));
    assert_eq!(encoder.bytes(&activity_root, 32), Ok(()));
    assert_eq!(encoder.i32(0), Ok(()));
    assert_eq!(encoder.sequence_length(0, 512), Ok(()));
    assert_eq!(encoder.u128(0), Ok(()));
    assert_eq!(encoder.bytes(&batch_id, 32), Ok(()));
    assert_eq!(encoder.u16(9), Ok(()));
    assert_eq!(encoder.u32(2), Ok(()));
    assert_eq!(encoder.u32(0), Ok(()));
    assert_eq!(encoder.u8(0), Ok(()));
    assert_eq!(encoder.bytes(&[0; 32], 32), Ok(()));
    assert_eq!(encoder.u128(0), Ok(()));
    assert_eq!(encoder.bytes(&[0; 32], 32), Ok(()));
    assert_eq!(encoder.u128(0), Ok(()));
    assert_eq!(encoder.u128(0), Ok(()));
    assert_eq!(encoder.u64(0), Ok(()));
    assert_eq!(encoder.bytes(&[0; 32], 32), Ok(()));
    assert_eq!(encoder.u128(0), Ok(()));
    assert_eq!(encoder.u128(0), Ok(()));
    assert_eq!(encoder.bytes(&[0; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[0x13; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[0x14; 32], 32), Ok(()));
    assert_eq!(encoder.u64(timestamp), Ok(()));
    assert_eq!(encoder.u8(u8::from(signature.is_some())), Ok(()));
    if let Some(signature) = signature {
        assert_eq!(encoder.bytes(&signature, 64), Ok(()));
    }
    encoder.finish()
}

/// # Panics
/// Panics if a fixture field violates its original canonical encoder bound.
#[must_use]
pub fn security_batch_header(
    batch_number: u64,
    timestamp: u64,
    resulting_state_root: [u8; 32],
    activity_root: [u8; 32],
    receipt_root: [u8; 32],
    sequencer_id: [u8; 32],
    epoch: u64,
) -> Vec<u8> {
    let mut encoder = Encoder::new(354);
    assert_eq!(encoder.structure_header_version(0x1701, 2), Ok(()));
    assert_eq!(encoder.u8(15), Ok(()));
    assert_eq!(encoder.tag(1, 15), Ok(()));
    assert_eq!(encoder.u16(2), Ok(()));
    assert_eq!(encoder.tag(2, 15), Ok(()));
    assert_eq!(encoder.u32(42), Ok(()));
    assert_eq!(encoder.tag(3, 15), Ok(()));
    assert_eq!(encoder.u64(epoch), Ok(()));
    assert_eq!(encoder.tag(4, 15), Ok(()));
    assert_eq!(encoder.u64(batch_number), Ok(()));
    assert_eq!(encoder.tag(5, 15), Ok(()));
    assert_eq!(encoder.u64(batch_number), Ok(()));
    assert_eq!(encoder.tag(6, 15), Ok(()));
    assert_eq!(encoder.u64(batch_number), Ok(()));
    assert_eq!(encoder.tag(7, 15), Ok(()));
    assert_eq!(encoder.bytes(&[0x11; 32], 32), Ok(()));
    assert_eq!(encoder.tag(8, 15), Ok(()));
    assert_eq!(encoder.bytes(&resulting_state_root, 32), Ok(()));
    assert_eq!(encoder.tag(9, 15), Ok(()));
    assert_eq!(encoder.bytes(&activity_root, 32), Ok(()));
    assert_eq!(encoder.tag(10, 15), Ok(()));
    assert_eq!(encoder.bytes(&receipt_root, 32), Ok(()));
    assert_eq!(encoder.tag(11, 15), Ok(()));
    assert_eq!(encoder.bytes(&[0x15; 32], 32), Ok(()));
    assert_eq!(encoder.tag(12, 15), Ok(()));
    assert_eq!(encoder.bytes(&[0x16; 32], 32), Ok(()));
    assert_eq!(encoder.tag(13, 15), Ok(()));
    assert_eq!(encoder.bytes(&[0x17; 32], 32), Ok(()));
    assert_eq!(encoder.tag(14, 15), Ok(()));
    assert_eq!(encoder.u64(timestamp), Ok(()));
    assert_eq!(encoder.tag(15, 15), Ok(()));
    assert_eq!(encoder.bytes(&sequencer_id, 32), Ok(()));
    encoder.finish()
}

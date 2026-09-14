use layerx_wire::encode::Encoder;

/// # Panics
/// Panics if a fixture field violates its original canonical encoder bound.
#[must_use]
pub fn withdrawal_receipt(
    protocol_version: u16,
    asset: [u8; 32],
    amount: u128,
    vault_balance: u128,
    signature: Option<[u8; 64]>,
) -> Vec<u8> {
    let mut encoder = Encoder::new(4_096);
    assert_eq!(
        encoder.structure_header_version(0x5201, protocol_version),
        Ok(())
    );
    assert_eq!(encoder.u16(protocol_version), Ok(()));
    assert_eq!(encoder.bytes(&[0x31; 32], 32), Ok(()));
    assert_eq!(encoder.u64(1), Ok(()));
    assert_eq!(encoder.bytes(&[0x41; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[0x42; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[0x44; 32], 32), Ok(()));
    assert_eq!(encoder.i32(0), Ok(()));
    assert_eq!(encoder.sequence_length(0, 512), Ok(()));
    assert_eq!(encoder.u128(1), Ok(()));
    assert_eq!(encoder.bytes(&[0x43; 32], 32), Ok(()));
    assert_eq!(encoder.u16(8), Ok(()));
    assert_eq!(encoder.u32(2), Ok(()));
    assert_eq!(encoder.u32(1), Ok(()));
    assert_eq!(encoder.u8(1), Ok(()));
    assert_eq!(encoder.bytes(&asset, 32), Ok(()));
    assert_eq!(encoder.u128(amount), Ok(()));
    assert_eq!(encoder.bytes(&[0x33; 32], 32), Ok(()));
    assert_eq!(encoder.u128(vault_balance), Ok(()));
    assert_eq!(encoder.u128(vault_balance - amount), Ok(()));
    assert_eq!(encoder.u64(1), Ok(()));
    assert_eq!(encoder.bytes(&[0x34; 32], 32), Ok(()));
    assert_eq!(encoder.u128(0), Ok(()));
    assert_eq!(encoder.u128(amount), Ok(()));
    assert_eq!(encoder.bytes(&[0x45; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[0x46; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[0x47; 32], 32), Ok(()));
    assert_eq!(encoder.u64(1_000), Ok(()));
    assert_eq!(encoder.u8(u8::from(signature.is_some())), Ok(()));
    if let Some(signature) = signature {
        assert_eq!(encoder.bytes(&signature, 64), Ok(()));
    }
    encoder.finish()
}

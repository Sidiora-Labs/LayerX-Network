use layerx_wire::encode::Encoder;

/// # Panics
/// Panics if a record exceeds the original canonical availability bound.
#[must_use]
pub fn small_availability_record(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = Encoder::new(1024);
    assert_eq!(encoder.sequence_length(1, 65_535), Ok(()));
    assert_eq!(encoder.bytes(bytes, 1024), Ok(()));
    encoder.finish()
}

/// # Panics
/// Panics if a record exceeds the original canonical availability bound.
#[must_use]
pub fn availability_record(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = Encoder::new(1_048_576);
    assert_eq!(encoder.sequence_length(1, 65_535), Ok(()));
    assert_eq!(encoder.bytes(bytes, 1_048_576), Ok(()));
    encoder.finish()
}

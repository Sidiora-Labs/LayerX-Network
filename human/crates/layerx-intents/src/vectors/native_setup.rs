use layerx_wire::encode::Encoder;
use layerx_wire::WireError;

/// # Errors
/// Refuses any field exceeding the original native Budget vector bounds.
pub fn native_budget_create(
    version: u16,
    budget_account: [u8; 32],
    source_account: [u8; 32],
) -> Result<Vec<u8>, WireError> {
    let mut encoded = Encoder::new(251);
    encoded.u16(version)?;
    encoded.fixed(&[8; 32])?;
    encoded.fixed(&budget_account)?;
    encoded.fixed(&[3; 32])?;
    encoded.fixed(&[9; 32])?;
    for amount in [100, 0, 25] {
        encoded.u128(amount)?;
    }
    for number in [100, 1000, 2000, 3] {
        encoded.u64(number)?;
    }
    encoded.u8(1)?;
    if version == 2 {
        encoded.fixed(&source_account)?;
        encoded.u64(2)?;
    }
    Ok(encoded.finish())
}

/// # Errors
/// Refuses any field exceeding the original native recovery vector bounds.
pub fn native_recovery_policy(extended: bool, did: [u8; 32]) -> Result<Vec<u8>, WireError> {
    let mut encoded = Encoder::new(86);
    encoded.fixed(&[0x71, 3, 0, if extended { 5 } else { 3 }])?;
    encoded.fixed(&did)?;
    encoded.fixed(&[7; 32])?;
    encoded.u16(2)?;
    if extended {
        encoded.u64(100)?;
        encoded.u64(500)?;
    }
    Ok(encoded.finish())
}

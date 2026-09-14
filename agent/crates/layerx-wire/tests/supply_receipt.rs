use layerx_wire::receipt::{decode, encode, encode_unsigned};

const PAUSE: &[u8] = include_bytes!("../../../../tests/fixtures/asset/supply-pause.receipt");
const UNPAUSE: &[u8] = include_bytes!("../../../../tests/fixtures/asset/supply-unpause.receipt");

#[test]
fn canonical_native_pause_and_unpause_supply_fields_roundtrip() -> Result<(), layerx_wire::WireError>
{
    for (bytes, operation) in [(PAUSE, 2), (UNPAUSE, 3)] {
        let receipt = decode(bytes)?;
        let Some(fields) = receipt.protocol() else {
            panic!("protocol receipt required");
        };
        assert_eq!(fields.operation(), operation);
        assert_eq!(fields.total_units(), Some((1_000_000, 1_000_000)));
        assert_eq!(fields.amount(), 0);
        assert_eq!(encode(&receipt)?, bytes);
        assert_eq!(
            encode_unsigned(&receipt)?,
            [&bytes[..bytes.len() - 69], &[0]].concat()
        );
        let supply = bytes.len() - 69 - 32;
        let mut changed = bytes.to_vec();
        changed[supply + 31] ^= 1;
        assert!(decode(&changed).is_err());
        for length in 0..bytes.len() {
            assert!(decode(&bytes[..length]).is_err());
        }
        let mut trailing = bytes.to_vec();
        trailing.push(0);
        assert!(decode(&trailing).is_err());
        let prefix = &bytes[..supply];
        let marker = [0, 1, 0, 0, 0, 1, 0, 0, 0, 1, operation];
        let Some(offset) = prefix.windows(marker.len()).position(|part| part == marker) else {
            panic!("native module and operation fields required");
        };
        for unsupported in [0, 9, 12, u8::MAX] {
            changed = bytes.to_vec();
            changed[offset + 10] = unsupported;
            assert!(decode(&changed).is_err());
        }
        changed = bytes.to_vec();
        changed[offset + 1] = 9;
        assert!(decode(&changed).is_err());
    }
    Ok(())
}

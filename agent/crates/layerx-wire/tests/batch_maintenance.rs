use layerx_wire::batch_maintenance::{
    decode_batch_maintenance, decode_maintenance, validate_effects, MaintenanceReceipt, DOMAIN,
    EFFECTS_DOMAIN, MAX_BYTES,
};
use layerx_wire::encode::Encoder;
use layerx_wire::maintenance::decode_occupancy_maintenance;
use layerx_wire::receipt::{decode_batch_header, BatchHeader};
use sha2::{Digest as _, Sha256};

#[path = "support/maintenance.rs"]
mod maintenance;

fn must<T, E: core::fmt::Debug>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|error| panic!("{error:?}"))
}

fn header() -> BatchHeader {
    let document = include_str!(
        "../../../../platform/hosted/authority/tests/fixtures/real-program-deploy-receipt.json"
    );
    let (_, field) = document
        .split_once("\"header_hex\": \"")
        .unwrap_or_else(|| panic!("header field"));
    let (encoded, _) = field
        .split_once('"')
        .unwrap_or_else(|| panic!("header value"));
    let bytes: Vec<_> = encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| must(u8::from_str_radix(must(core::str::from_utf8(pair)), 16)))
        .collect();
    must(decode_batch_header(&bytes))
}

fn effects(modules: &[u16], kind: u8, monetary: bool) -> Vec<u8> {
    let mut encoded = Encoder::new(MAX_BYTES);
    must(encoded.fixed(EFFECTS_DOMAIN));
    must(encoded.u16(must(u16::try_from(modules.len()))));
    for module in modules {
        must(encoded.u16(*module));
        must(encoded.u32(1));
        must(encoded.u16(1));
        must(encoded.u16(0));
        must(encoded.u16(if kind == 1 { 0 } else { 7 }));
        must(encoded.u8(kind));
        must(encoded.u8(u8::from(monetary)));
        must(encoded.fixed(&[u8::from(kind == 2); 32]));
        if kind == 1 {
            must(encoded.u16(36));
            must(encoded.u16(1));
            must(encoded.u8(b'k'));
            must(encoded.u8(1));
            must(encoded.fixed(&Sha256::digest([])));
        } else {
            must(encoded.u16(0));
        }
    }
    encoded.finish()
}

fn envelope(header: &BatchHeader, effects: &[u8]) -> Vec<u8> {
    let mut encoded = Encoder::new(MAX_BYTES);
    must(encoded.fixed(DOMAIN));
    must(encoded.u16(header.protocol_version()));
    must(encoded.u64(header.epoch()));
    must(encoded.u64(header.batch_number()));
    must(encoded.u64(header.timestamp_ms()));
    must(encoded.u64(header.last_sequence()));
    must(encoded.u32(1));
    must(encoded.bytes(&maintenance::maintenance_bytes(header), MAX_BYTES));
    must(encoded.bytes(effects, MAX_BYTES));
    encoded.finish()
}

#[test]
fn formats_are_distinct_and_every_envelope_boundary_is_checked() {
    let header = header();
    let legacy = maintenance::maintenance_bytes(&header);
    assert!(matches!(
        must(decode_maintenance(&legacy)),
        MaintenanceReceipt::Occupancy(_)
    ));
    assert!(decode_batch_maintenance(&legacy).is_err());
    let bytes = envelope(&header, &effects(&[], 1, false));
    assert!(decode_occupancy_maintenance(&bytes).is_err());
    let record = must(decode_maintenance(&bytes));
    assert!(matches!(record, MaintenanceReceipt::Batch(_)));
    assert_eq!(
        record.occupancy(),
        &must(decode_occupancy_maintenance(&legacy))
    );
    must(record.verify_header(&header));
    for length in 0..bytes.len() {
        assert!(
            decode_batch_maintenance(&bytes[..length]).is_err(),
            "length {length}"
        );
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(decode_batch_maintenance(&trailing).is_err());
    for offset in [
        DOMAIN.len() + 1,
        DOMAIN.len() + 17,
        DOMAIN.len() + 33,
        DOMAIN.len() + 37,
    ] {
        let mut changed = bytes.clone();
        changed[offset] ^= 1;
        assert!(
            decode_batch_maintenance(&changed).is_err(),
            "offset {offset}"
        );
    }
    for offset in [DOMAIN.len() + 9, DOMAIN.len() + 25] {
        let mut changed = bytes.clone();
        changed[offset] ^= 2;
        let record = must(decode_maintenance(&changed));
        assert!(record.verify_header(&header).is_err(), "offset {offset}");
    }
    assert!(decode_batch_maintenance(&vec![0; MAX_BYTES + 1]).is_err());
}

#[test]
fn module_effects_preserve_order_kind_accounting_and_deletion_commitments() {
    assert_eq!(must(validate_effects(&effects(&[2; 512], 1, false))), 512);
    assert!(validate_effects(&effects(&[2; 513], 1, false)).is_err());
    for (kind, monetary) in [(1, false), (2, false), (2, true), (3, false)] {
        let bytes = effects(&[2, 2, 3, 5], kind, monetary);
        assert_eq!(must(validate_effects(&bytes)), 4);
        for length in 0..bytes.len() {
            assert!(validate_effects(&bytes[..length]).is_err());
        }
    }
    for modules in [&[5, 2][..], &[2, 4], &[9], &[0]] {
        assert!(validate_effects(&effects(modules, 1, false)).is_err());
    }
    for (kind, monetary) in [(0, false), (4, false), (1, true), (3, true)] {
        assert!(validate_effects(&effects(&[2], kind, monetary)).is_err());
    }
    let bytes = effects(&[2], 1, false);
    let frame = EFFECTS_DOMAIN.len() + 2;
    let effect = frame + 8;
    for offset in [
        frame + 5,
        frame + 7,
        effect + 1,
        effect + 3,
        effect + 6,
        effect + 38,
        effect + 41,
        effect + 43,
        effect + 44,
    ] {
        let mut changed = bytes.clone();
        changed[offset] ^= 2;
        assert!(validate_effects(&changed).is_err(), "offset {offset}");
    }
    let mut transfer = effects(&[2], 2, true);
    transfer[effect + 6..effect + 38].fill(0);
    assert!(validate_effects(&transfer).is_err());
    assert!(validate_effects(&vec![0; MAX_BYTES + 1]).is_err());
}

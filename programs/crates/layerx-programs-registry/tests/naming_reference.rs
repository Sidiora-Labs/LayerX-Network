use layerx_program_sdk::naming::{
    decode_label, encode_label, occupancy_expiry, occupancy_price, Name, Record, Request,
    MAX_NAME_BYTES, MIN_NAME_BYTES, REFERENCE_MAX_PERIODS, REFERENCE_OCCUPANCY_CEILING,
    REFERENCE_PERIOD_BATCHES, REFERENCE_PERIOD_PRICE,
};
use layerx_program_sdk::{Amount, Principal};
use layerx_programs::{naming::reference_interface, ProgramInterface};
use layerx_programs_runtime::ProgramId;

fn fixtures() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/naming")
}

fn did(byte: u8) -> Principal {
    Principal::new([byte; 32]).unwrap_or_else(|e| panic!("{e}"))
}

fn name(bytes: &[u8]) -> Name<'_> {
    Name::new(bytes).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn real_naming_module_binds_the_published_interface_fixture() {
    let directory = fixtures();
    let wasm = std::fs::read(directory.join("naming.wasm")).unwrap_or_else(|e| panic!("{e}"));
    let program = ProgramId::new([0x55; 32]).unwrap_or_else(|e| panic!("{e}"));
    let interface = reference_interface(&wasm, program).unwrap_or_else(|e| panic!("{e}"));
    let published =
        std::fs::read(directory.join("naming.interface")).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(interface.canonical_encoding(), published);
    assert_eq!(ProgramInterface::decode(&published), Ok(interface));
}

#[test]
fn the_name_grammar_admits_only_normalised_labels() {
    assert!(Name::new(b"layerx").is_ok());
    assert!(Name::new(b"a-b").is_ok());
    assert!(Name::new(b"a-9-z").is_ok());
    assert!(Name::new(&[b'a'; MIN_NAME_BYTES]).is_ok());
    assert!(Name::new(&[b'a'; MAX_NAME_BYTES]).is_ok());

    assert!(Name::new(b"").is_err());
    assert!(Name::new(b"ab").is_err());
    assert!(Name::new(&[b'a'; MAX_NAME_BYTES + 1]).is_err());
    assert!(Name::new(b"-ab").is_err());
    assert!(Name::new(b"ab-").is_err());
    assert!(Name::new(b"---").is_err());
    assert!(Name::new(b"LayerX").is_err());
    assert!(Name::new(b"lay_erx").is_err());
    assert!(Name::new(b"lay.erx").is_err());
    assert!(Name::new("café".as_bytes()).is_err());
    assert!(Name::new("cafe\u{0301}".as_bytes()).is_err());
}

#[test]
fn every_entry_point_round_trips_its_canonical_request() {
    let label = b"layerx-beta";
    let requests = [
        Request::Register {
            name: name(label),
            did: did(0x11),
            periods: REFERENCE_MAX_PERIODS,
        },
        Request::Transfer {
            name: name(label),
            did: did(0x22),
        },
        Request::Renew {
            name: name(label),
            periods: 1,
        },
        Request::Resolve { name: name(label) },
        Request::ReverseResolve { did: did(0x33) },
    ];
    let methods = [
        "register",
        "transfer",
        "renew",
        "resolve",
        "reverse_resolve",
    ];
    for (request, method) in requests.into_iter().zip(methods) {
        let encoded = request.encode().unwrap_or_else(|e| panic!("{method}: {e}"));
        assert_eq!(request.method(), method);
        assert_eq!(&encoded.as_slice()[..3], b"LXN");
        assert_eq!(
            Request::decode(encoded.as_slice()),
            Ok(request),
            "{method} does not round trip"
        );
        let mut truncated = encoded.as_slice().to_vec();
        truncated.pop();
        assert!(Request::decode(&truncated).is_err(), "{method} accepts a short encoding");
        let mut trailing = encoded.as_slice().to_vec();
        trailing.push(0);
        assert!(Request::decode(&trailing).is_err(), "{method} accepts trailing bytes");
    }
}

#[test]
fn the_occupancy_term_is_bounded_and_paid_per_period() {
    assert_eq!(
        occupancy_price(1),
        Ok(Amount::from_u128(REFERENCE_PERIOD_PRICE))
    );
    assert_eq!(
        occupancy_price(REFERENCE_MAX_PERIODS),
        Ok(Amount::from_u128(REFERENCE_OCCUPANCY_CEILING))
    );
    assert!(occupancy_price(0).is_err());
    assert!(occupancy_price(REFERENCE_MAX_PERIODS + 1).is_err());

    assert_eq!(occupancy_expiry(7, 1), Ok(7 + REFERENCE_PERIOD_BATCHES));
    assert!(occupancy_expiry(u64::MAX, 1).is_err());
    assert!(occupancy_expiry(7, 0).is_err());
}

#[test]
fn a_record_and_a_label_survive_their_canonical_encoding() {
    let record = Record {
        did: did(0x44),
        expiry: REFERENCE_PERIOD_BATCHES,
    };
    let encoded = record.encode().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(Record::decode(encoded.as_slice()), Ok(record));
    assert!(Record::decode(&encoded.as_slice()[..39]).is_err());
    assert!(Record::decode(&[0; 40]).is_err());

    let label = encode_label(name(b"layerx-beta")).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(decode_label(label.as_slice()), Ok(name(b"layerx-beta")));
    assert!(decode_label(&label.as_slice()[..3]).is_err());
}

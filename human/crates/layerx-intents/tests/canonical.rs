use std::fmt::Debug;
use std::path::Path;

use layerx_intents::canonical::{
    batch_header_bytes, decode_batch_header, decode_receipt, receipt_bytes, receipt_digest,
    unsigned_receipt_bytes,
};
use layerx_proof::receipt::verify_sequencer_signature;

fn checked<T, E: Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("canonical native evidence: {error:?}"))
}

#[test]
fn original_native_receipts_keep_exact_bytes_and_signature_authority() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/fixtures");
    for (directory, names) in [
        ("asset/daemon-send-supply", &["receipt"][..]),
        ("custody/daemon-credit-receipt", &["credit.receipt"][..]),
        (
            "programs/maintained-multicall",
            &["receipt-0", "receipt-1"][..],
        ),
    ] {
        let path = root.join(directory);
        let key: [u8; 32] =
            checked(checked(std::fs::read(path.join("sequencer.public"))).try_into());
        for name in names {
            let bytes = checked(std::fs::read(path.join(name)));
            let verified = checked(verify_sequencer_signature(&bytes, key));
            let decoded = checked(decode_receipt(&bytes));
            assert_eq!(verified, decoded);
            assert_eq!(checked(receipt_bytes(&decoded)), bytes);
            assert_eq!(
                checked(receipt_digest(&checked(unsigned_receipt_bytes(&decoded)))),
                checked(receipt_digest(&checked(unsigned_receipt_bytes(&verified)))),
            );
            let mut changed = bytes.clone();
            let index = changed.len() - 1;
            changed[index] ^= 1;
            assert!(verify_sequencer_signature(&changed, key).is_err());
            let mut trailing = bytes.clone();
            trailing.push(0);
            assert!(decode_receipt(&trailing).is_err());
            assert!(decode_receipt(&bytes[..bytes.len() - 1]).is_err());
        }
    }
}

#[test]
fn original_native_headers_keep_exact_network_and_root_bindings() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/fixtures");
    for directory in [
        "custody/daemon-credit-receipt",
        "programs/maintained-multicall",
    ] {
        let bytes = checked(std::fs::read(root.join(directory).join("header")));
        let decoded = checked(decode_batch_header(&bytes));
        assert_eq!(checked(batch_header_bytes(&decoded)), bytes);
        assert_eq!(decoded.protocol_version(), 3);
        assert_ne!(decoded.network_id(), 0);
        assert_ne!(decoded.resulting_state_root(), [0; 32]);
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(decode_batch_header(&trailing).is_err());
        assert!(decode_batch_header(&bytes[..bytes.len() - 1]).is_err());
    }
}

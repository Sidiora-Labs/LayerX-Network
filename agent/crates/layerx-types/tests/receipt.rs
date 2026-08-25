use std::fs;
use std::process::Command;

use layerx_types::batch::BATCH_HEADER_FIELDS;
use layerx_types::checkpoint::GUARANTOR_ATTESTATION_FIELDS;
use layerx_types::receipt::{ACTIVITY_RECEIPT_FIELDS, LXP_RECEIPT_FIELDS};

#[test]
fn receipt_and_batch_field_sets_match_protocol() {
    assert_eq!(ACTIVITY_RECEIPT_FIELDS.len(), 15);
    assert_eq!(
        LXP_RECEIPT_FIELDS,
        [
            "protocol_version",
            "transaction_id",
            "operation",
            "global_sequence",
            "asset",
            "amount",
            "from",
            "from_balance_before",
            "from_balance_after",
            "from_sequence",
            "to",
            "to_balance_before",
            "to_balance_after",
            "transfer_set_root",
            "authorization_hash",
            "context_hash",
            "previous_state_root",
            "resulting_state_root",
            "batch_id",
            "timestamp",
            "sequencer_signature",
        ]
    );
    assert_eq!(BATCH_HEADER_FIELDS.len(), 15);
    assert_eq!(BATCH_HEADER_FIELDS[0], "protocol_version");
    assert_eq!(BATCH_HEADER_FIELDS[14], "sequencer_id");
    assert_eq!(GUARANTOR_ATTESTATION_FIELDS.len(), 17);
}

#[test]
fn receipt_has_no_arbitrary_local_constructor() {
    let dependency_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf));
    let Some(dependency_dir) = dependency_dir else {
        panic!("test dependency directory unavailable");
    };
    let rlib = fs::read_dir(&dependency_dir).ok().and_then(|entries| {
        entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name().is_some_and(|name| {
                    name.to_string_lossy().starts_with("liblayerx_types-")
                        && path
                            .extension()
                            .is_some_and(|extension| extension == "rlib")
                })
            })
            .max_by_key(|path| {
                fs::metadata(path)
                    .and_then(|metadata| metadata.modified())
                    .ok()
            })
    });
    let Some(rlib) = rlib else {
        panic!("layerx-types rlib unavailable");
    };
    let source = std::env::temp_dir().join(format!(
        "layerx-receipt-construction-{}.rs",
        std::process::id()
    ));
    let binary = source.with_extension("bin");
    assert!(fs::write(
        &source,
        "extern crate layerx_types;\nuse layerx_types::receipt::LxpReceipt;\nfn main() { let _ = LxpReceipt::new(); }\n",
    )
    .is_ok());
    let output = Command::new("rustc")
        .arg("--edition=2021")
        .arg(&source)
        .arg("--extern")
        .arg(format!("layerx_types={}", rlib.display()))
        .arg("-L")
        .arg(format!("dependency={}", dependency_dir.display()))
        .arg("-o")
        .arg(&binary)
        .output();
    let _ = fs::remove_file(&source);
    let _ = fs::remove_file(&binary);
    let Ok(output) = output else {
        panic!("rustc unavailable for construction boundary test");
    };
    assert!(
        !output.status.success(),
        "receipt became locally constructible"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no function or associated item named `new`"),
        "unexpected compiler refusal: {stderr}"
    );
}

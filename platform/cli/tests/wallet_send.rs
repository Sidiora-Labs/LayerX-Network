use layerx_platform_cli::wallet_send::{Authorization, Send};
use layerx_platform_cli::wallet_signing::SigningFacts;
use std::path::Path;
use std::process::Command;

fn vector() -> Send {
    Send {
        from: [1; 32],
        to: [2; 32],
        asset: [3; 32],
        amount: (1_u128 << 127) + 257,
        source_next_sequence: 0x0102_0304_0506_0708,
        idempotency_key: [4; 32],
        expires_at: 2000,
        context_hash: [5; 32],
        conditions: vec![(1, 1000), (2, 2000)],
        authorization: Authorization {
            kind: 1,
            controller: [1; 32],
            public_key: [6; 32],
            signature: [7; 64],
            signed_context_hash: [5; 32],
            network_id: 402,
            protocol_version: 3,
        },
    }
}

#[test]
fn native_send_and_authorization_bytes_match() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = cli.join("../..");
    let directory = tempfile::tempdir_in(root.join("platform/target"))?;
    let binary = directory.path().join("native-send");
    let output = Command::new("cc")
        .args([
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-ffunction-sections",
            "-fdata-sections",
            "-Wl,--gc-sections",
        ])
        .arg("-I")
        .arg(root.join("include"))
        .arg(cli.join("tests/fixtures/native-send.c"))
        .arg(root.join("src/ledger/lxp_send.c"))
        .arg(root.join("src/protocol/lxp_u128.c"))
        .arg("-o")
        .arg(&binary)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = Command::new(&binary).output()?;
    assert!(output.status.success());
    let send = vector();
    let mut encoded = send.encode()?;
    assert_eq!(&encoded[..4], &[0x53, 1, 0, 10]);
    encoded.extend(send.authorization_message()?);
    assert_eq!(encoded, output.stdout);
    Ok(())
}

#[test]
fn envelope_and_payload_sequences_are_independent() -> Result<(), String> {
    let send = vector();
    let facts = SigningFacts {
        actor: "did:layerx:alice",
        public_key: [6; 32],
        network_id: 402,
        identity_next_sequence: 17,
        not_before_ms: 1000,
        expires_at_ms: 2000,
        fee_limit: 10,
        idempotency_key: [4; 32],
    };
    let prepared = send.prepare(&facts)?;
    let kind = layerx_types::payload::ActivityType::new(layerx_types::payload::ModuleId::Asset, 5)
        .map_err(|e| format!("{e:?}"))?;
    let registry = layerx_types::payload::ModuleRegistry::new(&[
        layerx_types::payload::ModuleRegistration::new(
            layerx_types::payload::ModuleId::Asset,
            &[kind],
        )
        .map_err(|e| format!("{e:?}"))?,
    ])
    .map_err(|e| format!("{e:?}"))?;
    let decoded = layerx_wire::activity::decode_unsigned(prepared.canonical_unsigned(), &registry)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(decoded.account_sequence(), 17);
    assert_eq!(decoded.payload(), send.encode()?);
    let disclosure = prepared.confirmation();
    assert_eq!(disclosure["envelope_sequence"], 17);
    assert_eq!(
        disclosure["payment"]["payload_sequence"],
        send.source_next_sequence.to_string()
    );
    assert_eq!(disclosure["payment"], send.disclosure());
    let mut changed = send.clone();
    changed.source_next_sequence = u64::MAX;
    assert_ne!(send.encode()?, changed.encode()?);
    assert_eq!(
        changed.prepare(&facts)?.confirmation()["envelope_sequence"],
        17
    );
    Ok(())
}

#[test]
fn send_bounds_and_bindings_are_checked() -> Result<(), String> {
    let good = vector();
    let mut bad = good.clone();
    bad.amount = 0;
    assert!(bad.encode().is_err());
    let mut bad = good.clone();
    bad.to = bad.from;
    assert!(bad.encode().is_err());
    let mut bad = good.clone();
    bad.conditions = vec![(1, 0); 9];
    assert!(bad.encode().is_err());
    let mut bad = good.clone();
    bad.conditions = vec![(3, 0)];
    assert!(bad.encode().is_err());
    let mut bad = good.clone();
    bad.authorization.kind = 7;
    assert!(bad.encode().is_err());
    let mut bad = good.clone();
    bad.authorization.controller = [9; 32];
    assert!(bad.encode().is_err());
    let mut bad = good.clone();
    bad.authorization.signed_context_hash = [9; 32];
    assert!(bad.encode().is_err());
    let mut bounds = good;
    bounds.amount = u128::MAX;
    bounds.source_next_sequence = u64::MAX;
    bounds.conditions = vec![(2, u64::MAX); 8];
    assert_eq!(bounds.encode()?.len(), 436);
    Ok(())
}

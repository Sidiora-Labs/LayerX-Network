use std::error::Error;
use std::fs;
use std::path::PathBuf;

use ed25519_dalek::{SigningKey, Verifier as _};
use layerx_explorer_index::reads::{
    build_resolve_read, decode_record, interpret_answer, interpret_sequence,
    is_naming_reference_interface, parse_answer, resolve_calldata, resolved_json, validate_name,
    verify_resolve_answer, ReadAnswer, ReadEndpoint, ReadError, ReadPrincipal, ReadScope,
    ResolveFailure, ResolveOutcome, READINESS_PROBE_NAME,
};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::program_call::NativeProgramCall;
use layerx_wire::activity::{decode_signed, signing_bytes};
use layerx_wire::hash::Domain;
use serde_json::json;
use sha2::{Digest as _, Sha256};

const SEED: [u8; 32] = [7; 32];
const PROGRAM: [u8; 32] = [0x5a; 32];
const SCOPE: ReadScope = ReadScope {
    network_id: 1_280_070_740,
    protocol_version: 3,
    fee_limit: 50_000_000,
};

fn must<T, E: std::fmt::Debug>(result: Result<T, E>, what: &str) -> Result<T, Box<dyn Error>> {
    result.map_err(|error| format!("{what}: {error:?}").into())
}

fn seed_hex() -> String {
    SEED.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn fixture(path: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    Ok(fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../programs/crates/layerx-programs-registry/tests/fixtures")
            .join(path),
    )?)
}

fn registry() -> Result<ModuleRegistry, Box<dyn Error>> {
    let activity_type = must(ActivityType::new(ModuleId::Programs, 3), "activity type")?;
    let registration = must(
        ModuleRegistration::new(ModuleId::Programs, &[activity_type]),
        "module registration",
    )?;
    must(ModuleRegistry::new(&[registration]), "module registry")
}

#[test]
fn resolve_calldata_is_the_naming_programs_wire_form() -> Result<(), Box<dyn Error>> {
    let mut expected = b"LXN\x04\x01\x20".to_vec();
    expected.extend_from_slice(&6_u32.to_be_bytes());
    expected.push(5);
    expected.extend_from_slice(b"alice");
    assert_eq!(resolve_calldata("alice")?, expected);
    Ok(())
}

#[test]
fn names_follow_the_naming_programs_grammar() {
    let longest = "a".repeat(63);
    for name in ["abc", "a-b", "0x9", longest.as_str(), READINESS_PROBE_NAME] {
        assert_eq!(validate_name(name), Ok(()), "{name}");
    }
    let oversized = "a".repeat(64);
    for name in [
        "",
        "ab",
        "-abc",
        "abc-",
        "Abc",
        "a_b",
        "a.b",
        "a%2db",
        "abc&x=1",
        oversized.as_str(),
    ] {
        assert_eq!(validate_name(name), Err(ReadError::InvalidName), "{name}");
        assert_eq!(resolve_calldata(name), Err(ReadError::InvalidName));
    }
}

#[test]
fn the_signed_read_is_the_principals_own_resolve_call() -> Result<(), Box<dyn Error>> {
    let principal = ReadPrincipal::from_seed_hex(&seed_hex())?;
    let verifying = SigningKey::from_bytes(&SEED).verifying_key();
    assert_eq!(principal.public_key(), verifying.to_bytes());
    assert_eq!(
        principal.did(),
        format!(
            "did:layerx:{}",
            layerx_programs::hex::encode(&verifying.to_bytes())
        )
    );
    let now = 1_800_000_000_000;
    let read = build_resolve_read(&principal, SCOPE, PROGRAM, 2, "alice", 1, now)?;
    let activity = must(
        decode_signed(&read.signed_activity, &registry()?),
        "signed read",
    )?;
    assert_eq!(activity.protocol_version(), 3);
    assert_eq!(activity.network_id(), SCOPE.network_id);
    assert_eq!(activity.actor_did(), principal.did().as_bytes());
    assert_eq!(activity.account_sequence(), 1);
    assert_eq!(activity.fee_limit(), SCOPE.fee_limit);
    assert_eq!(
        must(layerx_wire::hash::activity_id(&activity), "activity id")?,
        read.activity_id
    );
    assert_eq!(
        must(layerx_wire::hash::payload_hash(&activity), "payload hash")?,
        read.payload_hash
    );
    let call = must(NativeProgramCall::decode(activity.payload()), "native call")?;
    assert_eq!(call.program_id.bytes(), PROGRAM);
    assert_eq!(call.guest_abi, 2);
    assert_eq!(call.entrypoint, b"resolve");
    assert_eq!(call.calldata, resolve_calldata("alice")?.as_slice());
    let mut preimage = Sha256::new();
    preimage.update(Domain::SignaturePreimage.tag());
    preimage.update(must(signing_bytes(&activity), "signing bytes")?.as_bytes());
    let signature =
        ed25519_dalek::Signature::from_slice(activity.signature().ok_or("read is unsigned")?)?;
    verifying.verify(&preimage.finalize(), &signature)?;
    let again = build_resolve_read(&principal, SCOPE, PROGRAM, 2, "alice", 1, now)?;
    assert_ne!(again.activity_id, read.activity_id);
    let unfunded = build_resolve_read(&principal, SCOPE, PROGRAM, 2, "alice", 0, now)?;
    assert_eq!(
        must(
            decode_signed(&unfunded.signed_activity, &registry()?),
            "signed read",
        )?
        .account_sequence(),
        0
    );
    Ok(())
}

#[test]
fn the_read_sequence_is_the_principals_own_sequence_document() -> Result<(), Box<dyn Error>> {
    let principal = ReadPrincipal::from_seed_hex(&seed_hex())?;
    let document = |did: &str, sequence: serde_json::Value| ReadAnswer {
        status: 200,
        body: json!({"ok": true, "result": {"did": did, "next_sequence": sequence,
            "observed_head_sequence": "9", "state_root": "00".repeat(32),
            "verification": "authenticated_node_snapshot"}, "trace": "core-1"})
        .to_string()
        .into_bytes(),
    };
    assert_eq!(
        interpret_sequence(&principal, &document(principal.did(), json!("1"))),
        Ok(1)
    );
    assert_eq!(
        interpret_sequence(
            &principal,
            &document(principal.did(), json!("18446744073709551615"))
        ),
        Ok(u64::MAX)
    );
    assert_eq!(
        interpret_sequence(&principal, &document("did:layerx:other", json!("1"))),
        Err(ResolveFailure::Unverified(ReadError::Unbound))
    );
    for sequence in [
        json!(1),
        json!("01"),
        json!("+1"),
        json!("-1"),
        json!(""),
        json!("18446744073709551616"),
        json!(null),
    ] {
        assert_eq!(
            interpret_sequence(&principal, &document(principal.did(), sequence)),
            Err(ResolveFailure::Unverified(ReadError::MalformedAnswer))
        );
    }
    let refused = ReadAnswer {
        status: 503,
        body: json!({"error": {"code": "sequence_unavailable", "retry": "after",
            "retry_after_seconds": 5}})
        .to_string()
        .into_bytes(),
    };
    assert_eq!(
        interpret_sequence(&principal, &refused),
        Err(ResolveFailure::Refused {
            status: 503,
            code: "sequence_unavailable".to_owned()
        })
    );
    let unacknowledged = ReadAnswer {
        status: 200,
        body: json!({"ok": false, "result": {"did": principal.did(), "next_sequence": "1"}})
            .to_string()
            .into_bytes(),
    };
    assert_eq!(
        interpret_sequence(&principal, &unacknowledged),
        Err(ResolveFailure::Unverified(ReadError::MalformedAnswer))
    );
    let unframed = ReadAnswer {
        status: 200,
        body: b"not json".to_vec(),
    };
    assert_eq!(
        interpret_sequence(&principal, &unframed),
        Err(ResolveFailure::Unverified(ReadError::MalformedAnswer))
    );
    Ok(())
}

#[test]
fn malformed_read_keys_are_refused() {
    for text in ["", "07", "zz", &"0".repeat(63), &"0".repeat(66)] {
        assert!(matches!(
            ReadPrincipal::from_seed_hex(text),
            Err(ReadError::InvalidKey)
        ));
    }
}

#[test]
fn only_the_naming_reference_interface_is_resolvable() -> Result<(), Box<dyn Error>> {
    let module = fixture("naming/naming.wasm")?;
    let program = must(layerx_programs::ProgramId::new(PROGRAM), "program")?;
    let interface = layerx_programs::naming::reference_interface(&module, program)?;
    assert!(is_naming_reference_interface(PROGRAM, &interface));
    let other = must(layerx_programs::ProgramId::new([0x11; 32]), "program")?;
    let foreign = layerx_programs::naming::reference_interface(&module, other)?;
    assert!(!is_naming_reference_interface(PROGRAM, &foreign));
    let token = layerx_programs::lxt721::reference_interface(&fixture("lxt721/nft-lxt721.wasm")?)?;
    assert!(!is_naming_reference_interface(PROGRAM, &token));
    Ok(())
}

#[test]
fn records_decode_into_the_web_contract() -> Result<(), Box<dyn Error>> {
    let mut body = vec![1, 0x20, 0, 0, 0, 0x28];
    body.extend_from_slice(&[0xab; 32]);
    body.extend_from_slice(&1_900_000_000_000_u64.to_be_bytes());
    let outcome = decode_record(&body)?;
    assert_eq!(
        outcome,
        ResolveOutcome::Resolved {
            did: [0xab; 32],
            expiry: 1_900_000_000_000,
        }
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&resolved_json(
            "alice",
            [0xab; 32],
            1_900_000_000_000
        ))?,
        json!({"name": "alice", "did": "ab".repeat(32), "expiry": "1900000000000"})
    );
    let mut truncated = body.clone();
    truncated.pop();
    let mut extended = body.clone();
    extended.push(0);
    let mut reframed = body.clone();
    reframed[1] = 0x21;
    let mut reserved = body.clone();
    reserved[6..38].fill(0);
    for refused in [&truncated, &extended, &reframed, &reserved, &Vec::new()] {
        assert_eq!(decode_record(refused), Err(ReadError::UnexpectedResponse));
    }
    Ok(())
}

#[test]
fn answers_without_sequencer_evidence_fail_closed() -> Result<(), Box<dyn Error>> {
    let principal = ReadPrincipal::from_seed_hex(&seed_hex())?;
    let read = build_resolve_read(&principal, SCOPE, PROGRAM, 2, "alice", 1, 1_800_000_000_000)?;
    let sequencer = SigningKey::from_bytes(&[9; 32]).verifying_key().to_bytes();
    let hex = layerx_programs::hex::encode;
    assert_eq!(
        verify_resolve_answer(&read, sequencer, &json!({})),
        Err(ReadError::MalformedAnswer)
    );
    let execution = |activity: &[u8; 32], program: &[u8; 32]| {
        json!({
            "state": "read",
            "activity_id": hex(activity),
            "program_id": hex(program),
            "result_code": 0,
            "receipt": hex(&[1, 2, 3]),
            "receipt_kind": "hypothetical",
            "terminal_payload": "",
            "call_graph": "",
        })
    };
    let committed = json!({"result": {"committed": true, "read_only": true,
        "execution": execution(&read.activity_id, &PROGRAM)}});
    assert_eq!(
        verify_resolve_answer(&read, sequencer, &committed),
        Err(ReadError::MalformedAnswer)
    );
    for foreign in [
        execution(&[3; 32], &PROGRAM),
        execution(&read.activity_id, &[3; 32]),
    ] {
        let answer =
            json!({"result": {"committed": false, "read_only": true, "execution": foreign}});
        assert_eq!(
            verify_resolve_answer(&read, sequencer, &answer),
            Err(ReadError::Unbound)
        );
    }
    let forged = json!({"ok": true, "result": {"committed": false, "read_only": true,
        "execution": execution(&read.activity_id, &PROGRAM)}});
    assert_eq!(
        verify_resolve_answer(&read, sequencer, &forged),
        Err(ReadError::UnverifiedExecution)
    );
    assert_eq!(
        interpret_answer(
            &read,
            sequencer,
            &ReadAnswer {
                status: 200,
                body: forged.to_string().into_bytes(),
            }
        ),
        Err(ResolveFailure::Unverified(ReadError::UnverifiedExecution))
    );
    assert_eq!(
        interpret_answer(
            &read,
            sequencer,
            &ReadAnswer {
                status: 422,
                body: br#"{"error":{"code":"program_read_refused","retry":"never"}}"#.to_vec(),
            }
        ),
        Err(ResolveFailure::Refused {
            status: 422,
            code: "program_read_refused".to_owned(),
        })
    );
    assert_eq!(
        interpret_answer(
            &read,
            sequencer,
            &ReadAnswer {
                status: 200,
                body: b"not json".to_vec(),
            }
        ),
        Err(ResolveFailure::Unverified(ReadError::MalformedAnswer))
    );
    Ok(())
}

#[test]
fn endpoint_answers_are_bounded_and_length_framed() -> Result<(), Box<dyn Error>> {
    let answer = parse_answer(
        b"HTTP/1.1 422 Unprocessable Entity\r\ncontent-length: 2\r\nConnection: close\r\n\r\n{}",
    )?;
    assert_eq!(
        answer,
        ReadAnswer {
            status: 422,
            body: b"{}".to_vec(),
        }
    );
    for refused in [
        &b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n{}"[..],
        &b"HTTP/1.1 200 OK\r\n\r\n{}"[..],
        &b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n"[..],
        &b"ICY 200 OK\r\nContent-Length: 2\r\n\r\n{}"[..],
    ] {
        assert!(parse_answer(refused).is_err());
    }
    for endpoint in [
        "http://core:9443",
        "https://core",
        "https://:9443",
        "https://core:0",
        "https://core:9443/v1",
        "https://user@core:9443",
    ] {
        assert!(matches!(
            ReadEndpoint::parse(endpoint, Vec::new()),
            Err(ReadError::InvalidEndpoint)
        ));
    }
    assert!(ReadEndpoint::parse(
        "https://layerx-pending-core.layerx-testnet.svc.cluster.local:9443",
        Vec::new()
    )
    .is_ok());
    Ok(())
}

//! Emits the JSON fixture consumed by the Go `layerxproof` packages and the
//! `layerxverify` precompile tests. Every positive vector is produced by the
//! real encoders and accepted by the real verifiers before it is written, and
//! every negative vector is refused by the real verifier; the recorded
//! `valid` flag and `refusal` text are the verifier's own answer.
//!
//! Usage: `cargo run -p layerx-client --example go_verifier_vectors > vectors.json`

use std::fmt::Write as _;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_client::lni::head_attestation::{
    decode_program_head_attestation, encode_program_head_attestation,
    program_discovery_proof_digest, ProgramDiscoveryHead, ProgramHeadAttestation,
};
use layerx_crypto::ed25519::{verify_digest, verify_message};
use layerx_crypto::SignatureMessage;
use layerx_proof::inclusion::{verify_receipt, SequencerAuthorization};
use layerx_proof::merkle::{build_proof, Proof};
use layerx_proof::receipt::{verify_outcome, verify_sequencer_signature, AuthorizedBatch};
use layerx_proof::state::decode_account_value;
use layerx_proof::state_witness::{AccountPath, StateWitness};
use layerx_wire::encode::Encoder;
use layerx_wire::hash::{batch_header_digest, receipt_digest, Domain};
use layerx_wire::receipt::{decode_merkle_proof, encode_merkle_proof};
use sha2::{Digest as _, Sha256};

type Failure = Box<dyn std::error::Error>;

const PROTOCOL: u16 = 3;
const GROUP_ORDER: [u8; 32] = [
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10,
];
const SMALL_ORDER_POINT: [u8; 32] = [
    0xc7, 0x17, 0x6a, 0x70, 0x3d, 0x4d, 0xd8, 0x4f, 0xba, 0x3c, 0x0b, 0x76, 0x0d, 0x10, 0x67, 0x0f,
    0x2a, 0x20, 0x53, 0xfa, 0x2c, 0x39, 0xcc, 0xc6, 0x4e, 0xc7, 0xfd, 0x77, 0x92, 0xac, 0x03, 0x7a,
];

fn fail<E: std::fmt::Debug>(context: &'static str) -> impl FnOnce(E) -> Failure {
    move |error| format!("{context}: {error:?}").into()
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// One JSON object under construction; values are already JSON-encoded.
struct Object(Vec<(String, String)>);

impl Object {
    fn new(name: &str) -> Self {
        let mut object = Self(Vec::new());
        object.text("name", name);
        object
    }
    fn text(&mut self, key: &str, value: &str) {
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        self.0.push((key.to_owned(), format!("\"{escaped}\"")));
    }
    fn bytes(&mut self, key: &str, value: &[u8]) {
        self.0.push((key.to_owned(), format!("\"{}\"", hex(value))));
    }
    fn number(&mut self, key: &str, value: u128) {
        self.0.push((key.to_owned(), format!("\"{value}\"")));
    }
    fn signed(&mut self, key: &str, value: i64) {
        self.0.push((key.to_owned(), format!("\"{value}\"")));
    }
    fn verdict<T, E: std::fmt::Debug>(&mut self, outcome: &Result<T, E>) {
        match outcome {
            Ok(_) => self.0.push(("valid".to_owned(), "true".to_owned())),
            Err(error) => {
                self.0.push(("valid".to_owned(), "false".to_owned()));
                self.text("refusal", &format!("{error:?}"));
            }
        }
    }
    fn render(&self) -> String {
        let fields: Vec<String> = self
            .0
            .iter()
            .map(|(key, value)| format!("      \"{key}\": {value}"))
            .collect();
        format!("    {{\n{}\n    }}", fields.join(",\n"))
    }
}

fn section(name: &str, objects: &[Object]) -> String {
    let rendered: Vec<String> = objects.iter().map(Object::render).collect();
    format!("  \"{name}\": [\n{}\n  ]", rendered.join(",\n"))
}

fn require<T, E: std::fmt::Debug>(
    outcome: Result<T, E>,
    expected_valid: bool,
    name: &str,
) -> Result<Result<T, E>, Failure> {
    if outcome.is_ok() == expected_valid {
        Ok(outcome)
    } else {
        Err(format!(
            "vector {name}: real verifier verdict {:?} contradicts the intended case",
            outcome.as_ref().err()
        )
        .into())
    }
}

struct ReceiptShape {
    activity_id: [u8; 32],
    global_sequence: u64,
    resulting_state_root: [u8; 32],
    effects: Vec<(u16, u16, Vec<u8>)>,
    amount: u128,
    body_limit: usize,
}

fn encode_receipt(shape: &ReceiptShape, signature: Option<[u8; 64]>) -> Result<Vec<u8>, Failure> {
    let mut e = Encoder::new(1_048_576);
    let w = fail::<layerx_wire::WireError>;
    e.structure_header_version(0x5201, PROTOCOL)
        .map_err(w("header"))?;
    e.u16(PROTOCOL).map_err(w("protocol"))?;
    e.bytes(&shape.activity_id, 32).map_err(w("activity"))?;
    e.u64(shape.global_sequence).map_err(w("sequence"))?;
    e.bytes(&[0x21; 32], 32).map_err(w("previous root"))?;
    e.bytes(&shape.resulting_state_root, 32)
        .map_err(w("resulting root"))?;
    e.bytes(&[0x22; 32], 32).map_err(w("activity root"))?;
    e.i32(0).map_err(w("result"))?;
    e.sequence_length(shape.effects.len(), 512)
        .map_err(w("effects"))?;
    for (module, ordinal, body) in &shape.effects {
        e.u16(*module).map_err(w("effect module"))?;
        e.u16(*ordinal).map_err(w("effect ordinal"))?;
        e.u16(7).map_err(w("effect event"))?;
        e.tag(3, 3).map_err(w("effect kind"))?;
        e.u8(0).map_err(w("effect monetary"))?;
        e.bytes(&[0; 32], 32).map_err(w("effect root"))?;
        e.bytes(body, shape.body_limit).map_err(w("effect body"))?;
    }
    e.u128(3).map_err(w("fee"))?;
    e.bytes(&[0x23; 32], 32).map_err(w("batch id"))?;
    e.u16(1).map_err(w("module"))?;
    e.u32(1).map_err(w("module version"))?;
    e.u32(1).map_err(w("parameter version"))?;
    e.u8(1).map_err(w("operation"))?;
    e.bytes(&[0x24; 32], 32).map_err(w("asset"))?;
    e.u128(shape.amount).map_err(w("amount"))?;
    e.bytes(&[0x25; 32], 32).map_err(w("from"))?;
    e.u128(1_000).map_err(w("from before"))?;
    e.u128(1_000 - shape.amount).map_err(w("from after"))?;
    e.u64(4).map_err(w("from sequence"))?;
    e.bytes(&[0x26; 32], 32).map_err(w("to"))?;
    e.u128(10).map_err(w("to before"))?;
    e.u128(10 + shape.amount).map_err(w("to after"))?;
    e.bytes(&[0x27; 32], 32).map_err(w("transfer root"))?;
    e.bytes(&[0x28; 32], 32).map_err(w("authorization"))?;
    e.bytes(&[0x29; 32], 32).map_err(w("context"))?;
    e.u64(1_758_000_000_000).map_err(w("timestamp"))?;
    e.u8(u8::from(signature.is_some()))
        .map_err(w("signature flag"))?;
    if let Some(signature) = signature {
        e.bytes(&signature, 64).map_err(w("signature"))?;
    }
    Ok(e.finish())
}

fn signed_receipt(shape: &ReceiptShape, key: &SigningKey) -> Result<Vec<u8>, Failure> {
    let unsigned = encode_receipt(shape, None)?;
    let digest = receipt_digest(&unsigned).map_err(fail("receipt digest"))?;
    encode_receipt(shape, Some(key.sign(&digest).to_bytes()))
}

struct HeaderShape {
    batch_number: u64,
    first_sequence: u64,
    last_sequence: u64,
    resulting_state_root: [u8; 32],
    receipt_root: [u8; 32],
    sequencer_id: [u8; 32],
}

fn encode_header(shape: &HeaderShape) -> Result<Vec<u8>, Failure> {
    let mut e = Encoder::new(354);
    let w = fail::<layerx_wire::WireError>;
    e.structure_header_version(0x1701, PROTOCOL)
        .map_err(w("batch header"))?;
    e.u8(15).map_err(w("field count"))?;
    e.tag(1, 15).map_err(w("tag"))?;
    e.u16(PROTOCOL).map_err(w("protocol"))?;
    e.tag(2, 15).map_err(w("tag"))?;
    e.u32(125).map_err(w("network"))?;
    for (field, value) in [
        (3_u8, 2_u64),
        (4, shape.batch_number),
        (5, shape.first_sequence),
        (6, shape.last_sequence),
    ] {
        e.tag(field, 15).map_err(w("tag"))?;
        e.u64(value).map_err(w("u64 field"))?;
    }
    for (field, value) in [
        (7_u8, [0x21_u8; 32]),
        (8, shape.resulting_state_root),
        (9, [0x32; 32]),
        (10, shape.receipt_root),
        (11, [0x33; 32]),
        (12, [0x34; 32]),
        (13, [0x35; 32]),
    ] {
        e.tag(field, 15).map_err(w("tag"))?;
        e.bytes(&value, 32).map_err(w("root field"))?;
    }
    e.tag(14, 15).map_err(w("tag"))?;
    e.u64(1_758_000_000_500).map_err(w("timestamp"))?;
    e.tag(15, 15).map_err(w("tag"))?;
    e.bytes(&shape.sequencer_id, 32).map_err(w("sequencer"))?;
    Ok(e.finish())
}

fn wire_proof(proof: &Proof) -> Result<Vec<u8>, Failure> {
    let mut e = Encoder::new(4 + 4 + 4 + 1 + 4 + 32 * 32);
    let w = fail::<layerx_wire::WireError>;
    e.structure_header(0x4d50).map_err(w("proof header"))?;
    e.u32(proof.leaf_index()).map_err(w("leaf index"))?;
    e.u32(proof.leaf_count()).map_err(w("leaf count"))?;
    e.u8(u8::try_from(proof.siblings().len())?)
        .map_err(w("depth"))?;
    let siblings: Vec<u8> = proof.siblings().iter().flatten().copied().collect();
    e.bytes(&siblings, 32 * 32).map_err(w("siblings"))?;
    let bytes = e.finish();
    let decoded = decode_merkle_proof(&bytes).map_err(fail("wire proof decode"))?;
    if encode_merkle_proof(&decoded).map_err(fail("wire proof encode"))? != bytes {
        return Err("wire proof did not round-trip".into());
    }
    Ok(bytes)
}

fn evidence_proof(bytes: &[u8]) -> Result<Proof, Failure> {
    let wire = decode_merkle_proof(bytes).map_err(fail("wire proof"))?;
    Proof::new(
        wire.leaf_index(),
        wire.leaf_count(),
        wire.siblings().to_vec(),
    )
    .map_err(fail("proof geometry"))
}

fn ed25519_vectors(key: &SigningKey) -> Result<Vec<Object>, Failure> {
    let public = key.verifying_key().to_bytes();
    let mut out = Vec::new();
    let domains = [
        (Domain::SignaturePreimage, 2_u8),
        (Domain::BatchHeader, 7),
        (Domain::Receipt, 8),
        (Domain::CheckpointCertificate, 9),
        (Domain::GuarantorAttestation, 19),
    ];
    for (domain, index) in domains {
        let canonical = format!("layerx signed object for domain {index}").into_bytes();
        let digest = SignatureMessage::new(domain, PROTOCOL, 125, &canonical)
            .map_err(fail("signature message"))?
            .digest();
        let signature = key.sign(&digest).to_bytes();
        let mut object = Object::new(&format!("domain-{index}"));
        object.bytes("public_key", &public);
        object.number("domain", u128::from(index));
        object.bytes("message", &canonical);
        object.bytes("digest", &digest);
        object.bytes("signature", &signature);
        object.verdict(&require(
            verify_digest(&public, &signature, &digest),
            true,
            "domain",
        )?);
        out.push(object);
    }
    let message = b"raw native message".to_vec();
    let signature = key.sign(&message).to_bytes();
    let mut cases: Vec<(&str, [u8; 32], Vec<u8>, [u8; 64], bool)> =
        vec![("raw-valid", public, message.clone(), signature, true)];
    let mut flipped = signature;
    flipped[5] ^= 0x01;
    cases.push((
        "raw-bit-flipped-signature",
        public,
        message.clone(),
        flipped,
        false,
    ));
    let mut other = message.clone();
    other[0] ^= 0x80;
    cases.push(("raw-wrong-message", public, other, signature, false));
    let mut non_reduced = signature;
    let mut carry = 0_u16;
    for index in 0..32 {
        let sum = u16::from(non_reduced[32 + index]) + u16::from(GROUP_ORDER[index]) + carry;
        non_reduced[32 + index] = u8::try_from(sum & 0xff)?;
        carry = sum >> 8;
    }
    cases.push((
        "raw-non-reduced-scalar",
        public,
        message.clone(),
        non_reduced,
        false,
    ));
    let mut identity_signature = [0_u8; 64];
    identity_signature[0] = 1;
    cases.push((
        "raw-small-order-key",
        SMALL_ORDER_POINT,
        message.clone(),
        identity_signature,
        false,
    ));
    let mut zero_key = [0_u8; 32];
    cases.push((
        "raw-zero-key",
        zero_key,
        message.clone(),
        signature,
        false,
    ));
    zero_key[0] = 1;
    cases.push((
        "raw-identity-key",
        zero_key,
        message.clone(),
        identity_signature,
        false,
    ));
    let mut non_canonical_key = [0xff_u8; 32];
    non_canonical_key[31] = 0x7f;
    cases.push((
        "raw-non-canonical-key",
        non_canonical_key,
        message,
        signature,
        false,
    ));
    for (name, public_key, message, signature, expected) in cases {
        let mut object = Object::new(name);
        object.bytes("public_key", &public_key);
        object.bytes("message", &message);
        object.bytes("signature", &signature);
        object.verdict(&require(
            verify_message(&public_key, &signature, &message),
            expected,
            name,
        )?);
        out.push(object);
    }
    Ok(out)
}

fn receipt_object(name: &str, receipt: &[u8], public: &[u8; 32], expected: bool) -> Result<Object, Failure> {
    let mut object = Object::new(name);
    object.bytes("receipt", receipt);
    object.bytes("public_key", public);
    let outcome = require(verify_sequencer_signature(receipt, *public), expected, name)?;
    if let Ok(layerx_wire::receipt::Receipt::Protocol(decoded)) = &outcome {
        let unsigned = layerx_wire::receipt::encode_unsigned(
            &layerx_wire::receipt::decode(receipt).map_err(fail("decode"))?,
        )
        .map_err(fail("unsigned"))?;
        object.bytes("unsigned", &unsigned);
        object.bytes(
            "digest",
            &receipt_digest(&unsigned).map_err(fail("digest"))?,
        );
        object.bytes("activity_id", &decoded.activity_id());
        object.number("global_sequence", u128::from(decoded.global_sequence()));
        object.signed("result_code", i64::from(decoded.result_code()));
        object.number("module_id", u128::from(decoded.module_id()));
        object.number("operation", u128::from(decoded.operation()));
        object.bytes("asset", &decoded.asset());
        object.number("amount", decoded.amount());
        object.bytes("resulting_state_root", &decoded.resulting_state_root());
        object.number("timestamp", u128::from(decoded.timestamp()));
    }
    object.verdict(&outcome);
    Ok(object)
}

fn mutate_u32(bytes: &[u8], offset: usize, value: u32) -> Vec<u8> {
    let mut out = bytes.to_vec();
    out[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    out
}

fn receipt_vectors(
    key: &SigningKey,
    receipts: &[Vec<u8>],
    oversize_body: &[u8],
) -> Result<Vec<Object>, Failure> {
    let public = key.verifying_key().to_bytes();
    let other = SigningKey::from_bytes(&[0x52; 32]).verifying_key().to_bytes();
    let base = &receipts[0];
    let mut out = Vec::new();
    for (index, receipt) in receipts.iter().enumerate() {
        out.push(receipt_object(&format!("valid-{index}"), receipt, &public, true)?);
    }
    let mut flipped = base.clone();
    let last = flipped.len() - 1;
    flipped[last] ^= 0x01;
    out.push(receipt_object("bit-flipped-signature", &flipped, &public, false)?);
    let mut body = base.clone();
    body[60] ^= 0x01;
    out.push(receipt_object("bit-flipped-body", &body, &public, false)?);
    out.push(receipt_object("wrong-sequencer-key", base, &other, false)?);
    out.push(receipt_object("truncated", &base[..base.len() - 1], &public, false)?);
    out.push(receipt_object("truncated-half", &base[..base.len() / 2], &public, false)?);
    let mut trailing = base.clone();
    trailing.push(0);
    out.push(receipt_object("trailing-byte", &trailing, &public, false)?);
    let unsigned = layerx_wire::receipt::encode_unsigned(
        &layerx_wire::receipt::decode(base).map_err(fail("decode base"))?,
    )
    .map_err(fail("unsigned base"))?;
    out.push(receipt_object("missing-signature", &unsigned, &public, false)?);
    // Offset 6 is the activity-identifier length prefix; 162 the effect count.
    out.push(receipt_object(
        "oversize-digest-length",
        &mutate_u32(base, 6, 33),
        &public,
        false,
    )?);
    out.push(receipt_object(
        "oversize-effect-count",
        &mutate_u32(base, 162, 513),
        &public,
        false,
    )?);
    out.push(receipt_object("oversize-effect-body", oversize_body, &public, false)?);
    let mut legacy = base.clone();
    legacy[1] = 1;
    legacy[5] = 1;
    out.push(receipt_object("legacy-protocol-version", &legacy, &public, false)?);
    let mut unknown_tag = base.clone();
    unknown_tag[3] = 3;
    out.push(receipt_object("unknown-structure-tag", &unknown_tag, &public, false)?);
    Ok(out)
}

struct Batch {
    receipts: Vec<Vec<u8>>,
    header: Vec<u8>,
    header_signature: [u8; 64],
    receipt_root: [u8; 32],
}

fn inclusion_object(
    name: &str,
    receipt: &[u8],
    proof: &[u8],
    header: &[u8],
    header_signature: &[u8; 64],
    authorization: &SequencerAuthorization,
    expected: bool,
) -> Result<Object, Failure> {
    let mut object = Object::new(name);
    object.bytes("receipt", receipt);
    object.bytes("proof", proof);
    object.bytes("header", header);
    object.bytes("header_signature", header_signature);
    object.bytes("sequencer_id", &authorization.sequencer_id());
    object.bytes("public_key", &authorization.public_key());
    object.number("first_batch", u128::from(authorization.first_batch_number()));
    object.number("last_batch", u128::from(authorization.last_batch_number()));
    let outcome: Result<_, String> = evidence_proof(proof)
        .map_err(|error| format!("Proof({error})"))
        .and_then(|proof| {
            verify_receipt(receipt, &proof, header, header_signature, authorization)
                .map_err(|error| format!("{error:?}"))
        })
        .and_then(|evidence| {
            verify_sequencer_signature(receipt, authorization.public_key())
                .map(|_| evidence)
                .map_err(|error| format!("{error:?}"))
        });
    let outcome = require(outcome, expected, name)?;
    if let Ok(evidence) = &outcome {
        object.bytes("header_digest", &evidence.header().digest());
        object.bytes(
            "receipt_root",
            &evidence.header().header().receipt_merkle_root(),
        );
        object.bytes(
            "resulting_state_root",
            &evidence.header().header().resulting_state_root(),
        );
        object.number(
            "batch_number",
            u128::from(evidence.header().header().batch_number()),
        );
    }
    object.verdict(&outcome);
    Ok(object)
}

fn inclusion_vectors(key: &SigningKey, batch: &Batch) -> Result<Vec<Object>, Failure> {
    let public = key.verifying_key().to_bytes();
    let authorization = SequencerAuthorization::new(public, public, 1, 100);
    let leaves: Vec<&[u8]> = batch.receipts.iter().map(Vec::as_slice).collect();
    let mut out = Vec::new();
    let mut proofs = Vec::new();
    for index in 0..leaves.len() {
        let (proof, root) = build_proof(&leaves, index).map_err(fail("build proof"))?;
        if root != batch.receipt_root {
            return Err("receipt root drifted".into());
        }
        let bytes = wire_proof(&proof)?;
        out.push(inclusion_object(
            &format!("valid-leaf-{index}"),
            leaves[index],
            &bytes,
            &batch.header,
            &batch.header_signature,
            &authorization,
            true,
        )?);
        proofs.push(bytes);
    }
    let receipt = leaves[0];
    let proof = &proofs[0];
    let mut wrong_sibling = proof.clone();
    let last = wrong_sibling.len() - 1;
    wrong_sibling[last] ^= 0x01;
    out.push(inclusion_object("wrong-root-sibling", receipt, &wrong_sibling, &batch.header, &batch.header_signature, &authorization, false)?);
    out.push(inclusion_object("proof-of-other-leaf", receipt, &proofs[1], &batch.header, &batch.header_signature, &authorization, false)?);
    out.push(inclusion_object("truncated-proof", receipt, &proof[..proof.len() - 1], &batch.header, &batch.header_signature, &authorization, false)?);
    let mut trailing = proof.clone();
    trailing.push(0);
    out.push(inclusion_object("trailing-proof-byte", receipt, &trailing, &batch.header, &batch.header_signature, &authorization, false)?);
    let mut deep = proof.clone();
    deep[12] = 33;
    out.push(inclusion_object("oversize-proof-depth", receipt, &deep, &batch.header, &batch.header_signature, &authorization, false)?);
    out.push(inclusion_object("index-outside-tree", receipt, &mutate_u32(proof, 4, 3), &batch.header, &batch.header_signature, &authorization, false)?);
    let mut promotion = proofs[2].clone();
    promotion[17] ^= 0x01;
    out.push(inclusion_object("forged-promotion-sibling", leaves[2], &promotion, &batch.header, &batch.header_signature, &authorization, false)?);
    let mut header_signature = batch.header_signature;
    header_signature[0] ^= 0x01;
    out.push(inclusion_object("bit-flipped-header-signature", receipt, proof, &batch.header, &header_signature, &authorization, false)?);
    let mut header = batch.header.clone();
    header[200] ^= 0x01;
    out.push(inclusion_object("mutated-header-root", receipt, proof, &header, &batch.header_signature, &authorization, false)?);
    out.push(inclusion_object("truncated-header", receipt, proof, &batch.header[..353], &batch.header_signature, &authorization, false)?);
    let mut long_header = batch.header.clone();
    long_header.push(0);
    out.push(inclusion_object("trailing-header-byte", receipt, proof, &long_header, &batch.header_signature, &authorization, false)?);
    let outside = SequencerAuthorization::new(public, public, 8, 100);
    out.push(inclusion_object("batch-outside-authorisation", receipt, proof, &batch.header, &batch.header_signature, &outside, false)?);
    let stranger = SequencerAuthorization::new([0x77; 32], public, 1, 100);
    out.push(inclusion_object("wrong-sequencer-identity", receipt, proof, &batch.header, &batch.header_signature, &stranger, false)?);
    let mut tampered = receipt.to_vec();
    tampered[60] ^= 0x01;
    out.push(inclusion_object("tampered-receipt", &tampered, proof, &batch.header, &batch.header_signature, &authorization, false)?);
    Ok(out)
}

fn state_leaf(key: &[u8], value: &[u8]) -> Result<[u8; 32], Failure> {
    let mut hasher = Sha256::new();
    hasher.update(b"LXP/v1/state-leaf\0");
    hasher.update(u32::try_from(key.len())?.to_be_bytes());
    hasher.update(u32::try_from(value.len())?.to_be_bytes());
    hasher.update(key);
    hasher.update(value);
    Ok(hasher.finalize().into())
}

fn account_value(name: &[u8], kind: u8, asset: [u8; 32], balance: u128) -> Result<([u8; 32], Vec<u8>), Failure> {
    let mut hasher = Sha256::new();
    hasher.update(b"LX:ACCOUNT:v1");
    hasher.update(u32::try_from(name.len())?.to_be_bytes());
    hasher.update(name);
    let account_id: [u8; 32] = hasher.finalize().into();
    let mut value = u16::try_from(name.len())?.to_be_bytes().to_vec();
    value.extend_from_slice(name);
    value.push(kind);
    value.extend_from_slice(&balance.to_be_bytes());
    value.extend_from_slice(&asset);
    value.push(1);
    value.extend_from_slice(&9_u64.to_be_bytes());
    value.extend_from_slice(&2_u64.to_be_bytes());
    value.extend_from_slice(&[0, 0]);
    value.extend_from_slice(&[0x5a; 32]);
    value.push(1);
    decode_account_value(account_id, &value).map_err(fail("account value"))?;
    Ok((account_id, value))
}

fn state_object(name: &str, witness: &[u8], root: &[u8; 32], expected: bool) -> Result<Object, Failure> {
    let mut object = Object::new(name);
    object.bytes("witness", witness);
    object.bytes("state_root", root);
    let outcome = StateWitness::decode(witness).and_then(|decoded| {
        decoded.verify(*root)?;
        Ok(decoded)
    });
    let outcome = require(outcome, expected, name)?;
    if let Ok(decoded) = &outcome {
        if decoded.encode().map_err(fail("witness encode"))? != witness {
            return Err("state witness did not round-trip".into());
        }
        object.number("module_id", u128::from(decoded.module_id));
        object.bytes("key", &decoded.key);
        object.bytes("value", &decoded.value);
    }
    object.verdict(&outcome);
    Ok(object)
}

struct StateFixture {
    objects: Vec<Object>,
    accounts: Vec<Object>,
}

fn state_vectors() -> Result<StateFixture, Failure> {
    let asset = [0x24_u8; 32];
    let (account_id, value) = account_value(b"system:fees", 10, asset, 123_456_789)?;
    let mut key = vec![4_u8];
    key.extend_from_slice(&account_id);
    let account = StateWitness {
        module_id: 0,
        key,
        value: value.clone(),
        account_path: Some(AccountPath {
            index: 1,
            count: 3,
            siblings: vec![state_leaf(&[4; 33], b"neighbour")?, [0x91; 32]],
        }),
        leaf_index_a: 0,
        leaf_count_a: 2,
        siblings_a: vec![state_leaf(b"sequence", &11_u64.to_be_bytes())?],
        leaf_count_b: 10,
        siblings_b: vec![[0xa1; 32], [0xa2; 32], [0xa3; 32], [0xa4; 32]],
    };
    let account_root = account.root().map_err(fail("account root"))?;
    let account_bytes = account.encode().map_err(fail("account encode"))?;

    let module_key = b"program-account/0001".to_vec();
    let module_value = vec![0x02_u8; 97];
    let module = StateWitness {
        module_id: 3,
        key: module_key.clone(),
        value: module_value.clone(),
        account_path: None,
        leaf_index_a: 2,
        leaf_count_a: 3,
        siblings_a: vec![state_leaf(&module_key, &module_value)?, [0xb1; 32]],
        leaf_count_b: 9,
        siblings_b: vec![[0xc1; 32], [0xc2; 32], [0xc3; 32], [0xc4; 32]],
    };
    let module_root = module.root().map_err(fail("module root"))?;
    let module_bytes = module.encode().map_err(fail("module encode"))?;

    let mut objects = vec![
        state_object("valid-account", &account_bytes, &account_root, true)?,
        state_object("valid-module-promoted", &module_bytes, &module_root, true)?,
    ];
    let mut wrong_root = account_root;
    wrong_root[0] ^= 0x01;
    objects.push(state_object("wrong-root", &account_bytes, &wrong_root, false)?);
    objects.push(state_object("root-of-other-witness", &account_bytes, &module_root, false)?);
    objects.push(state_object(
        "truncated",
        &account_bytes[..account_bytes.len() - 1],
        &account_root,
        false,
    )?);
    let mut trailing = account_bytes.clone();
    trailing.push(0);
    objects.push(state_object("trailing-byte", &trailing, &account_root, false)?);
    let mut value_flip = account_bytes.clone();
    value_flip[50] ^= 0x01;
    objects.push(state_object("mutated-value", &value_flip, &account_root, false)?);
    let mut version = module_bytes.clone();
    version[1] = 1;
    objects.push(state_object("unsupported-version", &version, &module_root, false)?);
    let mut module_ten = module_bytes.clone();
    module_ten[3] = 10;
    objects.push(state_object("module-out-of-range", &module_ten, &module_root, false)?);
    objects.push(state_object(
        "oversize-key-length",
        &mutate_u32(&module_bytes, 4, 130),
        &module_root,
        false,
    )?);
    objects.push(state_object(
        "oversize-value-length",
        &mutate_u32(&module_bytes, 8 + module_key.len(), 1_048_577),
        &module_root,
        false,
    )?);
    // Path A of the module witness starts after both prefixed fields.
    let path_a = 4 + 4 + module_key.len() + 4 + module_value.len();
    let mut deep = module_bytes[..path_a + 8].to_vec();
    deep.push(33);
    for _ in 0..33 {
        deep.extend_from_slice(&[0xd1; 32]);
    }
    deep.extend_from_slice(&module_bytes[path_a + 9 + 64..]);
    objects.push(state_object("oversize-path-depth", &deep, &module_root, false)?);
    let mut promotion = module_bytes.clone();
    promotion[path_a + 9] ^= 0x01;
    objects.push(state_object("forged-promotion-sibling", &promotion, &module_root, false)?);
    objects.push(state_object(
        "index-outside-subtree",
        &mutate_u32(&module_bytes, path_a, 3),
        &module_root,
        false,
    )?);
    let count_b = path_a + 9 + 64;
    objects.push(state_object(
        "module-tree-width",
        &mutate_u32(&module_bytes, count_b, 11),
        &module_root,
        false,
    )?);
    let mut missing_account_path = 2_u16.to_be_bytes().to_vec();
    missing_account_path.extend_from_slice(&account_bytes[2..4 + 4 + 33 + 4 + value.len()]);
    missing_account_path.extend_from_slice(&module_bytes[path_a..]);
    objects.push(state_object(
        "account-key-without-account-path",
        &missing_account_path,
        &account_root,
        false,
    )?);

    let mut accounts = Vec::new();
    let mut good = Object::new("valid-account-value");
    good.bytes("account_id", &account_id);
    good.bytes("asset_id", &asset);
    good.bytes("value", &value);
    good.number("balance", 123_456_789);
    good.verdict(&require(decode_account_value(account_id, &value), true, "account")?);
    accounts.push(good);
    let mut stranger = Object::new("account-identity-mismatch");
    stranger.bytes("account_id", &[0x44; 32]);
    stranger.bytes("asset_id", &asset);
    stranger.bytes("value", &value);
    stranger.verdict(&require(decode_account_value([0x44; 32], &value), false, "identity")?);
    accounts.push(stranger);
    let mut long = value.clone();
    long.push(0);
    let mut trailing_value = Object::new("account-trailing-byte");
    trailing_value.bytes("account_id", &account_id);
    trailing_value.bytes("asset_id", &asset);
    trailing_value.bytes("value", &long);
    trailing_value.verdict(&require(decode_account_value(account_id, &long), false, "trailing")?);
    accounts.push(trailing_value);
    let short = &value[..value.len() - 1];
    let mut truncated_value = Object::new("account-truncated");
    truncated_value.bytes("account_id", &account_id);
    truncated_value.bytes("asset_id", &asset);
    truncated_value.bytes("value", short);
    truncated_value.verdict(&require(decode_account_value(account_id, short), false, "short")?);
    accounts.push(truncated_value);
    Ok(StateFixture { objects, accounts })
}

fn discovery_object(
    name: &str,
    payload: &[u8],
    proof: &[u8],
    program_id: &[u8; 32],
    staleness_ms: u64,
    public: &[u8; 32],
    expected: bool,
) -> Result<Object, Failure> {
    let mut object = Object::new(name);
    object.bytes("payload", payload);
    object.bytes("proof_material", proof);
    object.bytes("program_id", program_id);
    object.number("staleness_ms", u128::from(staleness_ms));
    object.bytes("public_key", public);
    let outcome = require(
        decode_program_head_attestation(payload, proof, program_id, staleness_ms, public),
        expected,
        name,
    )?;
    if let Ok(attestation) = &outcome {
        object.bytes("digest", &attestation.digest);
        object.bytes("state_root", &attestation.head.state_root);
        object.bytes("code_hash", &attestation.head.code_hash);
        object.bytes("head_receipt_digest", &attestation.head_receipt_digest);
        object.number("version", u128::from(attestation.head.version));
        object.number("abi_version", u128::from(attestation.head.abi_version));
        object.number(
            "observed_sequence",
            u128::from(attestation.head.observed_sequence),
        );
        object.number("observed_at", u128::from(attestation.head.observed_at));
        object.number("valid_through", u128::from(attestation.head.valid_through));
    }
    object.verdict(&outcome);
    Ok(object)
}

fn discovery_vectors(key: &SigningKey) -> Result<Vec<Object>, Failure> {
    let public = key.verifying_key().to_bytes();
    let staleness = 30_000_u64;
    let program_id = [0x61_u8; 32];
    let head = ProgramDiscoveryHead {
        program_id,
        version: 4,
        code_hash: [0x62; 32],
        abi_version: 2,
        observed_sequence: 812,
        observed_at: 1_758_000_000_000,
        valid_through: 1_758_000_000_000 + staleness,
        state_root: [0x63; 32],
    };
    let digest = program_discovery_proof_digest(&head);
    let attestation = ProgramHeadAttestation {
        head,
        head_receipt_digest: [0x64; 32],
        digest,
        public_key: public,
        signature: key.sign(&digest).to_bytes(),
    };
    let (payload, proof) = encode_program_head_attestation(&attestation);
    let mut out = vec![discovery_object(
        "valid", &payload, &proof, &program_id, staleness, &public, true,
    )?];
    let mut flipped = proof;
    flipped[95] ^= 0x01;
    out.push(discovery_object("bit-flipped-signature", &payload, &flipped, &program_id, staleness, &public, false)?);
    let mut root = payload;
    root[100] ^= 0x01;
    out.push(discovery_object("wrong-state-root", &root, &proof, &program_id, staleness, &public, false)?);
    out.push(discovery_object("truncated-payload", &payload[..159], &proof, &program_id, staleness, &public, false)?);
    let mut long = payload.to_vec();
    long.push(0);
    out.push(discovery_object("trailing-payload-byte", &long, &proof, &program_id, staleness, &public, false)?);
    out.push(discovery_object("truncated-proof-material", &payload, &proof[..95], &program_id, staleness, &public, false)?);
    let mut long_proof = proof.to_vec();
    long_proof.push(0);
    out.push(discovery_object("oversize-proof-material", &payload, &long_proof, &program_id, staleness, &public, false)?);
    out.push(discovery_object("other-program", &payload, &proof, &[0x71; 32], staleness, &public, false)?);
    out.push(discovery_object("other-staleness-window", &payload, &proof, &program_id, staleness + 1, &public, false)?);
    let other = SigningKey::from_bytes(&[0x52; 32]).verifying_key().to_bytes();
    out.push(discovery_object("untrusted-sequencer-key", &payload, &proof, &program_id, staleness, &other, false)?);
    let mut layout = payload;
    layout[0] ^= 0x02;
    out.push(discovery_object("unknown-layout-version", &layout, &proof, &program_id, staleness, &public, false)?);
    Ok(out)
}

const WITHDRAWAL_RECEIPT: &[u8] =
    include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/receipt");
const WITHDRAWAL_PROOF: &[u8] =
    include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/receipt.proof");
const WITHDRAWAL_HEADER: &[u8] =
    include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/header");
const WITHDRAWAL_HEADER_SIGNATURE: &[u8; 64] =
    include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/header.signature");
const WITHDRAWAL_SEQUENCER: [u8; 32] =
    *include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/sequencer.public");

fn withdrawal_facts(receipt: &[u8], key: [u8; 32]) -> Result<AuthorizedBatch, Failure> {
    let decoded = layerx_wire::receipt::decode(receipt).map_err(fail("withdrawal decode"))?;
    let protocol = decoded.protocol().ok_or("withdrawal protocol receipt")?;
    Ok(AuthorizedBatch::new(
        protocol.batch_id(),
        protocol.asset(),
        protocol.previous_state_root(),
        protocol.resulting_state_root(),
        key,
    ))
}

fn withdrawal_object(name: &str, receipt: &[u8], key: [u8; 32]) -> Result<Object, Failure> {
    let mut object = Object::new(name);
    object.bytes("receipt", receipt);
    object.bytes("public_key", &key);
    let outcome = verify_outcome(receipt, &withdrawal_facts(receipt, key)?);
    if let Ok(verified) = &outcome {
        let protocol = verified
            .receipt()
            .protocol()
            .ok_or("withdrawal protocol receipt")?;
        let body = protocol
            .effects()
            .get(1)
            .ok_or("withdrawal event effect")?
            .body();
        object.number(
            "network_id",
            u128::from(u32::from_be_bytes(body[2..6].try_into()?)),
        );
        object.bytes("withdrawal_id", &body[6..38]);
        object.bytes("account", &body[38..70]);
        object.bytes("asset", &body[70..102]);
        object.number("amount", u128::from_be_bytes(body[102..118].try_into()?));
        object.bytes("recipient", &body[130..150]);
        object.bytes("anchor", &body[150..182]);
        object.bytes("nullifier", &protocol.context_hash());
        object.number(
            "fee_limit",
            u128::from(u64::from_be_bytes(body[246..254].try_into()?)),
        );
    }
    object.verdict(&outcome);
    Ok(object)
}

fn mutated_withdrawal(offset: usize, key: &SigningKey) -> Result<Vec<u8>, Failure> {
    let decoded =
        layerx_wire::receipt::decode(WITHDRAWAL_RECEIPT).map_err(fail("withdrawal decode"))?;
    let protocol = decoded.protocol().ok_or("withdrawal protocol receipt")?;
    let body = protocol
        .effects()
        .get(1)
        .ok_or("withdrawal event effect")?
        .body();
    let mut unsigned =
        layerx_wire::receipt::encode_unsigned(&decoded).map_err(fail("withdrawal unsigned"))?;
    let locations: Vec<usize> = unsigned
        .windows(body.len())
        .enumerate()
        .filter_map(|(position, value)| (value == body).then_some(position))
        .collect();
    let [start] = locations.as_slice() else {
        return Err("withdrawal event body is not unique".into());
    };
    unsigned[start + offset] ^= 1;
    if offset == 130 {
        let mut payload = [0_u8; 108];
        payload[..32].copy_from_slice(&body[70..102]);
        payload[32..48].copy_from_slice(&body[102..118]);
        payload[48..68].copy_from_slice(&unsigned[start + 130..start + 150]);
        payload[68..100].copy_from_slice(&body[150..182]);
        payload[100..].copy_from_slice(&body[246..]);
        let kind = layerx_types::payload::ActivityType::new(
            layerx_types::payload::ModuleId::Asset,
            9,
        )
        .map_err(fail("withdrawal kind"))?;
        let registry =
            layerx_proof::receipt::withdrawal::registry().map_err(fail("withdrawal registry"))?;
        let value = layerx_types::payload::Payload::new(&registry, kind, &payload)
            .map_err(fail("withdrawal payload"))?;
        let digest =
            layerx_wire::hash::payload_hash_for(&value).map_err(fail("withdrawal payload hash"))?;
        unsigned[start + 182..start + 214].copy_from_slice(&digest);
    }
    let digest = receipt_digest(&unsigned).map_err(fail("withdrawal digest"))?;
    if unsigned.pop() != Some(0) {
        return Err("withdrawal unsigned terminator".into());
    }
    let mut signature = Encoder::new(69);
    signature.u8(1).map_err(fail("withdrawal signature flag"))?;
    signature
        .bytes(&key.sign(&digest).to_bytes(), 64)
        .map_err(fail("withdrawal signature"))?;
    unsigned.extend_from_slice(&signature.finish());
    Ok(unsigned)
}

fn withdrawal_vectors() -> Result<Vec<Object>, Failure> {
    let header = layerx_wire::receipt::decode_batch_header(WITHDRAWAL_HEADER)
        .map_err(fail("withdrawal header"))?;
    let authorization = SequencerAuthorization::new(
        header.sequencer_id(),
        WITHDRAWAL_SEQUENCER,
        header.batch_number(),
        header.batch_number(),
    );
    let proof = evidence_proof(WITHDRAWAL_PROOF)?;
    verify_receipt(
        WITHDRAWAL_RECEIPT,
        &proof,
        WITHDRAWAL_HEADER,
        WITHDRAWAL_HEADER_SIGNATURE,
        &authorization,
    )
    .map_err(fail("withdrawal inclusion"))?;
    let mut real = withdrawal_object(
        "real-native-withdrawal",
        WITHDRAWAL_RECEIPT,
        WITHDRAWAL_SEQUENCER,
    )?;
    if real.0.iter().any(|(key, value)| key == "valid" && value != "true") {
        return Err("the real native withdrawal was refused".into());
    }
    real.bytes("proof", WITHDRAWAL_PROOF);
    real.bytes("header", WITHDRAWAL_HEADER);
    real.bytes("header_signature", WITHDRAWAL_HEADER_SIGNATURE);
    real.bytes("sequencer_id", &header.sequencer_id());
    real.number("batch_number", u128::from(header.batch_number()));
    real.number("header_network_id", u128::from(header.network_id()));
    real.bytes("header_state_root", &header.resulting_state_root());
    real.bytes("header_receipt_root", &header.receipt_merkle_root());
    let mut out = vec![real];
    let key = SigningKey::from_bytes(&[0x39; 32]);
    for offset in [0, 2, 6, 38, 70, 102, 118, 130, 150, 182, 214, 253] {
        out.push(withdrawal_object(
            &format!("resigned-event-offset-{offset}"),
            &mutated_withdrawal(offset, &key)?,
            key.verifying_key().to_bytes(),
        )?);
    }
    out.push(withdrawal_object(
        "real-receipt-under-foreign-key",
        WITHDRAWAL_RECEIPT,
        key.verifying_key().to_bytes(),
    )?);
    Ok(out)
}

fn exit_vectors() -> Result<Vec<Object>, Failure> {
    let authority = SigningKey::from_bytes(&[0x61; 32]);
    let asset = [0x24_u8; 32];
    let name = b"agent:exit-holder:main";
    let (account_id, mut value) = account_value(name, 1, asset, 5_000_000)?;
    let at = value.len() - 33;
    value[at..at + 32].copy_from_slice(&authority.verifying_key().to_bytes());
    decode_account_value(account_id, &value).map_err(fail("exit account value"))?;
    let mut key = vec![4_u8];
    key.extend_from_slice(&account_id);
    let witness = StateWitness {
        module_id: 0,
        key,
        value,
        account_path: Some(AccountPath {
            index: 0,
            count: 2,
            siblings: vec![state_leaf(&[4; 33], b"neighbour")?],
        }),
        leaf_index_a: 0,
        leaf_count_a: 2,
        siblings_a: vec![state_leaf(b"sequence", &21_u64.to_be_bytes())?],
        leaf_count_b: 10,
        siblings_b: vec![[0xd1; 32], [0xd2; 32], [0xd3; 32], [0xd4; 32]],
    };
    let root = witness.root().map_err(fail("exit root"))?;
    let bytes = witness.encode().map_err(fail("exit encode"))?;
    let network_id = 7332_u32;
    let recipient = [0x42_u8; 20];
    let mut message = b"LX:SETTLE:RECIPIENT:v1\0".to_vec();
    message.extend_from_slice(&network_id.to_be_bytes());
    message.extend_from_slice(&account_id);
    message.extend_from_slice(&asset);
    message.extend_from_slice(&recipient);
    message.extend_from_slice(&root);
    let signature = authority.sign(&message).to_bytes();
    let mut out = Vec::new();
    for (name, signature, expected) in [
        ("valid-exit", signature, true),
        (
            "bit-flipped-recipient-signature",
            {
                let mut changed = signature;
                changed[5] ^= 0x01;
                changed
            },
            false,
        ),
    ] {
        let mut object = state_object(name, &bytes, &root, true)?;
        object.0.retain(|(key, _)| key != "valid");
        object.number("network_id", u128::from(network_id));
        object.bytes("account", &account_id);
        object.bytes("asset", &asset);
        object.number("balance", 5_000_000);
        object.bytes("recipient", &recipient);
        object.bytes("authority", &authority.verifying_key().to_bytes());
        object.bytes("message", &message);
        object.bytes("recipient_signature", &signature);
        object.verdict(&require(
            verify_message(&authority.verifying_key().to_bytes(), &signature, &message),
            expected,
            name,
        )?);
        out.push(object);
    }
    Ok(out)
}

fn main() -> Result<(), Failure> {
    let key = SigningKey::from_bytes(&[0x51; 32]);
    let public = key.verifying_key().to_bytes();
    let state_root = [0x31_u8; 32];
    let mut receipts = Vec::new();
    for (index, effects) in [
        Vec::new(),
        Vec::new(),
        vec![(1_u16, 0_u16, b"effect body".to_vec())],
    ]
    .into_iter()
    .enumerate()
    {
        let ordinal = u8::try_from(index)?;
        let shape = ReceiptShape {
            activity_id: [0x11 + ordinal; 32],
            global_sequence: 10 + u64::from(ordinal),
            resulting_state_root: state_root,
            effects,
            amount: 25 + u128::from(ordinal),
            body_limit: 256,
        };
        receipts.push(signed_receipt(&shape, &key)?);
    }
    let oversize_body = signed_receipt(
        &ReceiptShape {
            activity_id: [0x19; 32],
            global_sequence: 19,
            resulting_state_root: state_root,
            effects: vec![(1, 0, vec![0x5c; 257])],
            amount: 1,
            body_limit: 257,
        },
        &key,
    )?;
    let leaves: Vec<&[u8]> = receipts.iter().map(Vec::as_slice).collect();
    let receipt_root = layerx_proof::merkle::root(&leaves).map_err(fail("receipt root"))?;
    let header = encode_header(&HeaderShape {
        batch_number: 7,
        first_sequence: 10,
        last_sequence: 12,
        resulting_state_root: state_root,
        receipt_root,
        sequencer_id: public,
    })?;
    let header_digest = batch_header_digest(&header).map_err(fail("header digest"))?;
    let batch = Batch {
        receipts: receipts.clone(),
        header,
        header_signature: key.sign(&header_digest).to_bytes(),
        receipt_root,
    };
    let state = state_vectors()?;
    let sections = [
        section("ed25519", &ed25519_vectors(&key)?),
        section("receipts", &receipt_vectors(&key, &receipts, &oversize_body)?),
        section("inclusion", &inclusion_vectors(&key, &batch)?),
        section("state", &state.objects),
        section("accounts", &state.accounts),
        section("discovery", &discovery_vectors(&key)?),
        section("withdrawal", &withdrawal_vectors()?),
        section("exit", &exit_vectors()?),
    ];
    println!(
        "{{\n  \"generator\": \"agent/crates/layerx-client/examples/go_verifier_vectors.rs\",\n{}\n}}",
        sections.join(",\n")
    );
    Ok(())
}

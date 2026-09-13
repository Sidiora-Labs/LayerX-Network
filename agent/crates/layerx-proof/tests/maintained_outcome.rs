use ed25519_dalek::{Signer as _, SigningKey};
use layerx_proof::inclusion::{InclusionError, SequencerAuthorization};
use layerx_proof::merkle::{build_proof, Proof};
use layerx_proof::receipt::{
    verify_outcome, verify_outcome_maintained, verify_outcome_maintained_chain, AuthorizedBatch,
    MaintainedOutcomeEvidence, MaintainedOutcomeFailure, ReceiptCheck,
};
use layerx_wire::encode::Encoder;
use layerx_wire::hash::{batch_header_digest, program_execution_batch_id, receipt_digest};
use layerx_wire::limits::PROTOCOL_VERSION;
use layerx_wire::receipt::decode_batch_header;
#[path = "../../layerx-wire/tests/support/maintenance.rs"]
mod maintenance;
fn must<T, E: core::fmt::Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("{error:?}"))
}
#[derive(Clone)]
struct Fields {
    activity_id: [u8; 32],
    sequence: u64,
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    batch_id: [u8; 32],
    asset: [u8; 32],
    amount: u128,
    from: [u8; 32],
    from_before: u128,
    from_after: u128,
    to: [u8; 32],
    to_before: u128,
    to_after: u128,
}

fn fields() -> Fields {
    Fields {
        activity_id: [1; 32],
        sequence: 9,
        previous_state_root: [2; 32],
        resulting_state_root: [3; 32],
        batch_id: [4; 32],
        asset: [5; 32],
        amount: 25,
        from: [6; 32],
        from_before: 100,
        from_after: 75,
        to: [7; 32],
        to_before: 10,
        to_after: 35,
    }
}

fn encode_fields_version(
    fields: &Fields,
    signature: Option<[u8; 64]>,
    protocol_version: u16,
) -> Vec<u8> {
    let mut encoder = Encoder::new(4096);
    assert_eq!(
        encoder.structure_header_version(0x5201, protocol_version),
        Ok(())
    );
    assert_eq!(encoder.u16(protocol_version), Ok(()));
    assert_eq!(encoder.bytes(&fields.activity_id, 32), Ok(()));
    assert_eq!(encoder.u64(fields.sequence), Ok(()));
    assert_eq!(encoder.bytes(&fields.previous_state_root, 32), Ok(()));
    assert_eq!(encoder.bytes(&fields.resulting_state_root, 32), Ok(()));
    assert_eq!(encoder.bytes(&[8; 32], 32), Ok(()));
    assert_eq!(encoder.i32(0), Ok(()));
    assert_eq!(encoder.sequence_length(0, 512), Ok(()));
    assert_eq!(encoder.u128(1), Ok(()));
    assert_eq!(encoder.bytes(&fields.batch_id, 32), Ok(()));
    assert_eq!(encoder.u16(1), Ok(()));
    assert_eq!(encoder.u32(1), Ok(()));
    assert_eq!(encoder.u32(1), Ok(()));
    assert_eq!(encoder.u8(1), Ok(()));
    assert_eq!(encoder.bytes(&fields.asset, 32), Ok(()));
    assert_eq!(encoder.u128(fields.amount), Ok(()));
    assert_eq!(encoder.bytes(&fields.from, 32), Ok(()));
    assert_eq!(encoder.u128(fields.from_before), Ok(()));
    assert_eq!(encoder.u128(fields.from_after), Ok(()));
    assert_eq!(encoder.u64(1), Ok(()));
    assert_eq!(encoder.bytes(&fields.to, 32), Ok(()));
    assert_eq!(encoder.u128(fields.to_before), Ok(()));
    assert_eq!(encoder.u128(fields.to_after), Ok(()));
    assert_eq!(encoder.bytes(&[9; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[10; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[11; 32], 32), Ok(()));
    assert_eq!(encoder.u64(1_000), Ok(()));
    assert_eq!(encoder.u8(u8::from(signature.is_some())), Ok(()));
    if let Some(value) = signature {
        assert_eq!(encoder.bytes(&value, 64), Ok(()));
    }
    encoder.finish()
}

fn header_bytes(
    batch_number: u64,
    resulting_state_root: [u8; 32],
    receipt_merkle_root: [u8; 32],
    count: u64,
    sequencer_id: [u8; 32],
) -> Vec<u8> {
    let mut encoder = Encoder::new(354);
    assert_eq!(
        encoder.structure_header_version(0x1701, PROTOCOL_VERSION),
        Ok(())
    );
    assert_eq!(encoder.u8(15), Ok(()));
    let fields: [(u8, Vec<u8>); 15] = [
        (1, PROTOCOL_VERSION.to_be_bytes().to_vec()),
        (2, 42_u32.to_be_bytes().to_vec()),
        (3, 2_u64.to_be_bytes().to_vec()),
        (4, batch_number.to_be_bytes().to_vec()),
        (5, 9_u64.to_be_bytes().to_vec()),
        (6, (9 + count).to_be_bytes().to_vec()),
        (7, [2; 32].to_vec()),
        (8, resulting_state_root.to_vec()),
        (9, [8; 32].to_vec()),
        (10, receipt_merkle_root.to_vec()),
        (11, [3; 32].to_vec()),
        (12, [4; 32].to_vec()),
        (13, [5; 32].to_vec()),
        (14, 1_000_u64.to_be_bytes().to_vec()),
        (15, sequencer_id.to_vec()),
    ];
    for (field, value) in fields {
        assert_eq!(encoder.tag(field, 15), Ok(()));
        match field {
            1 => assert_eq!(
                encoder.u16(u16::from_be_bytes([value[0], value[1]])),
                Ok(())
            ),
            2 => assert_eq!(
                encoder.u32(u32::from_be_bytes([value[0], value[1], value[2], value[3]])),
                Ok(())
            ),
            3..=6 | 14 => assert_eq!(
                encoder.u64(u64::from_be_bytes(
                    value
                        .as_slice()
                        .try_into()
                        .unwrap_or_else(|_| panic!("invalid u64 test field")),
                )),
                Ok(())
            ),
            _ => assert_eq!(encoder.bytes(&value, 32), Ok(())),
        }
    }
    let bytes = encoder.finish();
    assert_eq!(bytes.len(), 354);
    bytes
}

struct Fixture {
    bytes: Vec<u8>,
    following: Vec<Vec<u8>>,
    maintenance: Vec<u8>,
    header: Vec<u8>,
    signature: [u8; 64],
    proof: Proof,
    maintenance_proof: Proof,
    authorised: AuthorizedBatch,
    authorization: SequencerAuthorization,
}
impl Fixture {
    fn new(count: u64) -> Self {
        let key = SigningKey::from_bytes(&[3; 32]);
        let mut fields = fields();
        fields.batch_id = must(program_execution_batch_id(
            [2; 32],
            [8; 32],
            9,
            8 + count,
            7,
        ));
        let unsigned = encode_fields_version(&fields, None, PROTOCOL_VERSION);
        let signature = key.sign(&must(receipt_digest(&unsigned))).to_bytes();
        let bytes = encode_fields_version(&fields, Some(signature), PROTOCOL_VERSION);
        let header = header_bytes(7, [12; 32], [0; 32], count, key.verifying_key().to_bytes());
        let mut maintenance = maintenance::maintenance_bytes(&must(decode_batch_header(&header)));
        let end = maintenance.len();
        maintenance[end - 64..end - 32].copy_from_slice(&fields.resulting_state_root);
        let following = (1..count)
            .map(|index| {
                let mut following = fields.clone();
                following.sequence += index;
                following.activity_id = [must(u8::try_from(index + 1)); 32];
                following.previous_state_root = fields.resulting_state_root;
                let unsigned = encode_fields_version(&following, None, PROTOCOL_VERSION);
                let signature = key.sign(&must(receipt_digest(&unsigned))).to_bytes();
                encode_fields_version(&following, Some(signature), PROTOCOL_VERSION)
            })
            .collect();
        let mut result = Self {
            bytes,
            following,
            maintenance,
            header,
            signature: [0; 64],
            proof: must(Proof::new(0, 1, vec![])),
            maintenance_proof: must(Proof::new(0, 1, vec![])),
            authorised: AuthorizedBatch::new(
                fields.batch_id,
                fields.asset,
                [2; 32],
                [12; 32],
                key.verifying_key().to_bytes(),
            ),
            authorization: SequencerAuthorization::new(
                key.verifying_key().to_bytes(),
                key.verifying_key().to_bytes(),
                7,
                7,
            ),
        };
        result.seal(count);
        result
    }
    fn seal(&mut self, count: u64) {
        let key = SigningKey::from_bytes(&[3; 32]);
        let mut leaves = vec![self.bytes.as_slice()];
        leaves.extend(self.following.iter().map(Vec::as_slice));
        leaves.push(&self.maintenance);
        let (proof, root) = must(build_proof(&leaves, 0));
        self.proof = proof;
        self.maintenance_proof = must(build_proof(&leaves, must(usize::try_from(count)))).0;
        self.header = header_bytes(7, [12; 32], root, count, key.verifying_key().to_bytes());
        self.signature = key
            .sign(&must(batch_header_digest(&self.header)))
            .to_bytes();
    }
    fn evidence(&self) -> MaintainedOutcomeEvidence<'_> {
        MaintainedOutcomeEvidence {
            header: &self.header,
            header_signature: &self.signature,
            activity_proof: &self.proof,
            maintenance: &self.maintenance,
            maintenance_proof: &self.maintenance_proof,
            authorization: &self.authorization,
        }
    }
    fn verify(&self) -> Result<layerx_proof::receipt::VerifiedReceipt, MaintainedOutcomeFailure> {
        let mut receipts = vec![self.bytes.clone()];
        receipts.extend(self.following.clone());
        verify_outcome_maintained_chain(&self.bytes, &self.authorised, &self.evidence(), &receipts)
    }
}
#[test]
fn maintained_transition_preserves_historical_endpoint_strength() {
    for count in [1, 2] {
        let fixture = Fixture::new(count);
        assert!(fixture.verify().is_ok());
        if count > 1 {
            assert_eq!(
                verify_outcome_maintained(&fixture.bytes, &fixture.authorised, &fixture.evidence()),
                Err(MaintainedOutcomeFailure::SequenceRange)
            );
        }
        assert_eq!(
            must(
                verify_outcome(&fixture.bytes, &fixture.authorised)
                    .err()
                    .ok_or("historical accepted")
            )
            .check,
            ReceiptCheck::ResultingStateRoot
        );
        let mut changed = Fixture::new(count);
        let end = changed.maintenance.len();
        changed.maintenance[end - 64] ^= 1;
        changed.seal(count);
        assert_eq!(
            changed.verify(),
            Err(MaintainedOutcomeFailure::Receipt(
                ReceiptCheck::ResultingStateRoot
            ))
        );
    }
}
#[test]
fn maintained_roots_batch_and_inclusion_are_mandatory() {
    for offset in [1, 64] {
        let mut fixture = Fixture::new(1);
        let end = fixture.maintenance.len();
        fixture.maintenance[end - offset] ^= 1;
        assert!(matches!(
            fixture.verify(),
            Err(MaintainedOutcomeFailure::Inclusion(InclusionError::Merkle(
                _
            )))
        ));
        fixture.seal(1);
        assert_eq!(
            fixture.verify(),
            Err(MaintainedOutcomeFailure::Receipt(
                ReceiptCheck::ResultingStateRoot
            ))
        );
    }
    let mut fixture = Fixture::new(1);
    fixture.maintenance[b"LXP/programs/occupancy-receipt/v2\0".len() + 7] ^= 1;
    fixture.seal(1);
    assert_eq!(
        fixture.verify(),
        Err(MaintainedOutcomeFailure::Receipt(ReceiptCheck::BatchId))
    );
    let mut fixture = Fixture::new(1);
    fixture.signature[0] ^= 1;
    assert_eq!(
        fixture.verify(),
        Err(MaintainedOutcomeFailure::Inclusion(
            InclusionError::HeaderSignature
        ))
    );
}

#[test]
fn maintained_chain_authenticates_each_selected_transition_and_every_record() {
    let fixture = Fixture::new(2);
    let receipts = vec![fixture.bytes.clone(), fixture.following[0].clone()];
    let leaves = [
        receipts[0].as_slice(),
        receipts[1].as_slice(),
        &fixture.maintenance,
    ];
    let second_proof = must(build_proof(&leaves, 1)).0;
    let evidence = MaintainedOutcomeEvidence {
        activity_proof: &second_proof,
        ..fixture.evidence()
    };
    let verified = must(verify_outcome_maintained_chain(
        &receipts[1],
        &fixture.authorised,
        &evidence,
        &receipts,
    ));
    let selected = verified
        .receipt()
        .protocol()
        .expect("selected protocol receipt");
    assert_eq!(selected.previous_state_root(), [3; 32]);
    assert_eq!(selected.resulting_state_root(), [3; 32]);
    for changed in [
        vec![receipts[0].clone()],
        vec![receipts[1].clone(), receipts[0].clone()],
        vec![receipts[0].clone(), receipts[0].clone()],
    ] {
        assert!(verify_outcome_maintained_chain(
            &receipts[1],
            &fixture.authorised,
            &evidence,
            &changed
        )
        .is_err());
    }
    let mut broken_signature = fixture.clone();
    let end = broken_signature.following[0].len();
    broken_signature.following[0][end - 1] ^= 1;
    broken_signature.seal(2);
    assert_eq!(
        broken_signature.verify(),
        Err(MaintainedOutcomeFailure::Receipt(
            ReceiptCheck::SequencerSignature
        ))
    );
    let mut disconnected = fixture;
    let decoded = must(layerx_wire::receipt::decode(&disconnected.bytes));
    let mut following = fields();
    following.sequence += 1;
    following.activity_id = [2; 32];
    following.previous_state_root = [99; 32];
    following.batch_id = decoded.protocol().expect("protocol receipt").batch_id();
    let unsigned = encode_fields_version(&following, None, PROTOCOL_VERSION);
    let signature = SigningKey::from_bytes(&[3; 32])
        .sign(&must(receipt_digest(&unsigned)))
        .to_bytes();
    disconnected.following[0] =
        encode_fields_version(&following, Some(signature), PROTOCOL_VERSION);
    disconnected.seal(2);
    assert_eq!(
        disconnected.verify(),
        Err(MaintainedOutcomeFailure::Receipt(
            ReceiptCheck::PreviousStateRoot
        ))
    );
}

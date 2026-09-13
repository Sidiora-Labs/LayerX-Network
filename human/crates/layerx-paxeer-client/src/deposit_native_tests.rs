use super::verify_native_credit_receipt;
use crate::{AttestedNativeCustodyCredit, NativeCustodyExpectation};
use ed25519_dalek::{Signer as _, SigningKey};
use layerx_proof::receipt::AuthorizedBatch;
use layerx_wire::receipt::decode;

const RECEIPT: &[u8] =
    include_bytes!("../../../../tests/fixtures/custody/native-credit-receipt/receipt.unsigned");
const CREDIT: &[u8] =
    include_bytes!("../../../../tests/fixtures/custody/native-credit-receipt/credit");
const PROFILE: &[u8] =
    include_bytes!("../../../../tests/fixtures/custody/native-credit-receipt/profile");

fn field<const N: usize>(bytes: &[u8], offset: usize) -> [u8; N] {
    bytes[offset..offset + N]
        .try_into()
        .unwrap_or_else(|_| panic!("fixture field"))
}

fn signed(unsigned: &[u8], key: &SigningKey) -> Vec<u8> {
    assert_eq!(unsigned.last(), Some(&0));
    let digest = layerx_wire::hash::receipt_digest(unsigned)
        .unwrap_or_else(|error| panic!("receipt digest: {error:?}"));
    let mut bytes = unsigned.to_vec();
    bytes.pop();
    bytes.push(1);
    bytes.extend_from_slice(&64_u32.to_be_bytes());
    bytes.extend_from_slice(&key.sign(&digest).to_bytes());
    bytes
}

#[test]
fn real_native_credit_receipt_binds_attestation_supply_and_authority() {
    let credit = AttestedNativeCustodyCredit::verify(
        PROFILE,
        CREDIT,
        NativeCustodyExpectation {
            network_id: u32::from_be_bytes(field(CREDIT, 37)),
            beneficiary: field(CREDIT, 107),
            owner_key: field(CREDIT, 139),
        },
    )
    .unwrap_or_else(|error| panic!("native attestation: {error:?}"));
    let receipt = decode(RECEIPT).unwrap_or_else(|error| panic!("native receipt: {error:?}"));
    let protocol = receipt
        .protocol()
        .unwrap_or_else(|| panic!("protocol receipt"));
    let key = SigningKey::from_bytes(&[0x73; 32]);
    let authorized = AuthorizedBatch::new(
        protocol.batch_id(),
        protocol.asset(),
        protocol.previous_state_root(),
        protocol.resulting_state_root(),
        key.verifying_key().to_bytes(),
    );
    let activity_id = protocol.activity_id();
    let reserve = field(PROFILE, 129);
    let canonical = signed(RECEIPT, &key);
    assert_eq!(
        verify_native_credit_receipt(&credit, &canonical, &authorized, activity_id, reserve),
        Ok(())
    );
    let mut wrong = activity_id;
    wrong[0] ^= 1;
    assert!(
        verify_native_credit_receipt(&credit, &canonical, &authorized, wrong, reserve).is_err()
    );
    let mut wrong_reserve = reserve;
    wrong_reserve[0] ^= 1;
    assert!(verify_native_credit_receipt(
        &credit,
        &canonical,
        &authorized,
        activity_id,
        wrong_reserve
    )
    .is_err());
    for effect in &protocol.effects()[1..] {
        let start = RECEIPT
            .windows(effect.body().len())
            .position(|bytes| bytes == effect.body())
            .unwrap_or_else(|| panic!("native event encoding"));
        for offset in 0..effect.body().len() {
            let mut altered = RECEIPT.to_vec();
            altered[start + offset] ^= 1;
            let signed_altered = signed(&altered, &key);
            assert!(
                verify_native_credit_receipt(
                    &credit,
                    &signed_altered,
                    &authorized,
                    activity_id,
                    reserve
                )
                .is_err(),
                "accepted altered event {} byte {offset}",
                effect.event_type()
            );
        }
    }
    let mismatched = AuthorizedBatch::new(
        authorized.batch_id(),
        authorized.asset(),
        [0x54; 32],
        authorized.resulting_state_root(),
        authorized.sequencer_public_key(),
    );
    assert!(
        verify_native_credit_receipt(&credit, &canonical, &mismatched, activity_id, reserve)
            .is_err()
    );
}

fn checked<T, E: std::fmt::Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("daemon credit evidence: {error:?}"))
}

#[test]
fn actual_daemon_credit_passes_signed_batch_and_custody_verification() {
    use layerx_proof::inclusion::SequencerAuthorization;
    use layerx_proof::merkle::decode_proof;
    use layerx_proof::receipt::{authorized_maintained_activity_batch, MaintainedOutcomeEvidence};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/fixtures/custody/daemon-credit-receipt");
    let read = |name: &str| checked(std::fs::read(root.join(name)));
    let receipt_bytes = read("credit.receipt");
    let header_bytes = read("header");
    let header = checked(layerx_wire::receipt::decode_batch_header(&header_bytes));
    let receipt = checked(decode(&receipt_bytes));
    let protocol = receipt
        .protocol()
        .unwrap_or_else(|| panic!("protocol credit"));
    let key = checked(read("sequencer.public").try_into());
    let batch = AuthorizedBatch::new(
        protocol.batch_id(),
        [0; 32],
        header.previous_state_root(),
        header.resulting_state_root(),
        key,
    );
    let authorization = SequencerAuthorization::new(header.sequencer_id(), key, 1, 1);
    let signature = checked(read("header.signature").try_into());
    let proof = checked(decode_proof(&read("receipt.proof")));
    let maintenance = read("maintenance.receipt");
    let maintenance_proof = checked(decode_proof(&read("maintenance.proof")));
    let evidence = MaintainedOutcomeEvidence {
        header: &header_bytes,
        header_signature: &signature,
        activity_proof: &proof,
        maintenance: &maintenance,
        maintenance_proof: &maintenance_proof,
        authorization: &authorization,
    };
    let activity_batch = checked(authorized_maintained_activity_batch(
        &receipt_bytes,
        &batch,
        &evidence,
    ));
    let payload = read("credit");
    let profile = read("profile");
    let credit = checked(AttestedNativeCustodyCredit::verify(
        &profile,
        &payload,
        NativeCustodyExpectation {
            network_id: header.network_id(),
            beneficiary: field(&payload, 107),
            owner_key: field(&payload, 139),
        },
    ));
    let activity_type = checked(layerx_types::payload::ActivityType::new(
        layerx_types::payload::ModuleId::Bridge,
        1,
    ));
    let registration = checked(layerx_types::payload::ModuleRegistration::new(
        layerx_types::payload::ModuleId::Bridge,
        &[activity_type],
    ));
    let registry = checked(layerx_types::payload::ModuleRegistry::new(&[registration]));
    let activity = checked(layerx_wire::activity::decode_signed(
        &read("activity"),
        &registry,
    ));
    assert_eq!(activity.payload(), payload);
    let activity_id = checked(layerx_wire::hash::activity_id(&activity));
    let reserve = field(&profile, 129);
    assert_eq!(
        verify_native_credit_receipt(
            &credit,
            &receipt_bytes,
            &activity_batch,
            activity_id,
            reserve
        ),
        Ok(())
    );
    let mut altered = receipt_bytes.clone();
    let last = altered.len() - 1;
    altered[last] ^= 1;
    assert!(
        verify_native_credit_receipt(&credit, &altered, &activity_batch, activity_id, reserve)
            .is_err()
    );
    assert!(authorized_maintained_activity_batch(&altered, &batch, &evidence).is_err());
}

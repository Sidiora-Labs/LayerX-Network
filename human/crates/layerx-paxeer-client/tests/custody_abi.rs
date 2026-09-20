//! Byte-for-byte conformance of `src/custody.rs` against an independent
//! encoder.
//!
//! Every expectation in this file comes from `tests/vectors/custody_abi.json`,
//! which `tests/vectors/generate_custody_abi.sh` writes from Foundry `cast`
//! (selectors, event topics, calldata, return tuples) and `sha256sum` (the
//! domain-separated identifiers defined by `modules/layerxcustody/types/ids.go`
//! and `layerxproof/verify/withdrawal.go`). The evidence arguments are the real
//! `bound-native-withdrawal` fixture and a real native state-proof vector.

use layerx_paxeer_client::custody::{
    base_units_from_wei, decode_asset, decode_bool, decode_claim, decode_claim_finalised,
    decode_claim_queued, decode_custody_release, decode_emergency_exit_executed, decode_status,
    deposit_calldata, deposit_token_calldata, exit_claim_id, exit_eligible_calldata,
    exit_recipient_message, exit_withdrawal_id, get_asset_calldata, get_claim_calldata,
    native_asset_id_calldata, native_value_wei, nullifier_status_calldata, unique_custody_log,
    wire_merkle_proof, withdrawal_claim_id, withdrawal_nullifier, ClaimFinalised, ClaimQueued,
    CustodyAbiError, CustodyAsset, CustodyClaim, CustodyRelease, EmergencyExitExecuted,
    ForcedExitMaterial, WithdrawalMaterial, CLAIM_FINALISED_TOPIC, CLAIM_QUEUED_TOPIC,
    CUSTODY_DEPOSIT_TOPIC, CUSTODY_PRECOMPILE, CUSTODY_RELEASE_TOPIC,
    EMERGENCY_EXIT_EXECUTED_TOPIC, MAX_EVIDENCE_BYTES, SELECTOR_DEPOSIT, SELECTOR_DEPOSIT_TOKEN,
    SELECTOR_EXECUTE_FORCED_EXIT, SELECTOR_EXIT_ELIGIBLE, SELECTOR_FINALISE_WITHDRAWAL,
    SELECTOR_GET_ASSET, SELECTOR_GET_CLAIM, SELECTOR_NATIVE_ASSET_ID, SELECTOR_NULLIFIER_STATUS,
    SELECTOR_REQUEST_FORCED_EXIT, SELECTOR_REQUEST_WITHDRAWAL, WEI_PER_BASE_UNIT,
};
use layerx_paxeer_client::{parse_json, wire, Json, LogRecord};
use layerx_types::intent::EvmAddress;
use sha2::{Digest as _, Sha256};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const VECTORS: &str = include_str!("vectors/custody_abi.json");
const FIXTURE_RECEIPT: &[u8] =
    include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/receipt");
const FIXTURE_PROOF: &[u8] =
    include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/receipt.proof");
const FIXTURE_HEADER: &[u8] =
    include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/header");
const FIXTURE_HEADER_SIGNATURE: &[u8; 64] =
    include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/header.signature");

fn document() -> Result<Json, Box<dyn std::error::Error>> {
    parse_json(VECTORS).map_err(|error| format!("custody_abi.json: {error:?}").into())
}

fn at<'a>(value: &'a Json, path: &str) -> Result<&'a Json, Box<dyn std::error::Error>> {
    let mut current = value;
    for step in path.split('.') {
        current = current
            .member(step)
            .ok_or_else(|| format!("custody_abi.json: missing {path}"))?;
    }
    Ok(current)
}

fn text(value: &Json, path: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(at(value, path)?
        .as_text()
        .ok_or_else(|| format!("custody_abi.json: {path} is not text"))?
        .to_owned())
}

fn hex(value: &Json, path: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let text = text(value, path)?;
    let digits = text
        .strip_prefix("0x")
        .ok_or_else(|| format!("custody_abi.json: {path} is not 0x-prefixed"))?;
    if digits.len() % 2 != 0 {
        return Err(format!("custody_abi.json: {path} has an odd hex length").into());
    }
    let mut bytes = Vec::with_capacity(digits.len() / 2);
    for index in (0..digits.len()).step_by(2) {
        bytes.push(u8::from_str_radix(
            digits.get(index..index + 2).ok_or("hex bounds")?,
            16,
        )?);
    }
    Ok(bytes)
}

fn word(value: &Json, path: &str) -> Result<[u8; 32], Box<dyn std::error::Error>> {
    hex(value, path)?
        .try_into()
        .map_err(|_| format!("custody_abi.json: {path} is not 32 bytes").into())
}

fn address(value: &Json, path: &str) -> Result<EvmAddress, Box<dyn std::error::Error>> {
    let bytes: [u8; 20] = hex(value, path)?
        .try_into()
        .map_err(|_| format!("custody_abi.json: {path} is not 20 bytes"))?;
    Ok(EvmAddress::new(bytes))
}

fn decimal(value: &Json, path: &str) -> Result<u128, Box<dyn std::error::Error>> {
    Ok(text(value, path)?.parse::<u128>()?)
}

fn count(value: &Json, path: &str) -> Result<u64, Box<dyn std::error::Error>> {
    let raw = at(value, path)?
        .as_integer()
        .ok_or_else(|| format!("custody_abi.json: {path} is not a number"))?;
    Ok(u64::try_from(raw)?)
}

fn truth(value: &Json, path: &str) -> Result<bool, Box<dyn std::error::Error>> {
    match at(value, path)? {
        Json::Bool(flag) => Ok(*flag),
        other => Err(format!("custody_abi.json: {path} is not a boolean: {other:?}").into()),
    }
}

fn fixture_proof() -> Result<layerx_proof::merkle::Proof, Box<dyn std::error::Error>> {
    let wire = layerx_wire::receipt::decode_merkle_proof(FIXTURE_PROOF)
        .map_err(|error| format!("{error:?}"))?;
    layerx_proof::merkle::Proof::new(
        wire.leaf_index(),
        wire.leaf_count(),
        wire.siblings().to_vec(),
    )
    .map_err(|error| format!("{error:?}").into())
}

fn fixture_material() -> Result<WithdrawalMaterial, Box<dyn std::error::Error>> {
    WithdrawalMaterial::from_inclusion(
        FIXTURE_RECEIPT.to_vec(),
        &fixture_proof()?,
        FIXTURE_HEADER.to_vec(),
        *FIXTURE_HEADER_SIGNATURE,
    )
    .map_err(|error| format!("{error:?}").into())
}

fn exit_material(document: &Json) -> Result<ForcedExitMaterial, Box<dyn std::error::Error>> {
    ForcedExitMaterial {
        witness: hex(document, "inputs.witness")?,
        batch_number: count(document, "inputs.batch_number")?,
        account: word(document, "inputs.account")?,
        asset_id: word(document, "inputs.asset_id")?,
        recipient: address(document, "inputs.recipient")?,
        recipient_signature: hex(document, "inputs.recipient_signature")?
            .try_into()
            .map_err(|_| "recipient_signature is not 64 bytes")?,
    }
    .validated()
    .map_err(|error| format!("{error:?}").into())
}

fn log(topics: Vec<[u8; 32]>, data: Vec<u8>) -> LogRecord {
    LogRecord {
        address: CUSTODY_PRECOMPILE,
        topics,
        data,
    }
}

#[test]
fn selectors_and_topics_match_the_independent_encoder() -> TestResult {
    let document = document()?;
    for (path, selector) in [
        ("selectors.deposit", SELECTOR_DEPOSIT),
        ("selectors.depositToken", SELECTOR_DEPOSIT_TOKEN),
        ("selectors.requestWithdrawal", SELECTOR_REQUEST_WITHDRAWAL),
        ("selectors.finaliseWithdrawal", SELECTOR_FINALISE_WITHDRAWAL),
        ("selectors.requestForcedExit", SELECTOR_REQUEST_FORCED_EXIT),
        ("selectors.executeForcedExit", SELECTOR_EXECUTE_FORCED_EXIT),
        ("selectors.getClaim", SELECTOR_GET_CLAIM),
        ("selectors.nullifierStatus", SELECTOR_NULLIFIER_STATUS),
        ("selectors.getAsset", SELECTOR_GET_ASSET),
        ("selectors.exitEligible", SELECTOR_EXIT_ELIGIBLE),
        ("selectors.nativeAssetId", SELECTOR_NATIVE_ASSET_ID),
    ] {
        assert_eq!(hex(&document, path)?, selector.to_vec(), "{path}");
    }
    for (path, topic) in [
        ("topics.CustodyDeposit", CUSTODY_DEPOSIT_TOPIC),
        ("topics.ClaimQueued", CLAIM_QUEUED_TOPIC),
        ("topics.ClaimFinalised", CLAIM_FINALISED_TOPIC),
        ("topics.CustodyRelease", CUSTODY_RELEASE_TOPIC),
        (
            "topics.EmergencyExitExecuted",
            EMERGENCY_EXIT_EXECUTED_TOPIC,
        ),
    ] {
        assert_eq!(word(&document, path)?, topic, "{path}");
    }
    assert_eq!(
        hex(&document, "custody_precompile")?,
        CUSTODY_PRECOMPILE.bytes().to_vec()
    );
    assert_eq!(decimal(&document, "wei_per_base_unit")?, WEI_PER_BASE_UNIT);
    Ok(())
}

#[test]
fn calldata_matches_the_independent_encoder() -> TestResult {
    let document = document()?;
    assert_eq!(
        deposit_calldata(word(&document, "inputs.beneficiary")?),
        hex(&document, "calldata.deposit")?
    );
    assert_eq!(
        deposit_token_calldata(
            address(&document, "inputs.pointer")?,
            decimal(&document, "inputs.token_amount")?,
            word(&document, "inputs.beneficiary")?,
        ),
        hex(&document, "calldata.depositToken")?
    );

    assert_eq!(hex(&document, "inputs.receipt")?, FIXTURE_RECEIPT.to_vec());
    assert_eq!(hex(&document, "inputs.proof")?, FIXTURE_PROOF.to_vec());
    assert_eq!(hex(&document, "inputs.header")?, FIXTURE_HEADER.to_vec());
    assert_eq!(
        hex(&document, "inputs.header_signature")?,
        FIXTURE_HEADER_SIGNATURE.to_vec()
    );
    let material = fixture_material()?;
    assert_eq!(
        material.request_calldata(),
        hex(&document, "calldata.requestWithdrawal")?
    );
    assert_eq!(
        material.finalise_calldata(),
        hex(&document, "calldata.finaliseWithdrawal")?
    );

    let exit = exit_material(&document)?;
    assert_eq!(
        exit.request_calldata(),
        hex(&document, "calldata.requestForcedExit")?
    );
    assert_eq!(
        exit.execute_calldata(),
        hex(&document, "calldata.executeForcedExit")?
    );

    assert_eq!(
        get_claim_calldata(word(&document, "inputs.claim_id")?),
        hex(&document, "calldata.getClaim")?
    );
    assert_eq!(
        nullifier_status_calldata(word(&document, "inputs.nullifier")?),
        hex(&document, "calldata.nullifierStatus")?
    );
    assert_eq!(
        get_asset_calldata(word(&document, "inputs.asset_id")?),
        hex(&document, "calldata.getAsset")?
    );
    assert_eq!(
        exit_eligible_calldata(),
        hex(&document, "calldata.exitEligible")?
    );
    assert_eq!(
        native_asset_id_calldata(),
        hex(&document, "calldata.nativeAssetId")?
    );
    Ok(())
}

#[test]
fn return_decoders_match_the_independent_encoder() -> TestResult {
    let document = document()?;

    let encoded = hex(&document, "returns.getClaim.encoded")?;
    let claim = decode_claim(&encoded).map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        claim,
        CustodyClaim {
            claim_id: word(&document, "inputs.claim_id")?,
            kind: u8::try_from(count(&document, "returns.getClaim.kind")?)?,
            status: u8::try_from(count(&document, "returns.getClaim.status")?)?,
            nullifier: word(&document, "inputs.nullifier")?,
            withdrawal_id: word(&document, "inputs.withdrawal_id")?,
            account: word(&document, "inputs.account")?,
            asset_id: word(&document, "inputs.asset_id")?,
            denom: text(&document, "inputs.denom")?,
            recipient: address(&document, "inputs.recipient")?,
            amount: decimal(&document, "inputs.amount")?,
            batch_number: count(&document, "inputs.batch_number")?,
            anchor: word(&document, "inputs.anchor")?,
            available_at: count(&document, "inputs.available_at")?,
        }
    );
    assert_eq!(
        decode_claim(&encoded[..encoded.len() - 1]),
        Err(CustodyAbiError::Layout("getClaim"))
    );
    assert_eq!(
        decode_claim(&encoded[32..]),
        Err(CustodyAbiError::Layout("getClaim"))
    );
    let mut oversized_kind = encoded.clone();
    oversized_kind[94] = 1;
    assert_eq!(
        decode_claim(&oversized_kind),
        Err(CustodyAbiError::Layout("claim.kind"))
    );
    let mut wrong_recipient = encoded.clone();
    wrong_recipient[32 + 8 * 32] = 1;
    assert_eq!(
        decode_claim(&wrong_recipient),
        Err(CustodyAbiError::Layout("claim.recipient"))
    );

    let encoded = hex(&document, "returns.getAsset.encoded")?;
    assert_eq!(
        decode_asset(&encoded).map_err(|error| format!("{error:?}"))?,
        CustodyAsset {
            asset_id: word(&document, "inputs.asset_id")?,
            denom: text(&document, "inputs.denom")?,
            pointer: address(&document, "inputs.pointer")?,
            enabled: truth(&document, "returns.getAsset.enabled")?,
            paused: truth(&document, "returns.getAsset.paused")?,
            minimum_deposit: decimal(&document, "returns.getAsset.minimum_deposit")?,
            custody_cap: decimal(&document, "returns.getAsset.custody_cap")?,
            custodied: decimal(&document, "returns.getAsset.custodied")?,
            released: decimal(&document, "returns.getAsset.released")?,
            pending: decimal(&document, "returns.getAsset.pending")?,
        }
    );
    let mut non_boolean = encoded.clone();
    non_boolean[32 + 4 * 32 - 1] = 2;
    assert_eq!(
        decode_asset(&non_boolean),
        Err(CustodyAbiError::Layout("asset.enabled"))
    );
    assert_eq!(
        decode_asset(&encoded[..32]),
        Err(CustodyAbiError::Layout("asset.assetId"))
    );
    assert_eq!(
        decode_asset(&encoded[..64]),
        Err(CustodyAbiError::Layout("asset.denom"))
    );

    let encoded = hex(&document, "returns.nullifierStatus.encoded")?;
    assert_eq!(
        decode_status(&encoded).map_err(|error| format!("{error:?}"))?,
        u8::try_from(count(&document, "returns.nullifierStatus.status")?)?
    );
    assert_eq!(
        decode_status(&encoded[..31]),
        Err(CustodyAbiError::Layout("status"))
    );
    let mut wide = encoded.clone();
    wide[30] = 1;
    assert_eq!(decode_status(&wide), Err(CustodyAbiError::Layout("status")));

    assert!(decode_bool(&hex(&document, "returns.exitEligible.true")?)
        .map_err(|error| format!("{error:?}"))?);
    assert!(!decode_bool(&hex(&document, "returns.exitEligible.false")?)
        .map_err(|error| format!("{error:?}"))?);
    let mut two = hex(&document, "returns.exitEligible.false")?;
    two[31] = 2;
    assert_eq!(decode_bool(&two), Err(CustodyAbiError::Layout("bool")));
    assert_eq!(
        decode_bool(&two[..16]),
        Err(CustodyAbiError::Layout("bool"))
    );

    assert_eq!(
        hex(&document, "returns.nativeAssetId")?,
        word(&document, "inputs.asset_id")?.to_vec()
    );
    Ok(())
}

#[test]
fn event_decoders_match_the_independent_encoder() -> TestResult {
    let document = document()?;
    let claim_id = word(&document, "inputs.claim_id")?;
    let nullifier = word(&document, "inputs.nullifier")?;
    let anchor = word(&document, "inputs.anchor")?;
    let account = word(&document, "inputs.account")?;
    let asset_id = word(&document, "inputs.asset_id")?;
    let recipient = address(&document, "inputs.recipient")?;
    let recipient_topic = word(&document, "inputs.recipient_topic")?;
    let amount = decimal(&document, "inputs.amount")?;
    let available_at = count(&document, "inputs.available_at")?;

    let queued = log(
        vec![CLAIM_QUEUED_TOPIC, claim_id, nullifier, anchor],
        hex(&document, "events.ClaimQueued.data")?,
    );
    assert_eq!(
        decode_claim_queued(&queued).map_err(|error| format!("{error:?}"))?,
        ClaimQueued {
            claim_id,
            nullifier,
            anchor,
            asset_id,
            recipient,
            amount,
            available_at,
        }
    );
    let mut foreign = queued.clone();
    foreign.address = EvmAddress::new([0x0c; 20]);
    assert_eq!(
        decode_claim_queued(&foreign),
        Err(CustodyAbiError::Layout("ClaimQueued"))
    );
    let mut short_topics = queued.clone();
    short_topics.topics.pop();
    assert_eq!(
        decode_claim_queued(&short_topics),
        Err(CustodyAbiError::Layout("ClaimQueued"))
    );
    let mut short_data = queued.clone();
    short_data.data.truncate(96);
    assert_eq!(
        decode_claim_queued(&short_data),
        Err(CustodyAbiError::Layout("ClaimQueued"))
    );
    let mut dirty_recipient = queued.clone();
    dirty_recipient.data[32] = 1;
    assert_eq!(
        decode_claim_queued(&dirty_recipient),
        Err(CustodyAbiError::Layout("ClaimQueued.recipient"))
    );

    let finalised = log(vec![CLAIM_FINALISED_TOPIC, claim_id, nullifier], Vec::new());
    assert_eq!(
        decode_claim_finalised(&finalised).map_err(|error| format!("{error:?}"))?,
        ClaimFinalised {
            claim_id,
            nullifier
        }
    );
    assert_eq!(
        hex(&document, "events.ClaimFinalised.data")?,
        Vec::<u8>::new()
    );
    let mut padded = finalised.clone();
    padded.data.push(0);
    assert_eq!(
        decode_claim_finalised(&padded),
        Err(CustodyAbiError::Layout("ClaimFinalised"))
    );

    let release = log(
        vec![CUSTODY_RELEASE_TOPIC, claim_id, asset_id, recipient_topic],
        hex(&document, "events.CustodyRelease.data")?,
    );
    assert_eq!(
        decode_custody_release(&release).map_err(|error| format!("{error:?}"))?,
        CustodyRelease {
            claim_id,
            asset_id,
            recipient,
            amount,
            settlement_module: CUSTODY_PRECOMPILE,
        }
    );
    let mut dirty_topic = release.clone();
    dirty_topic.topics[3][0] = 1;
    assert_eq!(
        decode_custody_release(&dirty_topic),
        Err(CustodyAbiError::Layout("CustodyRelease.recipient"))
    );

    let executed = log(
        vec![EMERGENCY_EXIT_EXECUTED_TOPIC, claim_id, nullifier, anchor],
        hex(&document, "events.EmergencyExitExecuted.data")?,
    );
    assert_eq!(
        decode_emergency_exit_executed(&executed).map_err(|error| format!("{error:?}"))?,
        EmergencyExitExecuted {
            claim_id,
            nullifier,
            anchor,
            account,
            asset_id,
            recipient,
            amount,
        }
    );
    let mut wide_amount = executed.clone();
    wide_amount.data[3 * 32] = 1;
    assert_eq!(
        decode_emergency_exit_executed(&wide_amount),
        Err(CustodyAbiError::Layout("EmergencyExitExecuted.amount"))
    );

    let logs = vec![queued.clone(), finalised.clone(), release.clone()];
    assert_eq!(
        unique_custody_log(&logs, CLAIM_FINALISED_TOPIC, "ClaimFinalised")
            .map_err(|error| format!("{error:?}"))?,
        &finalised
    );
    assert_eq!(
        unique_custody_log(&logs, EMERGENCY_EXIT_EXECUTED_TOPIC, "absent"),
        Err(CustodyAbiError::Layout("absent"))
    );
    let repeated = vec![queued.clone(), queued];
    assert_eq!(
        unique_custody_log(&repeated, CLAIM_QUEUED_TOPIC, "ClaimQueued"),
        Err(CustodyAbiError::Layout("ClaimQueued"))
    );
    Ok(())
}

#[test]
fn native_value_refuses_a_wei_remainder() -> TestResult {
    let document = document()?;
    let amount = decimal(&document, "inputs.native_amount")?;
    let wei = decimal(&document, "inputs.native_wei")?;
    assert_eq!(wei, amount * WEI_PER_BASE_UNIT);

    let encoded = native_value_wei(amount).map_err(|error| format!("{error:?}"))?;
    let mut expected = [0_u8; 32];
    expected[16..].copy_from_slice(&wei.to_be_bytes());
    assert_eq!(encoded, expected);
    assert_eq!(
        base_units_from_wei(&encoded).map_err(|error| format!("{error:?}"))?,
        amount
    );

    let mut remainder = encoded;
    remainder[31] |= 1;
    assert_eq!(
        base_units_from_wei(&remainder),
        Err(CustodyAbiError::WeiRemainder)
    );

    let mut wide = encoded;
    wide[15] = 1;
    assert_eq!(
        base_units_from_wei(&wide),
        Err(CustodyAbiError::AmountOverflow)
    );

    assert_eq!(
        native_value_wei(u128::MAX),
        Err(CustodyAbiError::AmountOverflow)
    );
    assert_eq!(
        base_units_from_wei(&native_value_wei(0).map_err(|error| format!("{error:?}"))?)
            .map_err(|error| format!("{error:?}"))?,
        0
    );
    Ok(())
}

#[test]
fn wire_merkle_proof_reproduces_the_canonical_wire_form() -> TestResult {
    let proof = fixture_proof()?;
    let encoded = wire_merkle_proof(&proof).map_err(|error| format!("{error:?}"))?;
    assert_eq!(encoded, FIXTURE_PROOF.to_vec());
    let decoded = layerx_wire::receipt::decode_merkle_proof(&encoded)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(decoded.leaf_index(), proof.leaf_index());
    assert_eq!(decoded.leaf_count(), proof.leaf_count());
    assert_eq!(decoded.siblings(), proof.siblings());
    assert_eq!(
        layerx_wire::receipt::encode_merkle_proof(&decoded)
            .map_err(|error| format!("{error:?}"))?,
        encoded
    );

    let leaves: Vec<&[u8]> = vec![
        b"custody-leaf-0",
        b"custody-leaf-1",
        b"custody-leaf-2",
        b"custody-leaf-3",
        b"custody-leaf-4",
    ];
    for index in 0..leaves.len() {
        let (built, root) = layerx_proof::merkle::build_proof(&leaves, index)
            .map_err(|error| format!("{error:?}"))?;
        layerx_proof::merkle::verify_path(leaves[index], &built, &root)
            .map_err(|error| format!("{error:?}"))?;
        let encoded = wire_merkle_proof(&built).map_err(|error| format!("{error:?}"))?;
        let decoded = layerx_wire::receipt::decode_merkle_proof(&encoded)
            .map_err(|error| format!("{error:?}"))?;
        let round_tripped = layerx_proof::merkle::Proof::new(
            decoded.leaf_index(),
            decoded.leaf_count(),
            decoded.siblings().to_vec(),
        )
        .map_err(|error| format!("{error:?}"))?;
        layerx_proof::merkle::verify_path(leaves[index], &round_tripped, &root)
            .map_err(|error| format!("{error:?}"))?;
    }
    Ok(())
}

#[test]
fn withdrawal_material_binds_the_real_inclusion_fixture() -> TestResult {
    let material = fixture_material()?;
    assert_eq!(material.receipt, FIXTURE_RECEIPT.to_vec());
    assert_eq!(material.proof, FIXTURE_PROOF.to_vec());
    assert_eq!(material.header, FIXTURE_HEADER.to_vec());
    assert_eq!(&material.header_signature, FIXTURE_HEADER_SIGNATURE);

    let mut empty_receipt = material.clone();
    empty_receipt.receipt.clear();
    assert_eq!(
        empty_receipt.validated(),
        Err(CustodyAbiError::EvidenceBounds("receipt"))
    );

    let mut empty_proof = material.clone();
    empty_proof.proof.clear();
    assert_eq!(
        empty_proof.validated(),
        Err(CustodyAbiError::EvidenceBounds("proof"))
    );

    let mut oversized = material.clone();
    oversized.header = vec![0; MAX_EVIDENCE_BYTES + 1];
    assert_eq!(
        oversized.validated(),
        Err(CustodyAbiError::EvidenceBounds("header"))
    );

    let mut unsigned = material.clone();
    unsigned.header_signature = [0; 64];
    assert_eq!(
        unsigned.validated(),
        Err(CustodyAbiError::EvidenceBounds("header_signature"))
    );

    assert_eq!(
        WithdrawalMaterial::from_inclusion(
            Vec::new(),
            &fixture_proof()?,
            FIXTURE_HEADER.to_vec(),
            *FIXTURE_HEADER_SIGNATURE,
        ),
        Err(CustodyAbiError::EvidenceBounds("receipt"))
    );
    Ok(())
}

#[test]
fn forced_exit_material_refuses_every_empty_binding() -> TestResult {
    let document = document()?;
    let material = exit_material(&document)?;

    let mut empty_witness = material.clone();
    empty_witness.witness.clear();
    assert_eq!(
        empty_witness.validated(),
        Err(CustodyAbiError::EvidenceBounds("witness"))
    );

    let mut zero_batch = material.clone();
    zero_batch.batch_number = 0;
    assert_eq!(
        zero_batch.validated(),
        Err(CustodyAbiError::EvidenceBounds("batch_number"))
    );

    let mut empty_account = material.clone();
    empty_account.account = [0; 32];
    assert_eq!(
        empty_account.validated(),
        Err(CustodyAbiError::EvidenceBounds("account"))
    );

    let mut empty_asset = material.clone();
    empty_asset.asset_id = [0; 32];
    assert_eq!(
        empty_asset.validated(),
        Err(CustodyAbiError::EvidenceBounds("asset_id"))
    );

    let mut empty_recipient = material.clone();
    empty_recipient.recipient = EvmAddress::new([0; 20]);
    assert_eq!(
        empty_recipient.validated(),
        Err(CustodyAbiError::EvidenceBounds("recipient"))
    );

    let mut unsigned = material.clone();
    unsigned.recipient_signature = [0; 64];
    assert_eq!(
        unsigned.validated(),
        Err(CustodyAbiError::EvidenceBounds("recipient_signature"))
    );

    let mut oversized = material;
    oversized.witness = vec![7; MAX_EVIDENCE_BYTES + 1];
    assert_eq!(
        oversized.validated(),
        Err(CustodyAbiError::EvidenceBounds("witness"))
    );
    Ok(())
}

#[test]
fn material_wire_codecs_round_trip_and_refuse_noncanonical_bytes() -> TestResult {
    let document = document()?;
    let material = fixture_material()?;
    let encoded =
        wire::encode_withdrawal_material(&material, 1 << 20).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        wire::decode_withdrawal_material(&encoded, 1 << 20).map_err(|e| format!("{e:?}"))?,
        material
    );
    for length in 0..encoded.len() {
        assert!(wire::decode_withdrawal_material(&encoded[..length], 1 << 20).is_err());
    }
    let mut trailing = encoded.clone();
    trailing.push(0);
    assert!(wire::decode_withdrawal_material(&trailing, 1 << 20).is_err());
    let mut wrong_tag = encoded.clone();
    wrong_tag[1] ^= 0xff;
    assert!(wire::decode_withdrawal_material(&wrong_tag, 1 << 20).is_err());
    assert!(wire::decode_withdrawal_material(&encoded, encoded.len() - 1).is_err());
    assert!(wire::encode_withdrawal_material(&material, encoded.len() - 1).is_err());
    let mut unsigned = material;
    unsigned.header_signature = [0; 64];
    assert!(wire::encode_withdrawal_material(&unsigned, 1 << 20).is_err());

    let exit = exit_material(&document)?;
    let encoded =
        wire::encode_forced_exit_material(&exit, 1 << 20).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        wire::decode_forced_exit_material(&encoded, 1 << 20).map_err(|e| format!("{e:?}"))?,
        exit
    );
    for length in 0..encoded.len() {
        assert!(wire::decode_forced_exit_material(&encoded[..length], 1 << 20).is_err());
    }
    let mut trailing = encoded.clone();
    trailing.push(0);
    assert!(wire::decode_forced_exit_material(&trailing, 1 << 20).is_err());
    let mut wrong_tag = encoded.clone();
    wrong_tag[1] ^= 0xff;
    assert!(wire::decode_forced_exit_material(&wrong_tag, 1 << 20).is_err());
    assert!(wire::decode_forced_exit_material(&encoded, encoded.len() - 1).is_err());
    assert!(wire::encode_forced_exit_material(&exit, encoded.len() - 1).is_err());
    let mut zero_batch = exit;
    zero_batch.batch_number = 0;
    assert!(wire::encode_forced_exit_material(&zero_batch, 1 << 20).is_err());
    Ok(())
}

#[test]
fn identifier_formulas_match_the_go_definitions() -> TestResult {
    let document = document()?;
    let chain_id = count(&document, "chain_id")?;
    let network_id = u32::try_from(count(&document, "network_id")?)?;
    let account = word(&document, "inputs.account")?;
    let asset_id = word(&document, "inputs.asset_id")?;
    let anchor = word(&document, "inputs.anchor")?;
    let nullifier = word(&document, "inputs.nullifier")?;
    let withdrawal_id = word(&document, "inputs.withdrawal_id")?;
    let recipient = address(&document, "inputs.recipient")?;
    let amount = decimal(&document, "inputs.amount")?;

    for path in [
        "ids.withdrawal_claim_id",
        "ids.exit_claim_id",
        "ids.withdrawal_nullifier",
        "ids.exit_withdrawal_id",
    ] {
        let preimage = hex(&document, &format!("{path}.preimage"))?;
        let digest: [u8; 32] = Sha256::digest(&preimage).into();
        assert_eq!(
            digest,
            word(&document, &format!("{path}.digest"))?,
            "{path}: recorded digest is not the sha256 of the recorded preimage"
        );
    }

    assert_eq!(
        withdrawal_claim_id(chain_id, nullifier, recipient),
        word(&document, "ids.withdrawal_claim_id.digest")?
    );
    assert_eq!(
        exit_claim_id(chain_id, nullifier),
        word(&document, "ids.exit_claim_id.digest")?
    );
    assert_eq!(
        withdrawal_nullifier(
            network_id,
            &withdrawal_id,
            &account,
            &asset_id,
            amount,
            &anchor
        ),
        word(&document, "ids.withdrawal_nullifier.digest")?
    );
    assert_eq!(
        exit_withdrawal_id(network_id, &account, &asset_id, &anchor),
        word(&document, "ids.exit_withdrawal_id.digest")?
    );
    assert_eq!(
        exit_recipient_message(network_id, &account, &asset_id, recipient, &anchor),
        hex(&document, "ids.exit_recipient_message.message")?
    );

    assert_eq!(
        text(&document, "ids.withdrawal_claim_id.domain")?,
        "LXP/Paxeer/withdrawal-claim/v1"
    );
    assert_eq!(
        text(&document, "ids.exit_claim_id.domain")?,
        "LXP/Paxeer/emergency-exit/v1"
    );
    assert_eq!(
        text(&document, "ids.withdrawal_nullifier.domain")?,
        "LX:WITHDRAWAL:v1"
    );
    assert_eq!(
        hex(&document, "ids.exit_withdrawal_id.domain")?,
        b"LXP/v1/emergency-withdrawal-id\x00".to_vec()
    );
    assert_eq!(
        hex(&document, "ids.exit_recipient_message.domain")?,
        b"LX:SETTLE:RECIPIENT:v1\x00".to_vec()
    );

    for (path, head) in [
        ("ids.withdrawal_claim_id.preimage", 0xa0_u8),
        ("ids.exit_claim_id.preimage", 0x80),
    ] {
        let mut expected = [0_u8; 32];
        expected[31] = head;
        assert_eq!(
            hex(&document, path)?.get(..32),
            Some(&expected[..]),
            "{path}: the abi.encode head offset word"
        );
    }

    let mut changed_recipient = recipient.bytes();
    changed_recipient[19] ^= 1;
    assert_ne!(
        withdrawal_claim_id(chain_id, nullifier, EvmAddress::new(changed_recipient)),
        withdrawal_claim_id(chain_id, nullifier, recipient)
    );
    assert_ne!(
        exit_claim_id(chain_id + 1, nullifier),
        exit_claim_id(chain_id, nullifier)
    );
    assert_ne!(
        withdrawal_nullifier(
            network_id,
            &withdrawal_id,
            &account,
            &asset_id,
            amount + 1,
            &anchor
        ),
        withdrawal_nullifier(
            network_id,
            &withdrawal_id,
            &account,
            &asset_id,
            amount,
            &anchor
        )
    );
    assert_ne!(
        exit_withdrawal_id(network_id + 1, &account, &asset_id, &anchor),
        exit_withdrawal_id(network_id, &account, &asset_id, &anchor)
    );
    Ok(())
}

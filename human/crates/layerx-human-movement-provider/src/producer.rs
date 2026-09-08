use layerx_paxeer_client::{
    raw_call, CheckpointProof, EndpointConfig, ExecutionOutcome, FinalityStage, FinalityTracker,
    Json, TrackerConfig, TransactionHash,
};
use layerx_types::intent::EvmAddress;
use sha3::{Digest, Keccak256};

use crate::config::{hex, hex_string};
use crate::Error;

pub(crate) fn verify_checkpoint_registration(
    tracker: &TrackerConfig,
    registry: EvmAddress,
    proof: &CheckpointProof,
) -> Result<(), Error> {
    if registry.bytes() == [0; 20] {
        return Err(Error::Configuration);
    }
    let mut observations = Vec::new();
    for endpoint in &tracker.endpoints {
        if let Ok(observation) = registration(endpoint, registry, proof) {
            observations.push(observation);
        }
    }
    let observation = observations
        .iter()
        .find(|observation| {
            observations
                .iter()
                .filter(|other| other == observation)
                .count()
                >= tracker.minimum_endpoint_agreement
        })
        .ok_or(Error::Integrity)?;
    let mut finality = FinalityTracker::new(tracker.clone(), observation.transaction)
        .map_err(|_| Error::Configuration)?;
    let report = finality.poll();
    match report.stage() {
        FinalityStage::Final { inclusion, .. }
            if inclusion.execution == ExecutionOutcome::Succeeded
                && inclusion.block.number == observation.block_number
                && inclusion.block.hash == observation.block_hash =>
        {
            Ok(())
        }
        _ => Err(Error::Integrity),
    }
}

#[derive(Eq, PartialEq)]
struct Registration {
    transaction: TransactionHash,
    block_number: u64,
    block_hash: [u8; 32],
    log_index: u64,
}

fn registration(
    endpoint: &EndpointConfig,
    registry: EvmAddress,
    proof: &CheckpointProof,
) -> Result<Registration, Error> {
    let topic: [u8; 32] = Keccak256::digest(
        b"CheckpointRegistered(bytes32,uint64,uint64,uint64,uint64,bytes32,bytes32,bytes32,uint64)",
    )
    .into();
    let filter = Json::Object(vec![
        ("address".to_owned(), text_hex(&registry.bytes())),
        ("fromBlock".to_owned(), Json::Text("0x0".to_owned())),
        ("toBlock".to_owned(), Json::Text("latest".to_owned())),
        (
            "topics".to_owned(),
            Json::Array(vec![text_hex(&topic), text_hex(&proof.checkpoint_hash)]),
        ),
    ]);
    let response = raw_call(endpoint, "eth_getLogs", &[filter]).map_err(|_| Error::Integrity)?;
    let Json::Array(logs) = response else {
        return Err(Error::Integrity);
    };
    if logs.len() != 1 {
        return Err(Error::Integrity);
    }
    let log = logs.first().ok_or(Error::Integrity)?;
    let observed = decode_registration(log, registry, proof, topic)?;
    let receipt = raw_call(
        endpoint,
        "eth_getTransactionReceipt",
        &[Json::Text(observed.transaction.to_hex())],
    )
    .map_err(|_| Error::Integrity)?;
    if array_member::<32>(&receipt, "transactionHash")? != observed.transaction.bytes()
        || array_member::<32>(&receipt, "blockHash")? != observed.block_hash
        || quantity_member(&receipt, "blockNumber")? != observed.block_number
        || quantity_member(&receipt, "status")? != 1
    {
        return Err(Error::Integrity);
    }
    let Some(Json::Array(receipt_logs)) = receipt.member("logs") else {
        return Err(Error::Integrity);
    };
    let matching = receipt_logs.iter().filter(|candidate| {
        decode_registration(candidate, registry, proof, topic).is_ok_and(|value| value == observed)
    });
    if matching.count() != 1 {
        return Err(Error::Integrity);
    }
    let block = raw_call(
        endpoint,
        "eth_getBlockByNumber",
        &[
            Json::Text(format!("0x{:x}", observed.block_number)),
            Json::Bool(false),
        ],
    )
    .map_err(|_| Error::Integrity)?;
    if array_member::<32>(&block, "hash")? != observed.block_hash
        || quantity_member(&block, "number")? != observed.block_number
    {
        return Err(Error::Integrity);
    }
    Ok(observed)
}

fn decode_registration(
    log: &Json,
    registry: EvmAddress,
    proof: &CheckpointProof,
    topic: [u8; 32],
) -> Result<Registration, Error> {
    let Some(Json::Array(topics)) = log.member("topics") else {
        return Err(Error::Integrity);
    };
    let mut epoch = [0; 32];
    epoch[24..].copy_from_slice(&proof.epoch.to_be_bytes());
    let mut batch = [0; 32];
    batch[24..].copy_from_slice(&proof.batch_number.to_be_bytes());
    let expected = [topic, proof.checkpoint_hash, epoch, batch];
    if topics.len() != expected.len()
        || topics.iter().zip(expected).any(|(actual, expected)| {
            actual.as_text().and_then(|text| hex::<32>(text).ok()) != Some(expected)
        })
        || array_member::<20>(log, "address")? != registry.bytes()
        || log.member("removed") != Some(&Json::Bool(false))
    {
        return Err(Error::Integrity);
    }
    let data = array_member::<192>(log, "data")?;
    if data[96..128] != proof.state_root
        || data[128..160] != proof.data_availability_root
        || data[..24] != [0; 24]
        || data[32..56] != [0; 24]
        || data[160..184] != [0; 24]
    {
        return Err(Error::Integrity);
    }
    Ok(Registration {
        transaction: TransactionHash::new(array_member(log, "transactionHash")?),
        block_number: quantity_member(log, "blockNumber")?,
        block_hash: array_member(log, "blockHash")?,
        log_index: quantity_member(log, "logIndex")?,
    })
}

fn array_member<const N: usize>(value: &Json, field: &str) -> Result<[u8; N], Error> {
    let text = value
        .member(field)
        .and_then(Json::as_text)
        .ok_or(Error::Integrity)?;
    hex(text).map_err(|_| Error::Integrity)
}

fn quantity_member(value: &Json, field: &str) -> Result<u64, Error> {
    let digits = value
        .member(field)
        .and_then(Json::as_text)
        .and_then(|text| text.strip_prefix("0x"))
        .ok_or(Error::Integrity)?;
    if digits.is_empty() || (digits.len() > 1 && digits.starts_with('0')) {
        return Err(Error::Integrity);
    }
    u64::from_str_radix(digits, 16).map_err(|_| Error::Integrity)
}

fn text_hex(bytes: &[u8]) -> Json {
    Json::Text(format!("0x{}", hex_string(bytes)))
}

#[cfg(test)]
mod tests {
    use super::{decode_registration, text_hex};
    use layerx_paxeer_client::Json;
    use layerx_types::intent::EvmAddress;
    use sha3::{Digest, Keccak256};

    #[test]
    fn checkpoint_event_abi_requires_exact_roots_topics_and_canonical_log(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let proof = crate::tests::signed_checkpoint(2)?;
        let registry = EvmAddress::new([12; 20]);
        let topic: [u8; 32] = Keccak256::digest(
            b"CheckpointRegistered(bytes32,uint64,uint64,uint64,uint64,bytes32,bytes32,bytes32,uint64)",
        ).into();
        let mut epoch = [0; 32];
        epoch[24..].copy_from_slice(&proof.epoch.to_be_bytes());
        let mut batch = [0; 32];
        batch[24..].copy_from_slice(&proof.batch_number.to_be_bytes());
        let mut data = [0; 192];
        data[31] = 1;
        data[63] = 1;
        data[64..96].copy_from_slice(&[4; 32]);
        data[96..128].copy_from_slice(&proof.state_root);
        data[128..160].copy_from_slice(&proof.data_availability_root);
        data[191] = 1;
        let fields = vec![
            ("address".to_owned(), text_hex(&registry.bytes())),
            ("removed".to_owned(), Json::Bool(false)),
            (
                "topics".to_owned(),
                Json::Array(vec![
                    text_hex(&topic),
                    text_hex(&proof.checkpoint_hash),
                    text_hex(&epoch),
                    text_hex(&batch),
                ]),
            ),
            ("data".to_owned(), text_hex(&data)),
            ("transactionHash".to_owned(), text_hex(&[6; 32])),
            ("blockHash".to_owned(), text_hex(&[7; 32])),
            ("blockNumber".to_owned(), Json::Text("0x42".to_owned())),
            ("logIndex".to_owned(), Json::Text("0x0".to_owned())),
        ];
        let registration =
            decode_registration(&Json::Object(fields.clone()), registry, &proof, topic)?;
        assert_eq!(registration.transaction.bytes(), [6; 32]);
        assert_eq!(registration.block_number, 66);
        for (field, replacement) in [
            ("removed", Json::Bool(true)),
            ("address", text_hex(&[13; 20])),
            ("topics", Json::Array(vec![text_hex(&topic)])),
            ("data", text_hex(&data[..160])),
            ("blockNumber", Json::Text("0x042".to_owned())),
        ] {
            let mut altered = fields.clone();
            let value = altered
                .iter_mut()
                .find(|(name, _)| name == field)
                .ok_or("missing field")?;
            value.1 = replacement;
            assert!(decode_registration(&Json::Object(altered), registry, &proof, topic).is_err());
        }
        for offset in [96, 128] {
            let mut altered = fields.clone();
            let mut wrong_root = data;
            wrong_root[offset] ^= 1;
            let value = altered
                .iter_mut()
                .find(|(name, _)| name == "data")
                .ok_or("missing data")?;
            value.1 = text_hex(&wrong_root);
            assert!(decode_registration(&Json::Object(altered), registry, &proof, topic).is_err());
        }
        Ok(())
    }
}

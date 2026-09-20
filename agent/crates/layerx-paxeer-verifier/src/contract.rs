use layerx_proof::checkpoint::Certificate;
use layerx_wire::receipt::BatchHeader;

use crate::abi::{call, dynamic, word};
use crate::checkpoint::SettlementReference;
use crate::encoding::{bytes, hex, invalid};
use crate::{raw_call, BlockAnchor, EndpointFault, Json, PaxeerCheckpointPolicy, Publication};

/// `CheckpointSubmitted(uint64,bytes32,bytes32,bytes32,uint8)` on layerxAnchor.
const SUBMITTED_TOPIC: [u8; 32] = [
    0xf7, 0x32, 0xef, 0xc9, 0xdf, 0x2e, 0x75, 0x89, 0x89, 0x88, 0x99, 0xf8, 0x5c, 0xa5, 0xe6, 0xcb,
    0x25, 0xfa, 0x1c, 0x61, 0x94, 0x61, 0xd9, 0x6e, 0x81, 0xe8, 0x2b, 0xe9, 0xcc, 0xf1, 0x44, 0x16,
];
const STATUS_FINAL: u64 = 2;

/// Checks a certificate against the checkpoint layerxAnchor recorded for it:
/// the submission transaction the settlement reference names, its event, and
/// the anchor's own record, guarantor set, threshold and finality read at the
/// confirmed head. `policy.registry` is the layerxAnchor address.
pub(crate) fn verify(
    policy: &PaxeerCheckpointPolicy,
    certificate: &Certificate,
    header: &BatchHeader,
    identifier: [u8; 32],
    reference: SettlementReference,
) -> Result<Publication, EndpointFault> {
    let published = crate::publication_at(
        &policy.endpoint,
        policy.registry,
        SUBMITTED_TOPIC,
        2,
        identifier,
        policy.confirmations,
    )
    .map_err(|error| error.fault)?;
    if published.transaction_hash != reference.transaction_id
        || published.registration.number != reference.block_number
        || published.registration.timestamp.checked_mul(1_000) != Some(reference.observed_at_ms)
    {
        return Err(invalid());
    }
    bind_submission(&published, certificate)?;
    let signers = u64::try_from(certificate.attestations().len()).map_err(|_| invalid())?;
    if published.topics != [SUBMITTED_TOPIC, word(header.batch_number()), identifier]
        || published.data
            != [
                header.resulting_state_root(),
                header.receipt_merkle_root(),
                word(signers),
            ]
            .concat()
    {
        return Err(invalid());
    }
    let reader = Reader {
        policy,
        anchor: published.confirmed_head,
    };
    let batch = word(header.batch_number());
    reader.require(
        "threshold()",
        &[],
        &word(u64::try_from(certificate.threshold()).map_err(|_| invalid())?),
    )?;
    reader.require(
        "checkpointBatch(bytes32)",
        &[identifier],
        &[batch, word(STATUS_FINAL)].concat(),
    )?;
    reader.require(
        "finalizedStateRoot(uint64)",
        &[batch],
        &[header.resulting_state_root(), word(1)].concat(),
    )?;
    let record = reader.read(&call("checkpoint(uint64)", &[batch]))?;
    let expected = [
        (0, batch),
        (1, identifier),
        (3, word(header.epoch())),
        (4, word(header.first_sequence())),
        (5, word(header.last_sequence())),
        (6, header.previous_state_root()),
        (7, header.resulting_state_root()),
        (8, header.receipt_merkle_root()),
        (9, header.data_availability_root()),
        (10, header.sequencer_id()),
        (11, word(header.timestamp_ms())),
        (12, word(STATUS_FINAL)),
        (13, word(signers)),
        (16, word(published.registration.number)),
    ];
    if record.len() != 18 * 32
        || expected
            .iter()
            .any(|(index, value)| record[index * 32..(index + 1) * 32] != value[..])
    {
        return Err(invalid());
    }
    let mut attested = certificate
        .attestations()
        .iter()
        .map(|attestation| attestation.guarantor_id())
        .collect::<Vec<_>>();
    attested.sort_unstable();
    let mut recorded = word(32).to_vec();
    recorded.extend_from_slice(&word(signers));
    for guarantor in &attested {
        recorded.extend_from_slice(guarantor);
    }
    if attested.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(invalid());
    }
    reader.require("checkpointGuarantors(uint64)", &[batch], &recorded)?;
    reader.confirm(published.registration)?;
    reader.confirm(published.confirmed_head)?;
    Ok(published)
}

/// The submission is `submitCheckpoint(bytes header, bytes headerSignature,
/// bytes certificate)` carrying exactly the certificate's header.
fn bind_submission(
    published: &Publication,
    certificate: &Certificate,
) -> Result<(), EndpointFault> {
    let selector = call("submitCheckpoint(bytes,bytes,bytes)", &[]);
    let args = published
        .input
        .strip_prefix(&selector[..])
        .ok_or_else(invalid)?;
    if dynamic(args, 3, 0)? != certificate.checkpoint().header_bytes()
        || dynamic(args, 3, 1)?.len() != 64
        || dynamic(args, 3, 2)?.is_empty()
    {
        return Err(invalid());
    }
    Ok(())
}

struct Reader<'a> {
    policy: &'a PaxeerCheckpointPolicy,
    anchor: BlockAnchor,
}

impl Reader<'_> {
    fn read(&self, calldata: &[u8]) -> Result<Vec<u8>, EndpointFault> {
        let value = raw_call(
            &self.policy.endpoint,
            "eth_call",
            &[
                Json::Object(vec![
                    ("to".into(), Json::Text(hex(&self.policy.registry.bytes()))),
                    ("data".into(), Json::Text(hex(calldata))),
                ]),
                Json::Text(format!("0x{:x}", self.anchor.number)),
            ],
        )
        .map_err(|error| error.fault)?;
        bytes(&value)
    }

    fn require(
        &self,
        signature: &str,
        words: &[[u8; 32]],
        expected: &[u8],
    ) -> Result<(), EndpointFault> {
        if self.read(&call(signature, words))? != expected {
            return Err(invalid());
        }
        Ok(())
    }

    fn confirm(&self, expected: BlockAnchor) -> Result<(), EndpointFault> {
        let block = raw_call(
            &self.policy.endpoint,
            "eth_getBlockByNumber",
            &[
                Json::Text(format!("0x{:x}", expected.number)),
                Json::Bool(false),
            ],
        )
        .map_err(|error| error.fault)?;
        if BlockAnchor::decode(&block)? != expected {
            return Err(invalid());
        }
        Ok(())
    }
}

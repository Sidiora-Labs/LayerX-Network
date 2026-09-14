use layerx_proof::checkpoint::Certificate;
use layerx_wire::receipt::BatchHeader;

use crate::abi::{address, call, registered_certificate, registration, word};
use crate::checkpoint::SettlementReference;
use crate::encoding::{bytes, hex, invalid};
use crate::{raw_call, BlockAnchor, EndpointFault, Json, PaxeerCheckpointPolicy, Publication};

const REGISTERED_TOPIC: [u8; 32] = [
    0x09, 0x4d, 0x06, 0x13, 0x2b, 0xe9, 0x0f, 0x15, 0x44, 0xeb, 0xa6, 0x3f, 0xf4, 0xd5, 0x0f, 0xf3,
    0x21, 0x69, 0x50, 0xfc, 0xa4, 0x91, 0x2b, 0x3d, 0x46, 0x9d, 0x48, 0x2f, 0xbf, 0x88, 0x26, 0x1c,
];

pub(crate) fn verify(
    policy: &PaxeerCheckpointPolicy,
    certificate: &Certificate,
    header: &BatchHeader,
    identifier: [u8; 32],
    set_version: u64,
    reference: SettlementReference,
) -> Result<Publication, EndpointFault> {
    let published = crate::publication(
        &policy.endpoint,
        policy.registry,
        REGISTERED_TOPIC,
        identifier,
        policy.confirmations,
    )
    .map_err(|error| error.fault)?;
    if published.transaction_hash != reference.transaction_id
        || published.registration.number != reference.block_number
        || published.registration.timestamp.checked_mul(1_000) != Some(reference.observed_at_ms)
        || published.input != registration(certificate, header)
    {
        return Err(invalid());
    }
    bind_event(&published, header, identifier, set_version)?;
    let reader = Reader {
        policy,
        anchor: published.confirmed_head,
    };
    reader.bind_policy(certificate.threshold())?;
    reader.require(
        "registeredAt(bytes32)",
        &[identifier],
        word(published.registration.timestamp),
    )?;
    reader.require(
        "checkpointGuarantorSetVersion(bytes32)",
        &[identifier],
        word(set_version),
    )?;
    reader.require(
        "checkpointTimestamp(bytes32)",
        &[identifier],
        word(header.timestamp_ms()),
    )?;
    reader.require("isCanonicalCheckpoint(bytes32)", &[identifier], word(1))?;
    reader.require(
        "isFinalised(bytes32,bytes32)",
        &[identifier, header.resulting_state_root()],
        word(1),
    )?;
    if reader.read(&registered_certificate(certificate, header, identifier))? != word(1) {
        return Err(invalid());
    }
    reader.confirm(published.registration)?;
    reader.confirm(published.confirmed_head)?;
    Ok(published)
}

fn bind_event(
    published: &Publication,
    header: &BatchHeader,
    identifier: [u8; 32],
    set_version: u64,
) -> Result<(), EndpointFault> {
    if published.topics
        != [
            REGISTERED_TOPIC,
            identifier,
            word(header.epoch()),
            word(header.batch_number()),
        ]
        || published.data
            != [
                word(header.first_sequence()),
                word(header.last_sequence()),
                header.previous_state_root(),
                header.resulting_state_root(),
                header.data_availability_root(),
                word(set_version),
            ]
            .concat()
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
        expected: [u8; 32],
    ) -> Result<(), EndpointFault> {
        if self.read(&call(signature, words))? != expected {
            return Err(invalid());
        }
        Ok(())
    }

    fn bind_policy(&self, threshold: usize) -> Result<(), EndpointFault> {
        for (signature, expected) in [
            (
                "protocolVersion()",
                word(u64::from(self.policy.protocol_version)),
            ),
            ("networkId()", word(u64::from(self.policy.network_id))),
            (
                "genesisCanonicalStateRoot()",
                self.policy.canonical_genesis_root,
            ),
            (
                "guarantorEligibility()",
                address(self.policy.guarantor_bond.bytes()),
            ),
            (
                "threshold()",
                word(u64::try_from(threshold).map_err(|_| invalid())?),
            ),
        ] {
            self.require(signature, &[], expected)?;
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

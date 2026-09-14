use crate::evidence::{
    verify_module_evidence, verify_module_evidence_with_history, AccountEvidencePolicy,
};

use super::{
    decode_core_refusal, decode_envelope, encode_envelope, require_level, Envelope, FrameTransport,
    Freshness, ReadContext, ReadError, ReadValue, RootSelector, ACCOUNT_READ_REQUEST_TAG,
    ACCOUNT_READ_RESPONSE_TAG, ERROR_RESPONSE_TAG,
};

pub(super) fn read(
    transport: &mut dyn FrameTransport,
    module_id: u16,
    key: &[u8],
    context: ReadContext,
) -> Result<ReadValue, ReadError> {
    read_with_history(transport, module_id, key, context, None)
}

pub(super) fn read_with_history(
    transport: &mut dyn FrameTransport,
    module_id: u16,
    key: &[u8],
    context: ReadContext,
    history: Option<&crate::handover::SequencerHistory>,
) -> Result<ReadValue, ReadError> {
    if key.is_empty() || key.len() > 129 || module_id > 9 {
        return Err(ReadError::PageBound);
    }
    if context.expected_protocol_version != 3 || context.interface_version.minor < 5 {
        return Err(ReadError::UnavailableCapability);
    }
    let mut selector = 1_u16.to_be_bytes().to_vec();
    selector.push(4);
    selector.extend_from_slice(&module_id.to_be_bytes());
    selector.extend_from_slice(
        &u16::try_from(key.len())
            .map_err(|_| ReadError::PageBound)?
            .to_be_bytes(),
    );
    selector.extend_from_slice(key);
    context.root_selector.encode(&mut selector);
    selector.push(context.requested.level().wire_rank());
    transport.send(&encode_envelope(Envelope {
        version: context.interface_version,
        message_tag: ACCOUNT_READ_REQUEST_TAG,
        correlation_id: context.correlation_id,
        canonical_payload: &selector,
        proof_material: &[],
    })?)?;
    let bytes = transport.receive()?;
    let response = decode_envelope(&bytes)?;
    if response.version != context.interface_version
        || response.correlation_id != context.correlation_id
    {
        return Err(ReadError::UnexpectedResponse);
    }
    if response.message_tag == ERROR_RESPONSE_TAG && response.proof_material.is_empty() {
        let refusal =
            decode_core_refusal(response.canonical_payload).ok_or(ReadError::UnexpectedResponse)?;
        return Err(ReadError::CoreRefusal {
            class: refusal.class,
            result: refusal.result,
        });
    }
    if response.message_tag != ACCOUNT_READ_RESPONSE_TAG {
        return Err(ReadError::UnexpectedResponse);
    }
    let policy = AccountEvidencePolicy {
        root_selector: context.root_selector,
        expected_protocol_version: context.expected_protocol_version,
        expected_network_id: context.expected_network_id,
        handshake_sequencer_key: context.handshake_sequencer_key,
    };
    let verified = if let Some(history) = history {
        verify_module_evidence_with_history(
            response.canonical_payload,
            response.proof_material,
            module_id,
            key,
            policy,
            history,
        )
    } else {
        verify_module_evidence(
            response.canonical_payload,
            response.proof_material,
            module_id,
            key,
            policy,
        )
    }
    .map_err(ReadError::ProductionEvidence)?;
    if history.is_none()
        && verified.signed_header().response_authorization() != context.sequencer_authorization
    {
        return Err(ReadError::AuthorityRangeMismatch);
    }
    let header =
        layerx_wire::receipt::decode_batch_header(&verified.signed_header().canonical_bytes)
            .map_err(|_| ReadError::MalformedValue)?;
    if context.root_selector == RootSelector::Latest
        && (header.batch_number() != context.head.sealed_batch
            || header.last_sequence() != context.head.chain_sequence)
    {
        return Err(ReadError::HeadMismatch {
            expected_batch: context.head.sealed_batch,
            actual_batch: header.batch_number(),
        });
    }
    require_level(context.requested, verified.level())?;
    Ok(ReadValue {
        canonical_bytes: response.canonical_payload.to_vec(),
        proof_material: response.proof_material.to_vec(),
        achieved: verified.level(),
        freshness: Freshness {
            global_sequence: header.last_sequence(),
            batch_number: header.batch_number(),
            observed_head_sequence: context.head.chain_sequence,
            observed_checkpoint: verified
                .checkpoint_id()
                .unwrap_or(context.head.finalised_checkpoint),
        },
    })
}

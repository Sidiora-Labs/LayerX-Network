use super::{
    decode_batch_header, verify_receipt_inclusion, AuthorizedBatch, RawReceiptEvidence,
    ReceiptEvidenceError, SequencerAuthorization, VerifiedReceiptEvidence, VerifierPolicyError,
};
use layerx_proof::receipt::{
    authorized_maintained_activity_batch_chain, verify_native_owner_outcome,
    MaintainedOutcomeEvidence, NativeOwnerOutcomeContext,
};
use layerx_wire::hash::receipt_execution_batch_id;
use sha2::{Digest as _, Sha256};

impl VerifiedReceiptEvidence {
    pub(crate) fn verify_authorized_native_owner(
        raw: &RawReceiptEvidence,
        batch: &AuthorizedBatch,
        expected: &NativeOwnerOutcomeContext<'_>,
        maintained: Option<(&MaintainedOutcomeEvidence<'_>, &[Vec<u8>])>,
    ) -> Result<Self, ReceiptEvidenceError> {
        let header = decode_batch_header(raw.canonical_header())
            .map_err(|_| ReceiptEvidenceError::Policy(VerifierPolicyError::HeaderDecode))?;
        if header.protocol_version() != 3 || header.network_id() != expected.network_id {
            return Err(ReceiptEvidenceError::ProtocolVersion);
        }
        let authorization = SequencerAuthorization::new(
            header.sequencer_id(),
            batch.sequencer_public_key(),
            header.batch_number(),
            header.batch_number(),
        );
        let inclusion = verify_receipt_inclusion(
            raw.canonical_receipt(),
            raw.proof(),
            raw.canonical_header(),
            &raw.header_signature(),
            &authorization,
        )
        .map_err(ReceiptEvidenceError::Inclusion)?;
        let authenticated = if let Some((evidence, receipts)) = maintained {
            if evidence.header != raw.canonical_header()
                || evidence.header_signature != &raw.header_signature()
                || evidence.activity_proof != raw.proof()
                || evidence.authorization != &authorization
            {
                return Err(ReceiptEvidenceError::BatchIdentity);
            }
            let sealed = AuthorizedBatch::new(
                batch.batch_id(),
                batch.asset(),
                header.previous_state_root(),
                header.resulting_state_root(),
                batch.sequencer_public_key(),
            );
            authorized_maintained_activity_batch_chain(
                raw.canonical_receipt(),
                &sealed,
                evidence,
                receipts,
            )
            .map_err(|_| ReceiptEvidenceError::BatchIdentity)?
        } else {
            if header.first_sequence() != header.last_sequence()
                || header.previous_state_root() != batch.previous_state_root()
                || header.resulting_state_root() != batch.resulting_state_root()
            {
                return Err(ReceiptEvidenceError::BatchIdentity);
            }
            *batch
        };
        if authenticated != *batch {
            return Err(ReceiptEvidenceError::BatchIdentity);
        }
        let verified = verify_native_owner_outcome(raw.canonical_receipt(), batch, expected)
            .map_err(ReceiptEvidenceError::NativeOwner)?;
        let protocol = verified
            .receipt()
            .protocol()
            .ok_or(ReceiptEvidenceError::BatchIdentity)?;
        if protocol.global_sequence() < header.first_sequence()
            || protocol.global_sequence() > header.last_sequence()
        {
            return Err(ReceiptEvidenceError::SequenceRange);
        }
        if maintained.is_none()
            && receipt_execution_batch_id(protocol, &header)
                .map_err(|_| ReceiptEvidenceError::BatchIdentity)?
                != batch.batch_id()
        {
            return Err(ReceiptEvidenceError::BatchIdentity);
        }
        Ok(Self {
            receipt_ref: Sha256::digest(verified.canonical_bytes()).into(),
            activity_id: protocol.activity_id(),
            global_sequence: protocol.global_sequence(),
            result_code: protocol.result_code(),
            amount: protocol.amount(),
            module_id: protocol.module_id(),
            operation: protocol.operation(),
            verified,
            inclusion,
        })
    }
}

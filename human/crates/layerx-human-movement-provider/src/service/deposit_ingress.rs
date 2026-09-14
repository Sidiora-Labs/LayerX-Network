use super::{proof_error, transaction_matches, deposit_calldata, EvidenceService};
use crate::{config::hex_string, journal::read_private};
use layerx_human_service::journeys::WalletCustodyRequest;
use layerx_paxeer_client::{AdmittedCustody, AttestedNativeCustodyCredit, CreditFault,
    DepositFailure, Json, NativeCustodyExpectation, NativeDepositAdmission, ProofFault,
    TransactionHash, raw_call};
use layerx_types::account::AccountId;

impl EvidenceService {
    pub(super) fn refresh_publication(&self, transaction: TransactionHash) -> Result<(), crate::Error> {
        use sha3::{Digest, Keccak256};
        let recipient = self.journal.deposit_recipient(transaction)?;
        let selector = Keccak256::digest(b"latestCanonicalCheckpointHash()");
        let call = Json::Object(vec![
            ("to".to_owned(), Json::Text(format!("0x{}", hex_string(&self.checkpoint_registry.bytes())))),
            ("data".to_owned(), Json::Text(format!("0x{}", hex_string(&selector[..4])))),
        ]);
        let mut checkpoints = Vec::new();
        for endpoint in &self.tracker_config.endpoints {
            let Ok(Json::Text(value)) = raw_call(endpoint, "eth_call", &[call.clone(), Json::Text("latest".to_owned())])
            else { continue; };
            if let Ok(value) = crate::config::hex::<32>(&value) {
                if value != [0; 32] { checkpoints.push(value); }
            }
        }
        let checkpoint = checkpoints.iter().find(|value| checkpoints.iter().filter(|other| other == value).count()
            >= self.tracker_config.minimum_endpoint_agreement).ok_or(crate::Error::Integrity)?;
        crate::evidence_export::publish_registered(&self.tracker_config, &self.policy,
            self.vault, self.checkpoint_registry, &self.evidence_root,
            &crate::evidence_export::Request { transaction, checkpoint: *checkpoint, recipient })
    }

    pub(super) fn custody_for_request(&mut self, request: &WalletCustodyRequest,
        transaction: TransactionHash, recipient: &AccountId)
        -> Result<AdmittedCustody, DepositFailure>
    {
        if request.chain_id != self.chain_id || request.vault != self.vault
            || request.identity.account != request.beneficiary
            || request.identity.wallet != request.wallet
        { return Err(DepositFailure::CreditRefused(CreditFault::NativeBinding)); }
        let report = self.poll(transaction).map_err(|_| proof_error(ProofFault::MissingQuorumEvidence))?;
        let custody = self.verifier.admit_custody(&report, self.vault, recipient)?;
        let facts = custody.custody();
        if facts.payer != request.wallet || facts.asset != request.asset
            || facts.beneficiary != request.beneficiary || facts.amount != request.amount
        { return Err(DepositFailure::CreditRefused(CreditFault::NativeBinding)); }
        let input = deposit_calldata(request);
        let agreement = self.tracker_config.endpoints.iter().filter(|endpoint| {
            raw_call(endpoint, "eth_getTransactionByHash", &[Json::Text(transaction.to_hex())])
                .is_ok_and(|value| transaction_matches(&value, request, transaction,
                    custody.inclusion(), &input))
        }).count();
        if agreement < self.tracker_config.minimum_endpoint_agreement {
            return Err(proof_error(ProofFault::MissingQuorumEvidence));
        }
        Ok(custody)
    }

    pub(super) fn admit(&mut self, request: &WalletCustodyRequest,
        transaction: TransactionHash, recipient: &AccountId)
        -> Result<NativeDepositAdmission, DepositFailure>
    {
        let custody = self.custody_for_request(request, transaction, recipient)?;
        let credit = self.read_credit(transaction, custody.custody().beneficiary)?;
        NativeDepositAdmission::new(&custody, credit)
    }

    pub(super) fn read_credit(&self, transaction: TransactionHash, beneficiary: [u8; 32])
        -> Result<AttestedNativeCustodyCredit, DepositFailure>
    {
        let profile = self.custody_profile.as_ref()
            .ok_or_else(|| proof_error(ProofFault::EvidenceSourceMismatch))?;
        let path = self.evidence_root.join(format!("credit-{}.bin", hex_string(&transaction.bytes())));
        let payload = read_private(&path, 427)
            .map_err(|_| proof_error(ProofFault::ProducerUnavailable))?;
        let owner_key = payload.get(139..171)
            .ok_or_else(|| proof_error(ProofFault::EvidenceSourceMismatch))?
            .try_into().map_err(|_| proof_error(ProofFault::EvidenceSourceMismatch))?;
        AttestedNativeCustodyCredit::verify(profile, &payload,
            NativeCustodyExpectation { network_id: self.policy.layerx_network_id,
                beneficiary, owner_key })
            .map_err(|_| proof_error(ProofFault::EvidenceSourceMismatch))
    }
}

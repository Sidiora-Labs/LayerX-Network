use layerx_proof::checkpoint::{checkpoint_id, Certificate};
use layerx_types::intent::EvmAddress;
use layerx_wire::receipt::decode_batch_header;

use crate::encoding::{failure, invalid};
use crate::{
    canonical_endpoint_identity, BlockAnchor, EndpointConfig, EndpointFailure, EndpointFault,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaxeerCheckpointPolicy {
    pub endpoint: EndpointConfig,
    pub registry: EvmAddress,
    pub guarantor_bond: EvmAddress,
    pub protocol_version: u16,
    pub network_id: u32,
    pub canonical_genesis_root: [u8; 32],
    pub confirmations: u64,
}

#[derive(Clone, Debug)]
pub struct PaxeerCheckpointVerifier {
    policy: PaxeerCheckpointPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedCheckpointPublication {
    policy: PaxeerCheckpointPolicy,
    checkpoint_id: [u8; 32],
    canonical_header: Vec<u8>,
    settlement_reference: Vec<u8>,
    set_version: u64,
    registration: BlockAnchor,
    confirmed_head: BlockAnchor,
}

impl VerifiedCheckpointPublication {
    #[must_use]
    pub const fn policy(&self) -> &PaxeerCheckpointPolicy {
        &self.policy
    }
    #[must_use]
    pub const fn checkpoint_id(&self) -> [u8; 32] {
        self.checkpoint_id
    }
    #[must_use]
    pub fn canonical_header(&self) -> &[u8] {
        &self.canonical_header
    }
    #[must_use]
    pub fn settlement_reference(&self) -> &[u8] {
        &self.settlement_reference
    }
    #[must_use]
    pub const fn set_version(&self) -> u64 {
        self.set_version
    }
    #[must_use]
    pub const fn protocol_version(&self) -> u16 {
        self.policy.protocol_version
    }
    #[must_use]
    pub const fn network_id(&self) -> u32 {
        self.policy.network_id
    }
    #[must_use]
    pub const fn canonical_genesis_root(&self) -> [u8; 32] {
        self.policy.canonical_genesis_root
    }
    #[must_use]
    pub const fn registration(&self) -> BlockAnchor {
        self.registration
    }
    #[must_use]
    pub const fn confirmed_head(&self) -> BlockAnchor {
        self.confirmed_head
    }
}

impl PaxeerCheckpointVerifier {
    /// # Errors
    /// Refuses invalid transport or incomplete, conflicting settlement identity.
    pub fn new(policy: PaxeerCheckpointPolicy) -> Result<Self, EndpointFault> {
        canonical_endpoint_identity(&policy.endpoint)?;
        if policy.endpoint.request_timeout.is_zero()
            || policy.confirmations == 0
            || !matches!(policy.protocol_version, 2 | 3)
            || policy.network_id == 0
            || policy.registry.bytes() == [0; 20]
            || policy.guarantor_bond.bytes() == [0; 20]
            || policy.registry == policy.guarantor_bond
            || policy.canonical_genesis_root == [0; 32]
        {
            return Err(invalid());
        }
        Ok(Self { policy })
    }

    #[must_use]
    pub const fn policy(&self) -> &PaxeerCheckpointPolicy {
        &self.policy
    }

    /// # Errors
    /// Refuses any certificate, publication, canonical-chain or pinned-policy mismatch.
    pub fn verify(
        &self,
        certificate: &Certificate,
        expected_guarantor_set_version: u64,
    ) -> Result<VerifiedCheckpointPublication, EndpointFailure> {
        self.verify_inner(certificate, expected_guarantor_set_version)
            .map_err(|fault| failure(&self.policy.endpoint, fault))
    }

    fn verify_inner(
        &self,
        certificate: &Certificate,
        set_version: u64,
    ) -> Result<VerifiedCheckpointPublication, EndpointFault> {
        let header =
            decode_batch_header(certificate.checkpoint().header_bytes()).map_err(|_| invalid())?;
        let identifier = checkpoint_id(certificate.checkpoint()).map_err(|_| invalid())?;
        if header.protocol_version() != self.policy.protocol_version
            || header.network_id() != self.policy.network_id
            || set_version == 0
            || certificate.threshold() == 0
            || certificate.attestations().len() < certificate.threshold()
            || certificate.attestations().len() > 32
        {
            return Err(invalid());
        }
        let reference_bytes = certificate.settlement_reference().ok_or_else(invalid)?;
        let reference = SettlementReference::decode(reference_bytes, &self.policy, identifier)?;
        let published = crate::contract::verify(
            &self.policy,
            certificate,
            &header,
            identifier,
            set_version,
            reference,
        )?;
        Ok(VerifiedCheckpointPublication {
            policy: self.policy.clone(),
            checkpoint_id: identifier,
            canonical_header: certificate.checkpoint().header_bytes().to_vec(),
            settlement_reference: reference_bytes.to_vec(),
            set_version,
            registration: published.registration,
            confirmed_head: published.confirmed_head,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EndpointTransport;
    use std::time::Duration;

    fn policy() -> PaxeerCheckpointPolicy {
        PaxeerCheckpointPolicy {
            endpoint: EndpointConfig {
                url: "http://127.0.0.1:18545".into(),
                request_timeout: Duration::from_secs(5),
                transport: EndpointTransport::LocalEmulator,
                expected_chain_id: 125,
            },
            registry: EvmAddress::new([1; 20]),
            guarantor_bond: EvmAddress::new([2; 20]),
            protocol_version: 3,
            network_id: 77,
            canonical_genesis_root: [3; 32],
            confirmations: 2,
        }
    }

    #[test]
    fn explicit_policy_preserves_every_pin() {
        let policy = policy();
        let verifier = PaxeerCheckpointVerifier::new(policy.clone())
            .unwrap_or_else(|error| panic!("valid policy: {error:?}"));
        assert_eq!(verifier.policy(), &policy);
    }

    #[test]
    fn incomplete_or_untrusted_policy_is_refused_before_rpc() {
        let original = policy();
        let changes: [fn(&mut PaxeerCheckpointPolicy); 12] = [
            |p| p.confirmations = 0,
            |p| p.protocol_version = 1,
            |p| p.network_id = 0,
            |p| p.canonical_genesis_root = [0; 32],
            |p| p.registry = EvmAddress::new([0; 20]),
            |p| p.guarantor_bond = EvmAddress::new([0; 20]),
            |p| p.registry = p.guarantor_bond,
            |p| p.endpoint.expected_chain_id = 0,
            |p| p.endpoint.request_timeout = Duration::ZERO,
            |p| p.endpoint.url = "http://example.com/".into(),
            |p| p.endpoint.url = "http://127.0.0.1/\r\nInjected:yes".into(),
            |p| {
                p.endpoint.url = "https://localhost/".into();
                p.endpoint.transport = EndpointTransport::PinnedTls {
                    trust_anchor_der: Vec::new(),
                };
            },
        ];
        for change in changes {
            let mut candidate = original.clone();
            change(&mut candidate);
            assert!(PaxeerCheckpointVerifier::new(candidate).is_err());
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SettlementReference {
    pub transaction_id: [u8; 32],
    pub block_number: u64,
    pub observed_at_ms: u64,
}

impl SettlementReference {
    pub(crate) fn decode(
        bytes: &[u8],
        policy: &PaxeerCheckpointPolicy,
        identifier: [u8; 32],
    ) -> Result<Self, EndpointFault> {
        if bytes.len() != 110
            || bytes[..2] != [0, 1]
            || bytes[2..10] != policy.endpoint.expected_chain_id.to_be_bytes()
            || bytes[10..30] != policy.guarantor_bond.bytes()
            || bytes[30..62] != identifier
        {
            return Err(invalid());
        }
        let reference = Self {
            transaction_id: bytes[62..94].try_into().map_err(|_| invalid())?,
            block_number: u64::from_be_bytes(bytes[94..102].try_into().map_err(|_| invalid())?),
            observed_at_ms: u64::from_be_bytes(bytes[102..110].try_into().map_err(|_| invalid())?),
        };
        if reference.transaction_id == [0; 32]
            || reference.block_number == 0
            || reference.observed_at_ms == 0
        {
            return Err(invalid());
        }
        Ok(reference)
    }
}

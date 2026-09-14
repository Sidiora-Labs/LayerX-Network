use layerx_human_service::server::movement_provider::{
    MovementProviderCodec, MovementProviderResponse, NativeMovementCodec,
};
use layerx_paxeer_client::{
    DepositProof, DepositProofVerifier, FinalityTracker, PublishedDepositProof, TransactionHash,
};
use layerx_types::account::AccountId;

use crate::config::{hex, hex_string, Config, MAX_FRAME};
use crate::journal::{private_directory, publish_private, read_private};
use crate::Error;

pub(crate) struct Request {
    pub(crate) transaction: TransactionHash,
    pub(crate) checkpoint: [u8; 32],
    pub(crate) recipient: AccountId,
}

impl Request {
    pub fn arguments(mut arguments: impl Iterator<Item = String>) -> Result<Option<Self>, Error> {
        let Some(operation) = arguments.next() else {
            return Ok(None);
        };
        if operation != "--publish-deposit-proof" {
            return Err(Error::Configuration);
        }
        let transaction = TransactionHash::from_hex(&arguments.next().ok_or(Error::Configuration)?)
            .map_err(|_| Error::Configuration)?;
        let checkpoint = hex(&arguments.next().ok_or(Error::Configuration)?)?;
        let account = arguments.next().ok_or(Error::Configuration)?;
        let recipient = AccountId::parse(&account).map_err(|_| Error::Configuration)?;
        if transaction.bytes() == [0; 32]
            || checkpoint == [0; 32]
            || recipient.canonical() != account
            || arguments.next().is_some()
        {
            return Err(Error::Configuration);
        }
        Ok(Some(Self {
            transaction,
            checkpoint,
            recipient,
        }))
    }
}

pub(crate) fn publish(config: &Config, request: &Request) -> Result<(), Error> {
    if config.listener.protocol != config.proof.layerx_protocol_version {
        return Err(Error::Configuration);
    }
    publish_registered(&config.tracker, &config.proof, config.vault,
        config.checkpoint_registry, &config.evidence_root, request)
}

pub(crate) fn publish_registered(tracker: &layerx_paxeer_client::TrackerConfig,
    policy: &layerx_paxeer_client::DepositProofConfig, vault: layerx_types::intent::EvmAddress,
    registry: layerx_types::intent::EvmAddress, evidence_root: &std::path::Path,
    request: &Request) -> Result<(), Error>
{
    private_directory(evidence_root)?;
    if tracker.endpoints != policy.endpoints
        || tracker.minimum_endpoint_agreement != policy.minimum_endpoint_agreement
        || tracker.required_confirmations != policy.required_confirmations
    {
        return Err(Error::Configuration);
    }
    let mut tracker = FinalityTracker::new(tracker.clone(), request.transaction)
        .map_err(|_| Error::Configuration)?;
    let report = tracker.poll();
    let verifier =
        DepositProofVerifier::new(policy.clone()).map_err(|_| Error::Configuration)?;
    let custody = verifier
        .admit_custody(&report, vault, &request.recipient)
        .map_err(|_| Error::Integrity)?;
    let codec = NativeMovementCodec::for_protocol(policy.layerx_protocol_version)
        .map_err(|_| Error::Configuration)?;
    let mut candidates = Vec::new();
    for endpoint in &policy.endpoints {
        let Ok(published) = PublishedDepositProof::fetch_published(
            endpoint,
            vault,
            registry,
            request.checkpoint,
            custody.custody(),
            policy.required_confirmations,
        ) else {
            continue;
        };
        let Ok(proof) = verifier.obtain(&report, vault, published) else {
            continue;
        };
        let bytes = codec
            .encode_response(&MovementProviderResponse::DepositProof(Ok(proof.clone())))
            .map_err(|_| Error::Integrity)?;
        if bytes.is_empty() || bytes.len() > MAX_FRAME {
            return Err(Error::Capacity);
        }
        candidates.push((proof, bytes));
    }
    let (proof, bytes) = candidates
        .iter()
        .find(|(_, bytes)| {
            candidates
                .iter()
                .filter(|(_, other)| other == bytes)
                .count()
                >= policy.minimum_endpoint_agreement
        })
        .ok_or(Error::Integrity)?;
    let path = evidence_root.join(format!(
        "deposit-{}.bin",
        hex_string(&request.transaction.bytes())
    ));
    if !publish_private(&path, bytes)? {
        let prior = read_private(&path, MAX_FRAME)?;
        let MovementProviderResponse::DepositProof(Ok(prior)) = codec
            .decode_response(&prior)
            .map_err(|_| Error::Integrity)?
        else {
            return Err(Error::Integrity);
        };
        if !same_publication(&prior, proof) {
            return Err(Error::Conflict);
        }
    }
    Ok(())
}

fn same_publication(prior: &DepositProof, current: &DepositProof) -> bool {
    prior.transaction() == current.transaction()
        && prior.inclusion() == current.inclusion()
        && prior.confirmations() <= current.confirmations()
        && prior.required() == current.required()
        && prior.chain_id() == current.chain_id()
        && prior.vault() == current.vault()
        && prior.custody() == current.custody()
        && prior.checkpoint_id() == current.checkpoint_id()
        && prior.checkpoint_state_root() == current.checkpoint_state_root()
        && prior.deposit_root() == current.deposit_root()
        && prior.custody_reference() == current.custody_reference()
        && prior.network_id() == current.network_id()
        && prior.protocol_version() == current.protocol_version()
        && prior.registration_signature() == current.registration_signature()
        && prior.inclusion_proof() == current.inclusion_proof()
        && prior.leaf_hash() == current.leaf_hash()
        && prior.nullifier() == current.nullifier()
}

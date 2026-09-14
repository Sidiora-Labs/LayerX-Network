use super::HumanOperationError;
use layerx_client::receipt::ReceiptError;
use layerx_proof::receipt::NativeOwnerOutcomeContext;
use layerx_types::payload::ModuleRegistry;
use layerx_wire::activity::{decode_signed, encode_signed, Activity};
use layerx_wire::hash::activity_id;

pub(super) struct RetainedNativeOwner {
    canonical: Vec<u8>,
    activity: Activity,
    owner: [u8; 32],
}

impl RetainedNativeOwner {
    pub(super) fn decode(
        canonical: Vec<u8>,
        registry: &ModuleRegistry,
        node: &layerx_client::lni::handshake::NodeInfo,
        action: [u8; 32],
        identifier: [u8; 32],
    ) -> Result<Option<Self>, HumanOperationError> {
        let activity = decode_signed(&canonical, registry).map_err(|_| HumanOperationError::Refused)?;
        if encode_signed(&activity).map_err(|_| HumanOperationError::Refused)? != canonical
            || activity_id(&activity).map_err(|_| HumanOperationError::Refused)? != identifier
            || activity.idempotency_key() != action || activity.network_id() != node.network_id
            || activity.protocol_version() != node.protocol_version
        {
            return Err(HumanOperationError::Refused);
        }
        if activity.protocol_version() != 3 || !(2..=7).contains(&(activity.activity_type().module() as u16)) {
            return Ok(None);
        }
        let owner = activity.authority().try_into().map_err(|_| HumanOperationError::Refused)?;
        Ok(Some(Self { canonical, activity, owner }))
    }

    pub(super) fn context(&self) -> NativeOwnerOutcomeContext<'_> {
        NativeOwnerOutcomeContext { canonical_activity: &self.canonical,
            actor: self.activity.actor_did(), action_key: self.activity.idempotency_key(),
            activity_type: self.activity.activity_type(), owner_public_key: self.owner,
            network_id: self.activity.network_id() }
    }
}

pub(super) fn map_lookup_error(error: ReceiptError) -> HumanOperationError {
    match error {
        ReceiptError::Transport(_) | ReceiptError::Disconnected | ReceiptError::UnavailableCapability => HumanOperationError::Unavailable,
        _ => HumanOperationError::Refused,
    }
}

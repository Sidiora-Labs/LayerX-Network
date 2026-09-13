use crate::lni::preparation::{preparation_state, PreparationStateContext};
use crate::payments::{estimate_fee, get_asset, SnapshotContext};
use layerx_types::ids::Did;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistry};
use layerx_wire::activity::Activity;

use crate::lni::transport::FrameTransport;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WithdrawalConfigurationError {
    Unsupported,
    Unavailable,
    InvalidActivity,
    AssetUnavailable,
}

/// Reads one authenticated committed snapshot for withdrawal admission.
///
/// # Errors
/// Refuses unsupported schedules or runtimes, stale or inconsistent snapshots,
/// noncanonical payloads and unavailable custody assets.
pub fn registry(
    transport: &mut dyn FrameTransport,
    activity: &Activity,
    context: SnapshotContext,
    network_id: u32,
) -> Result<ModuleRegistry, WithdrawalConfigurationError> {
    let unavailable = || WithdrawalConfigurationError::Unavailable;
    let unsupported = || WithdrawalConfigurationError::Unsupported;
    if activity.protocol_version() != 3
        || activity.network_id() != network_id
        || activity.activity_type().module() != ModuleId::Asset
        || activity.activity_type().ordinal() != 9
    {
        return Err(WithdrawalConfigurationError::InvalidActivity);
    }
    let schedule =
        estimate_fee(transport, 0x0001_0005, 0, 0, 0, context).map_err(|_| unavailable())?;
    if schedule.value.canonical_schedule.len() != 255
        || schedule.value.canonical_schedule[..2] != [0, 3]
        || schedule.value.canonical_schedule[86] != 11
    {
        return Err(unsupported());
    }
    let actor = Did::new(activity.actor_did())
        .map_err(|_| WithdrawalConfigurationError::InvalidActivity)?;
    let prepared = preparation_state(
        transport,
        &actor,
        PreparationStateContext {
            interface_version: context.interface_version,
            expected_network_id: network_id,
            minimum_observed_head: schedule.observed_sequence,
            correlation_id: context
                .correlation_id
                .checked_add(1)
                .ok_or_else(unavailable)?,
        },
    )
    .map_err(|_| unavailable())?;
    let credit = ActivityType::new(ModuleId::Bridge, 1).map_err(|_| unavailable())?;
    if !prepared.module_registry.declares(activity.activity_type())
        || !prepared.module_registry.declares(credit)
    {
        return Err(unsupported());
    }
    let unsigned = layerx_wire::activity::encode_unsigned(activity)
        .map_err(|_| WithdrawalConfigurationError::InvalidActivity)?;
    layerx_crypto::disclosure::bind(&unsigned, &prepared.module_registry)
        .map_err(|_| WithdrawalConfigurationError::InvalidActivity)?;
    let asset_id = activity
        .payload()
        .get(..32)
        .ok_or(WithdrawalConfigurationError::InvalidActivity)?
        .try_into()
        .map_err(|_| WithdrawalConfigurationError::InvalidActivity)?;
    let asset = get_asset(
        transport,
        asset_id,
        SnapshotContext {
            correlation_id: context
                .correlation_id
                .checked_add(2)
                .ok_or_else(unavailable)?,
            minimum_sequence: prepared.observed_head_sequence,
            ..context
        },
    )
    .map_err(|_| unavailable())?;
    if prepared.observed_head_sequence != schedule.observed_sequence
        || prepared.observed_state_root != schedule.state_root
        || asset.observed_sequence != schedule.observed_sequence
        || asset.state_root != schedule.state_root
    {
        return Err(unavailable());
    }
    if asset.value.custody_kind != 2
        || asset.value.issuer_kind != 2
        || asset.value.custody_reference.is_empty()
        || asset.value.custody_reference.iter().all(|byte| *byte == 0)
        || asset.value.paused
    {
        return Err(WithdrawalConfigurationError::AssetUnavailable);
    }
    Ok(prepared.module_registry)
}

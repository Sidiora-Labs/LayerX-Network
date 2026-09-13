use layerx_client::lni::preparation::{preparation_state, PreparationStateContext};
use layerx_client::payments::{estimate_fee, get_asset, SnapshotContext};
use layerx_types::ids::Did;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistry};
use layerx_wire::activity::Activity;

use super::{connect_raw, refusal, Config, Response, SubmissionValidationError};

pub(super) fn registry(
    config: &Config,
    activity: &Activity,
) -> Result<ModuleRegistry, Response> {
    let unavailable = || refusal(503, "withdrawal_configuration_unavailable", Some(5));
    let unsupported = || SubmissionValidationError::AssetOrdinalReserved.response();
    let (mut transport, handshake) = connect_raw(config).map_err(|_| unavailable())?;
    let node = handshake.node();
    let context = SnapshotContext {
        interface_version: node.interface_version,
        correlation_id: 1,
        minimum_sequence: node.chain_head_sequence,
    };
    let schedule = estimate_fee(&mut transport, 0x0001_0005, 0, 0, 0, context)
        .map_err(|_| unavailable())?;
    if schedule.value.canonical_schedule.len() != 255
        || schedule.value.canonical_schedule[..2] != [0, 3]
        || schedule.value.canonical_schedule[86] != 11
    {
        return Err(unsupported());
    }
    let actor = Did::new(activity.actor_did())
        .map_err(|_| refusal(400, "invalid_asset_activity", None))?;
    let prepared = preparation_state(
        &mut transport,
        &actor,
        PreparationStateContext {
            interface_version: context.interface_version,
            expected_network_id: config.network_id,
            minimum_observed_head: schedule.observed_sequence,
            correlation_id: 2,
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
        .map_err(|_| refusal(400, "invalid_asset_activity", None))?;
    layerx_crypto::disclosure::bind(&unsigned, &prepared.module_registry)
        .map_err(|_| refusal(400, "invalid_asset_activity", None))?;
    let asset_id = activity.payload()[..32]
        .try_into()
        .map_err(|_| refusal(400, "invalid_asset_activity", None))?;
    let asset = get_asset(
        &mut transport,
        asset_id,
        SnapshotContext {
            correlation_id: 3,
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
        return Err(refusal(422, "withdrawal_asset_unavailable", None));
    }
    Ok(prepared.module_registry)
}

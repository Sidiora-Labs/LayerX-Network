use layerx_client::payments::SnapshotContext;
use layerx_client::withdrawal::WithdrawalConfigurationError;
use layerx_types::payload::ModuleRegistry;
use layerx_wire::activity::Activity;

use super::{connect_raw, refusal, Config, Response, SubmissionValidationError};

pub(super) fn registry(config: &Config, activity: &Activity) -> Result<ModuleRegistry, Response> {
    let (mut transport, handshake) = connect_raw(config)
        .map_err(|_| refusal(503, "withdrawal_configuration_unavailable", Some(5)))?;
    layerx_client::withdrawal::registry(
        &mut transport,
        activity,
        SnapshotContext {
            interface_version: handshake.node().interface_version,
            correlation_id: 1,
            minimum_sequence: handshake.node().chain_head_sequence,
        },
        config.network_id,
    )
    .map_err(|error| match error {
        WithdrawalConfigurationError::Unsupported => {
            SubmissionValidationError::AssetOrdinalReserved.response()
        }
        WithdrawalConfigurationError::Unavailable => {
            refusal(503, "withdrawal_configuration_unavailable", Some(5))
        }
        WithdrawalConfigurationError::InvalidActivity => {
            refusal(400, "invalid_asset_activity", None)
        }
        WithdrawalConfigurationError::AssetUnavailable => {
            refusal(422, "withdrawal_asset_unavailable", None)
        }
    })
}

package keeper

import (
	"fmt"
	"time"

	"github.com/paxeer-network/paxlog"
	"github.com/sidiora-labs/paxeer-network/modules/epoch/types"
	"github.com/sidiora-labs/paxeer-network/sdk/telemetry"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/utils/metrics"
)

var logger = paxlog.NewLogger("x", "epoch", "keeper")

func (k Keeper) BeginBlock(ctx sdk.Context) {
	start := time.Now()
	defer func() {
		epochMetrics.beginBlockerDuration.Record(ctx.Context(), time.Since(start).Seconds())
		// TODO(PLT-336): remove once epoch_begin_blocker_duration_seconds verified
		telemetry.ModuleMeasureSince(types.ModuleName, start, telemetry.MetricKeyBeginBlocker)
	}()
	lastEpoch := k.GetEpoch(ctx)
	logger.Info(" Block time", "current", ctx.BlockTime(), "last", lastEpoch.CurrentEpochStartTime, "epoch-duration", lastEpoch.EpochDuration)

	if ctx.BlockTime().Sub(lastEpoch.CurrentEpochStartTime) > lastEpoch.EpochDuration {
		k.AfterEpochEnd(ctx, lastEpoch)

		newEpoch := types.Epoch{
			GenesisTime:           lastEpoch.GenesisTime,
			EpochDuration:         lastEpoch.EpochDuration,
			CurrentEpoch:          lastEpoch.CurrentEpoch + 1,
			CurrentEpochStartTime: ctx.BlockTime(),
			CurrentEpochHeight:    ctx.BlockHeight(),
		}
		k.SetEpoch(ctx, newEpoch)
		k.BeforeEpochStart(ctx, newEpoch)

		ctx.EventManager().EmitEvent(
			sdk.NewEvent(types.EventTypeNewEpoch,
				sdk.NewAttribute(types.AttributeEpochNumber, fmt.Sprint(newEpoch.CurrentEpoch)),
				sdk.NewAttribute(types.AttributeEpochTime, newEpoch.CurrentEpochStartTime.String()),
				sdk.NewAttribute(types.AttributeEpochHeight, fmt.Sprint(newEpoch.CurrentEpochHeight)),
			),
		)

		epochMetrics.epochNew.Record(ctx.Context(), int64(newEpoch.CurrentEpoch)) //nolint:gosec
		// TODO(PLT-336): remove once epoch_new verified
		metrics.SetEpochNew(newEpoch.CurrentEpoch)
	}
}

package capability

import (
	"time"

	"github.com/sidiora-labs/paxeer-network/sdk/telemetry"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/capability/keeper"
	"github.com/sidiora-labs/paxeer-network/sdk/x/capability/types"
)

// BeginBlocker will call InitMemStore to initialize the memory stores in the case
// that this is the first time the node is executing a block since restarting (wiping memory).
// In this case, the BeginBlocker method will reinitialize the memory stores locally, so that subsequent
// capability transactions will pass.
// Otherwise BeginBlocker performs a no-op.
func BeginBlocker(ctx sdk.Context, k keeper.Keeper) {
	beginBlockerStart := time.Now()
	defer func() {
		capabilityMetrics.beginBlockerDuration.Record(ctx.Context(), time.Since(beginBlockerStart).Seconds())
		// TODO(PLT-414): remove once capability_begin_blocker_duration verified
		telemetry.ModuleMeasureSince(types.ModuleName, beginBlockerStart, telemetry.MetricKeyBeginBlocker)
	}()

	k.InitMemStore(ctx)
}

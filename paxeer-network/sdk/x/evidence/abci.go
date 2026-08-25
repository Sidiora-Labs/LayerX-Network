package evidence

import (
	"time"

	"github.com/paxeer-network/paxlog"
	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	"github.com/sidiora-labs/paxeer-network/sdk/telemetry"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/evidence/keeper"
	"github.com/sidiora-labs/paxeer-network/sdk/x/evidence/types"
)

var logger = paxlog.NewLogger("cosmos", "x", "evidence")

// BeginBlocker iterates through and handles any newly discovered evidence of
// misbehavior submitted by Tendermint. Currently, only equivocation is handled.
func BeginBlocker(ctx sdk.Context, byzantineValidators []abci.Misbehavior, k keeper.Keeper) {
	beginBlockerStart := time.Now()
	defer func() {
		evidenceMetrics.beginBlockerDuration.Record(ctx.Context(), time.Since(beginBlockerStart).Seconds())
		// TODO(PLT-414): remove once evidence_begin_blocker_duration verified
		telemetry.ModuleMeasureSince(types.ModuleName, beginBlockerStart, telemetry.MetricKeyBeginBlocker)
	}()

	for _, tmEvidence := range byzantineValidators {
		switch tmEvidence.Type {
		// It's still ongoing discussion how should we treat and slash attacks with
		// premeditation. So for now we agree to treat them in the same way.
		case abci.MisbehaviorType_DUPLICATE_VOTE, abci.MisbehaviorType_LIGHT_CLIENT_ATTACK:
			evidence := types.FromABCIEvidence(abci.Evidence(tmEvidence))
			k.HandleEquivocationEvidence(ctx, evidence.(*types.Equivocation))

		default:
			logger.Error("ignored unknown evidence type", "type", tmEvidence.Type)
		}
	}
}

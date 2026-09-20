package legacyabci

import (
	"time"

	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	"github.com/sidiora-labs/paxeer-network/sdk/telemetry"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/capability"
	capabilitykeeper "github.com/sidiora-labs/paxeer-network/sdk/x/capability/keeper"

	"github.com/sidiora-labs/paxeer-network/sdk/x/distribution"
	distrkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/distribution/keeper"

	"github.com/sidiora-labs/paxeer-network/sdk/x/evidence"
	evidencekeeper "github.com/sidiora-labs/paxeer-network/sdk/x/evidence/keeper"
	"github.com/sidiora-labs/paxeer-network/sdk/x/slashing"
	slashingkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/slashing/keeper"

	"github.com/sidiora-labs/paxeer-network/sdk/x/staking"
	stakingkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/staking/keeper"
	"github.com/sidiora-labs/paxeer-network/sdk/x/upgrade"
	upgradekeeper "github.com/sidiora-labs/paxeer-network/sdk/x/upgrade/keeper"

	ibcclient "github.com/sidiora-labs/paxeer-network/interchain/modules/core/02-client"
	ibckeeper "github.com/sidiora-labs/paxeer-network/interchain/modules/core/keeper"
	epochmodulekeeper "github.com/sidiora-labs/paxeer-network/modules/epoch/keeper"
	evmkeeper "github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
)

type BeginBlockKeepers struct {
	EpochKeeper      *epochmodulekeeper.Keeper
	UpgradeKeeper    *upgradekeeper.Keeper
	CapabilityKeeper *capabilitykeeper.Keeper
	DistrKeeper      *distrkeeper.Keeper
	SlashingKeeper   *slashingkeeper.Keeper
	EvidenceKeeper   *evidencekeeper.Keeper
	StakingKeeper    *stakingkeeper.Keeper
	IBCKeeper        *ibckeeper.Keeper
	EvmKeeper        *evmkeeper.Keeper
}

func BeginBlock(
	ctx sdk.Context,
	height int64,
	votes []abci.VoteInfo,
	byzantineValidators []abci.Misbehavior,
	keepers BeginBlockKeepers,
) {
	start := time.Now()
	defer func() {
		legacyAbciMetrics.totalBeginBlockDuration.Record(ctx.Context(), time.Since(start).Seconds())
		// TODO(PLT-343): remove once begin_blocker_duration verified
		telemetry.MeasureSince(start, "module", "total_begin_block")
	}()

	keepers.EpochKeeper.BeginBlock(ctx)
	upgrade.BeginBlocker(*keepers.UpgradeKeeper, ctx)
	capability.BeginBlocker(ctx, *keepers.CapabilityKeeper)
	distribution.BeginBlocker(ctx, votes, *keepers.DistrKeeper)
	slashing.BeginBlocker(ctx, votes, *keepers.SlashingKeeper)
	evidence.BeginBlocker(ctx, byzantineValidators, *keepers.EvidenceKeeper)
	staking.BeginBlocker(ctx, *keepers.StakingKeeper)
	func() {
		ibcStart := time.Now()
		defer func() {
			legacyAbciMetrics.ibcBeginBlockerDuration.Record(ctx.Context(), time.Since(ibcStart).Seconds())
			// TODO(PLT-343): remove once ibc_begin_blocker_duration verified
			telemetry.ModuleMeasureSince("ibc", ibcStart, telemetry.MetricKeyBeginBlocker)
		}()
		ibcclient.BeginBlocker(ctx, keepers.IBCKeeper.ClientKeeper)
	}()
	keepers.EvmKeeper.BeginBlock(ctx)
}

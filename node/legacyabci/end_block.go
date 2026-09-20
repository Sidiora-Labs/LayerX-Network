package legacyabci

import (
	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	evmkeeper "github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/oracle"
	oraclekeeper "github.com/sidiora-labs/paxeer-network/modules/oracle/keeper"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/gov"
	govkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/gov/keeper"
	"github.com/sidiora-labs/paxeer-network/sdk/x/staking"
	stakingkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/staking/keeper"
)

type EndBlockKeepers struct {
	GovKeeper     *govkeeper.Keeper
	StakingKeeper *stakingkeeper.Keeper
	OracleKeeper  *oraclekeeper.Keeper
	EvmKeeper     *evmkeeper.Keeper
}

func EndBlock(ctx sdk.Context, height int64, blockGasUsed int64, keepers EndBlockKeepers) []abci.ValidatorUpdate {
	gov.EndBlocker(ctx, *keepers.GovKeeper)
	vals := staking.EndBlocker(ctx, *keepers.StakingKeeper)
	oracle.EndBlocker(ctx, *keepers.OracleKeeper)
	keepers.EvmKeeper.EndBlock(ctx, height, blockGasUsed)
	return vals
}

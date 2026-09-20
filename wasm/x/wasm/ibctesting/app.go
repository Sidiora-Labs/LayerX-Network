package ibctesting

import (
	"github.com/sidiora-labs/paxeer-network/interchain/modules/core/keeper"
	"github.com/sidiora-labs/paxeer-network/sdk/baseapp"
	"github.com/sidiora-labs/paxeer-network/sdk/client"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	capabilitykeeper "github.com/sidiora-labs/paxeer-network/sdk/x/capability/keeper"
	stakingkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/staking/keeper"

	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	"github.com/sidiora-labs/paxeer-network/sdk/codec"
)

type TestingApp interface {
	abci.Application

	// ibc-go additions
	GetBaseApp() *baseapp.BaseApp
	GetStakingKeeper() stakingkeeper.Keeper
	GetIBCKeeper() *keeper.Keeper
	GetScopedIBCKeeper() capabilitykeeper.ScopedKeeper
	GetTxConfig() client.TxConfig

	// Implemented by SimApp
	AppCodec() codec.Codec

	// Implemented by BaseApp
	LastCommitID() sdk.CommitID
	LastBlockHeight() int64
}

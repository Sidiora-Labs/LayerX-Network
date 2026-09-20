package epoch

import (
	"math/rand"

	epochsimulation "github.com/sidiora-labs/paxeer-network/modules/epoch/simulation"
	"github.com/sidiora-labs/paxeer-network/modules/epoch/types"
	paxappparams "github.com/sidiora-labs/paxeer-network/node/params"
	"github.com/sidiora-labs/paxeer-network/sdk/baseapp"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/types/module"
	simtypes "github.com/sidiora-labs/paxeer-network/sdk/types/simulation"
	"github.com/sidiora-labs/paxeer-network/sdk/x/simulation"
	"github.com/sidiora-labs/paxeer-network/testutil/sample"
)

// avoid unused import issue
var (
	_ = sample.AccAddress
	_ = epochsimulation.FindAccount
	_ = paxappparams.StakePerAccount
	_ = simulation.MsgEntryKind
	_ = baseapp.Paramspace
)

const (
// this line is used by starport scaffolding # simapp/module/const
)

// GenerateGenesisState creates a randomized GenState of the module
func (AppModule) GenerateGenesisState(simState *module.SimulationState) {
	accs := make([]string, len(simState.Accounts))
	for i, acc := range simState.Accounts {
		accs[i] = acc.Address.String()
	}
	epochGenesis := types.GenesisState{
		// this line is used by starport scaffolding # simapp/module/genesisState
	}
	simState.GenState[types.ModuleName] = simState.Cdc.MustMarshalJSON(&epochGenesis)
}

// ProposalContents doesn't return any content functions for governance proposals
func (AppModule) ProposalContents(_ module.SimulationState) []simtypes.WeightedProposalContent {
	return nil
}

// RandomizedParams creates randomized  param changes for the simulator
func (am AppModule) RandomizedParams(_ *rand.Rand) []simtypes.ParamChange {
	return []simtypes.ParamChange{}
}

// RegisterStoreDecoder registers a decoder
func (am AppModule) RegisterStoreDecoder(_ sdk.StoreDecoderRegistry) {}

// WeightedOperations returns the all the gov module operations with their respective weights.
func (am AppModule) WeightedOperations(_ module.SimulationState) []simtypes.WeightedOperation {
	operations := make([]simtypes.WeightedOperation, 0)

	// this line is used by starport scaffolding # simapp/module/operation

	return operations
}

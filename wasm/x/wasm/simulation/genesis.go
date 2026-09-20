package simulation

import (
	"github.com/sidiora-labs/paxeer-network/sdk/types/module"

	"github.com/sidiora-labs/paxeer-network/wasm/x/wasm/types"
)

// RandomizeGenState generates a random GenesisState for wasm
func RandomizedGenState(simstate *module.SimulationState) {
	params := RandomParams(simstate.Rand)
	wasmGenesis := types.GenesisState{
		Params:    params,
		Codes:     nil,
		Contracts: nil,
		Sequences: []types.Sequence{
			{IDKey: types.KeyLastCodeID, Value: simstate.Rand.Uint64()},
			{IDKey: types.KeyLastInstanceID, Value: simstate.Rand.Uint64()},
		},
		GenMsgs: nil,
	}

	_, err := simstate.Cdc.MarshalAsJSON(&wasmGenesis)
	if err != nil {
		panic(err)
	}

	simstate.GenState[types.ModuleName] = simstate.Cdc.MustMarshalJSON(&wasmGenesis)
}

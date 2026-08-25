package simulation

import (
	"math/rand"

	simtypes "github.com/sidiora-labs/paxeer-network/sdk/types/simulation"

	"github.com/sidiora-labs/paxeer-network/interchain/modules/core/03-connection/types"
)

// GenConnectionGenesis returns the default connection genesis state.
func GenConnectionGenesis(_ *rand.Rand, _ []simtypes.Account) types.GenesisState {
	return types.DefaultGenesisState()
}

package simulation

// DONTCOVER

import (
	"fmt"
	"math/rand"

	simtypes "github.com/sidiora-labs/paxeer-network/sdk/types/simulation"
	"github.com/sidiora-labs/paxeer-network/sdk/x/simulation"

	"github.com/sidiora-labs/paxeer-network/modules/oracle/types"
)

// ParamChanges defines the parameters that can be modified by param change proposals
// on the simulation
func ParamChanges(_ *rand.Rand) []simtypes.ParamChange {
	return []simtypes.ParamChange{
		simulation.NewSimParamChange(types.ModuleName, string(types.KeyVotePeriod),
			func(r *rand.Rand) string {
				return fmt.Sprintf("\"%d\"", GenVotePeriod(r))
			},
		),
		simulation.NewSimParamChange(types.ModuleName, string(types.KeyVoteThreshold),
			func(r *rand.Rand) string {
				return fmt.Sprintf("\"%s\"", GenVoteThreshold(r))
			},
		),
		simulation.NewSimParamChange(types.ModuleName, string(types.KeyRewardBand),
			func(r *rand.Rand) string {
				return fmt.Sprintf("\"%s\"", GenRewardBand(r))
			},
		),
		simulation.NewSimParamChange(types.ModuleName, string(types.KeySlashFraction),
			func(r *rand.Rand) string {
				return fmt.Sprintf("\"%s\"", GenSlashFraction(r))
			},
		),
		simulation.NewSimParamChange(types.ModuleName, string(types.KeySlashWindow),
			func(r *rand.Rand) string {
				return fmt.Sprintf("\"%d\"", GenSlashWindow(r))
			},
		),
	}
}

package types_test

import (
	"testing"

	"github.com/sidiora-labs/paxeer-network/modules/epoch/types"
	"github.com/stretchr/testify/require"
)

func TestGenesisState_Validate(t *testing.T) {
	for _, tc := range []struct {
		desc     string
		genState *types.GenesisState
		valid    bool
	}{
		{
			desc:     "default is valid",
			genState: types.DefaultGenesis(),
			valid:    true,
		},
		{
			desc:     "invalid genesis state",
			genState: &types.GenesisState{},
			valid:    false,
		},
	} {
		t.Run(tc.desc, func(t *testing.T) {
			err := tc.genState.Validate()
			if tc.valid {
				require.NoError(t, err)
			} else {
				require.Error(t, err)
			}
		})
	}
}

func TestDefaultGenesisIsDeterministic(t *testing.T) {
	first := types.DefaultGenesis()
	second := types.DefaultGenesis()

	require.Equal(t, first, second)
	require.Equal(t, types.DefaultGenesisTime(), first.Epoch.GenesisTime)
	require.Equal(t, types.DefaultGenesisTime(), first.Epoch.CurrentEpochStartTime)
}

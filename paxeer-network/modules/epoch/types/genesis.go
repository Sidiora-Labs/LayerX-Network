package types

import "time"

// this line is used by starport scaffolding # genesis/types/import

// DefaultIndex is the default capability global index
const DefaultIndex uint64 = 1

func DefaultGenesisTime() time.Time {
	return time.Unix(1, 0).UTC()
}

// DefaultGenesis returns the default Capability genesis state
func DefaultGenesis() *GenesisState {
	return &GenesisState{
		// this line is used by starport scaffolding # genesis/types/default
		Params: DefaultParams(),
		Epoch: &Epoch{
			GenesisTime:           DefaultGenesisTime(),
			EpochDuration:         time.Minute,
			CurrentEpoch:          0,
			CurrentEpochStartTime: DefaultGenesisTime(),
			CurrentEpochHeight:    0,
		},
	}
}

// Validate performs basic genesis state validation returning an error upon any
// failure.
func (gs GenesisState) Validate() error {
	err := gs.Params.Validate()
	if err != nil {
		return err
	}

	err = gs.Epoch.Validate()
	return err
}

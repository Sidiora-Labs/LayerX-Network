package types

import (
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
)

// GenesisState is the whole xweb state. The default carries the documented
// parameters, no attestors, no requests and is paused: the module ships
// dormant and only governance registers attestors and unpauses it.
type GenesisState struct {
	Params    Params      `json:"params"`
	Paused    bool        `json:"paused"`
	Attestors AttestorSet `json:"attestors"`
	Nonce     uint64      `json:"nonce"`
	Requests  []Request   `json:"requests"`
	Results   []Result    `json:"results"`
}

// DefaultAuthority is the gov module account.
func DefaultAuthority() string {
	return authtypes.NewModuleAddress(govtypes.ModuleName).String()
}

func DefaultGenesis() *GenesisState {
	return &GenesisState{Params: DefaultParams(DefaultAuthority()), Paused: true}
}

func (g GenesisState) Validate() error {
	if err := g.Params.Validate(); err != nil {
		return err
	}
	if err := g.Attestors.Validate(); err != nil {
		return ErrInvalidGenesis.Wrap(err.Error())
	}
	requests := map[uint64]Request{}
	for _, request := range g.Requests {
		if err := request.Validate(); err != nil {
			return ErrInvalidGenesis.Wrap(err.Error())
		}
		if request.ID > g.Nonce {
			return ErrInvalidGenesis.Wrapf("request %d above nonce %d", request.ID, g.Nonce)
		}
		if _, duplicate := requests[request.ID]; duplicate {
			return ErrInvalidGenesis.Wrapf("duplicate request %d", request.ID)
		}
		requests[request.ID] = request
	}
	results := map[uint64]bool{}
	for _, result := range g.Results {
		if err := result.Validate(); err != nil {
			return ErrInvalidGenesis.Wrap(err.Error())
		}
		request, found := requests[result.RequestID]
		if !found || request.Status != StatusFulfilled {
			return ErrInvalidGenesis.Wrapf("result %d has no fulfilled request", result.RequestID)
		}
		if results[result.RequestID] {
			return ErrInvalidGenesis.Wrapf("duplicate result %d", result.RequestID)
		}
		results[result.RequestID] = true
	}
	for id, request := range requests {
		if request.Status == StatusFulfilled && !results[id] {
			return ErrInvalidGenesis.Wrapf("fulfilled request %d has no result", id)
		}
	}
	return nil
}

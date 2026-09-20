package keeper

import (
	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/precompiles/bank"
	"github.com/sidiora-labs/paxeer-network/precompiles/gov"
	"github.com/sidiora-labs/paxeer-network/precompiles/staking"
	"github.com/sidiora-labs/paxeer-network/precompiles/wasmd"
)

// add any payable precompiles here
// these will suppress transfer events to/from the precompile address
var payablePrecompiles = map[string]struct{}{
	bank.BankAddress:       {},
	staking.StakingAddress: {},
	gov.GovAddress:         {},
	wasmd.WasmdAddress:     {},
}

func IsPayablePrecompile(addr *common.Address) bool {
	if addr == nil {
		return false
	}
	_, ok := payablePrecompiles[addr.Hex()]
	return ok
}

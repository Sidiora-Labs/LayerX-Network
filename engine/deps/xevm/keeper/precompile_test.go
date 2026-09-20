package keeper_test

import (
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/stretchr/testify/require"

	"github.com/sidiora-labs/paxeer-network/engine/deps/testutil/keeper"
	evmkeeper "github.com/sidiora-labs/paxeer-network/engine/deps/xevm/keeper"
	"github.com/sidiora-labs/paxeer-network/precompiles/bank"
	"github.com/sidiora-labs/paxeer-network/precompiles/gov"
	"github.com/sidiora-labs/paxeer-network/precompiles/staking"
)

func toAddr(addr string) *common.Address {
	ca := common.HexToAddress(addr)
	return &ca
}

func TestIsPayablePrecompile(t *testing.T) {
	_, evmAddr := keeper.MockAddressPair()
	require.False(t, evmkeeper.IsPayablePrecompile(&evmAddr))
	require.False(t, evmkeeper.IsPayablePrecompile(nil))

	require.True(t, evmkeeper.IsPayablePrecompile(toAddr(bank.BankAddress)))
	require.True(t, evmkeeper.IsPayablePrecompile(toAddr(staking.StakingAddress)))
	require.True(t, evmkeeper.IsPayablePrecompile(toAddr(gov.GovAddress)))
}

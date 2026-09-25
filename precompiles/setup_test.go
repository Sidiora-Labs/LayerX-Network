package precompiles_test

import (
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/sidiora-labs/paxeer-network/precompiles"
	"github.com/sidiora-labs/paxeer-network/precompiles/feetoken"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func TestFeeTokenRegistration(t *testing.T) {
	address := common.HexToAddress("0x0000000000000000000000000000000000001018")
	require.Equal(t, address, common.HexToAddress(feetoken.FeeTokenAddress))
	require.NoError(t, precompiles.InitializePrecompiles(true, testkeeper.EVMTestApp.GetPrecompileKeepers()))
	info := precompiles.GetPrecompileInfo(feetoken.PrecompileName)
	require.Equal(t, address, info.Address)
	require.Len(t, info.ABI.Methods, 3)
	seen := make(map[common.Address]string)
	for name, entry := range precompiles.PrecompileNamesToInfo {
		previous, exists := seen[entry.Address]
		require.False(t, exists, "%s collides with %s at %s", name, previous, entry.Address)
		seen[entry.Address] = name
	}
	versioned := precompiles.GetCustomPrecompiles("v6.6", testkeeper.EVMTestApp.GetPrecompileKeepers())
	versions, found := versioned[address]
	require.True(t, found)
	require.Len(t, versions, 1)
	p, ok := versions["v6.6"].(precompiles.IPrecompile)
	require.True(t, ok)
	require.Equal(t, feetoken.PrecompileName, p.GetName())
	require.Equal(t, address, p.Address())
	for registered, entries := range versioned {
		for _, entry := range entries {
			named, ok := entry.(precompiles.IPrecompile)
			require.True(t, ok)
			require.Equal(t, registered, named.Address())
			if registered != address {
				require.NotEqual(t, feetoken.PrecompileName, named.GetName())
			}
		}
	}
	require.NoError(t, precompiles.InitializePrecompiles(false, testkeeper.EVMTestApp.GetPrecompileKeepers()))
	for _, contracts := range []map[common.Address]vm.PrecompiledContract{
		vm.PrecompiledContractsHomestead, vm.PrecompiledContractsByzantium,
		vm.PrecompiledContractsIstanbul, vm.PrecompiledContractsBerlin,
		vm.PrecompiledContractsCancun, vm.PrecompiledContractsBLS,
	} {
		entry, exists := contracts[address]
		require.True(t, exists)
		named, ok := entry.(precompiles.IPrecompile)
		require.True(t, ok)
		require.Equal(t, feetoken.PrecompileName, named.GetName())
	}
}

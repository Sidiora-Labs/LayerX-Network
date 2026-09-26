package precompiles_test

import (
	"maps"
	"slices"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/sidiora-labs/paxeer-network/precompiles"
	"github.com/sidiora-labs/paxeer-network/precompiles/feetoken"
	"github.com/sidiora-labs/paxeer-network/precompiles/xweb"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func TestFeeTokenRegistration(t *testing.T) {
	initialized := precompiles.Initialized
	infoBefore := precompiles.PrecompileNamesToInfo
	precompiles.Initialized = false
	precompiles.PrecompileNamesToInfo = maps.Clone(infoBefore)
	t.Cleanup(func() {
		precompiles.Initialized = initialized
		precompiles.PrecompileNamesToInfo = infoBefore
	})
	for _, contracts := range []*vm.PrecompiledContracts{
		&vm.PrecompiledContractsHomestead, &vm.PrecompiledContractsByzantium,
		&vm.PrecompiledContractsIstanbul, &vm.PrecompiledContractsBerlin,
		&vm.PrecompiledContractsCancun, &vm.PrecompiledContractsBLS,
	} {
		before := *contracts
		*contracts = maps.Clone(before)
		t.Cleanup(func() { *contracts = before })
	}
	for _, addresses := range []*[]common.Address{
		&vm.PrecompiledAddressesHomestead, &vm.PrecompiledAddressesByzantium,
		&vm.PrecompiledAddressesIstanbul, &vm.PrecompiledAddressesBerlin,
		&vm.PrecompiledAddressesCancun,
	} {
		before := *addresses
		*addresses = slices.Clone(before)
		t.Cleanup(func() { *addresses = before })
	}
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

func TestXWebRegistration(t *testing.T) {
	initialized := precompiles.Initialized
	infoBefore := precompiles.PrecompileNamesToInfo
	precompiles.Initialized = false
	precompiles.PrecompileNamesToInfo = maps.Clone(infoBefore)
	t.Cleanup(func() {
		precompiles.Initialized = initialized
		precompiles.PrecompileNamesToInfo = infoBefore
	})
	for _, contracts := range []*vm.PrecompiledContracts{
		&vm.PrecompiledContractsHomestead, &vm.PrecompiledContractsByzantium,
		&vm.PrecompiledContractsIstanbul, &vm.PrecompiledContractsBerlin,
		&vm.PrecompiledContractsCancun, &vm.PrecompiledContractsBLS,
	} {
		before := *contracts
		*contracts = maps.Clone(before)
		t.Cleanup(func() { *contracts = before })
	}
	for _, addresses := range []*[]common.Address{
		&vm.PrecompiledAddressesHomestead, &vm.PrecompiledAddressesByzantium,
		&vm.PrecompiledAddressesIstanbul, &vm.PrecompiledAddressesBerlin,
		&vm.PrecompiledAddressesCancun,
	} {
		before := *addresses
		*addresses = slices.Clone(before)
		t.Cleanup(func() { *addresses = before })
	}
	address := common.HexToAddress("0x0000000000000000000000000000000000001019")
	require.Equal(t, address, common.HexToAddress(xweb.XWebAddress))
	require.NoError(t, precompiles.InitializePrecompiles(true, testkeeper.EVMTestApp.GetPrecompileKeepers()))
	info := precompiles.GetPrecompileInfo(xweb.PrecompileName)
	require.Equal(t, address, info.Address)
	require.Len(t, info.ABI.Methods, 9)
	require.Len(t, info.ABI.Events, 3)
	seen := make(map[common.Address]string)
	for name, entry := range precompiles.PrecompileNamesToInfo {
		previous, exists := seen[entry.Address]
		require.False(t, exists, "%s collides with %s at %s", name, previous, entry.Address)
		seen[entry.Address] = name
	}
	versioned := precompiles.GetCustomPrecompiles("v6.8", testkeeper.EVMTestApp.GetPrecompileKeepers())
	versions, found := versioned[address]
	require.True(t, found)
	require.Len(t, versions, 1)
	p, ok := versions["v6.8"].(precompiles.IPrecompile)
	require.True(t, ok)
	require.Equal(t, xweb.PrecompileName, p.GetName())
	require.Equal(t, address, p.Address())
	_, metered := versions["v6.8"].(vm.DynamicGasPrecompiledContract)
	require.True(t, metered)
	for registered, entries := range versioned {
		for _, entry := range entries {
			named, ok := entry.(precompiles.IPrecompile)
			require.True(t, ok)
			require.Equal(t, registered, named.Address())
			if registered != address {
				require.NotEqual(t, xweb.PrecompileName, named.GetName())
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
		require.Equal(t, xweb.PrecompileName, named.GetName())
	}
	require.Contains(t, vm.PrecompiledAddressesCancun, address)
}

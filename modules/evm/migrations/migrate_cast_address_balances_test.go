package migrations_test

import (
	"testing"
	"time"

	"github.com/sidiora-labs/paxeer-network/modules/evm/migrations"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func TestMigrateCastAddressBalances(t *testing.T) {
	k := testkeeper.EVMTestApp.EvmKeeper
	ctx := testkeeper.EVMTestApp.GetContextForDeliverTx([]byte{}).WithBlockTime(time.Now())
	require.Nil(t, k.BankKeeper().MintCoins(ctx, types.ModuleName, testkeeper.UhpxCoins(100)))
	// unassociated account with funds
	paxAddr1, evmAddr1 := testkeeper.MockAddressPair()
	require.Nil(t, k.BankKeeper().SendCoinsFromModuleToAccount(ctx, types.ModuleName, sdk.AccAddress(evmAddr1[:]), testkeeper.UhpxCoins(10)))
	// associated account without funds
	paxAddr2, evmAddr2 := testkeeper.MockAddressPair()
	k.SetAddressMapping(ctx, paxAddr2, evmAddr2)
	// associated account with funds
	paxAddr3, evmAddr3 := testkeeper.MockAddressPair()
	require.Nil(t, k.BankKeeper().SendCoinsFromModuleToAccount(ctx, types.ModuleName, sdk.AccAddress(evmAddr3[:]), testkeeper.UhpxCoins(10)))
	k.SetAddressMapping(ctx, paxAddr3, evmAddr3)

	require.Nil(t, migrations.MigrateCastAddressBalances(ctx, &k))

	require.Equal(t, sdk.NewInt(10), k.BankKeeper().GetBalance(ctx, sdk.AccAddress(evmAddr1[:]), "uhpx").Amount)
	require.Equal(t, sdk.ZeroInt(), k.BankKeeper().GetBalance(ctx, paxAddr1, "uhpx").Amount)
	require.Equal(t, sdk.ZeroInt(), k.BankKeeper().GetBalance(ctx, sdk.AccAddress(evmAddr2[:]), "uhpx").Amount)
	require.Equal(t, sdk.ZeroInt(), k.BankKeeper().GetBalance(ctx, paxAddr2, "uhpx").Amount)
	require.Equal(t, sdk.ZeroInt(), k.BankKeeper().GetBalance(ctx, sdk.AccAddress(evmAddr3[:]), "uhpx").Amount)
	require.Equal(t, sdk.NewInt(10), k.BankKeeper().GetBalance(ctx, paxAddr3, "uhpx").Amount)
}

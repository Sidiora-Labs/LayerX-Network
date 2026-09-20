package migrations_test

import (
	"testing"

	"github.com/sidiora-labs/paxeer-network/modules/evm/migrations"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/node"
	"github.com/stretchr/testify/require"
)

func TestMigrateSstoreGas(t *testing.T) {
	a := app.Setup(t, false, false, false)
	k := a.EvmKeeper
	ctx := a.GetContextForDeliverTx([]byte{})

	params := k.GetParams(ctx)
	params.PaxSstoreSetGasEip2200 = 12345
	k.SetParams(ctx, params)

	require.NoError(t, migrations.MigrateSstoreGas(ctx, &k))

	updated := k.GetParams(ctx)
	require.Equal(t, types.DefaultPaxSstoreSetGasEIP2200, updated.PaxSstoreSetGasEip2200)
}

package migrations_test

import (
	"testing"

	tmtypes "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/migrations"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func TestAddNewParamsAndSetAllToDefaults(t *testing.T) {
	k := testkeeper.EVMTestApp.EvmKeeper
	ctx := testkeeper.EVMTestApp.NewContext(false, tmtypes.Header{})
	migrations.AddNewParamsAndSetAllToDefaults(ctx, &k)
	require.NotPanics(t, func() { k.GetParams(ctx) })
}

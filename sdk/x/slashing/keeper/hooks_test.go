package keeper_test

import (
	"testing"

	tmproto "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	paxapp "github.com/sidiora-labs/paxeer-network/node"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/stretchr/testify/assert"
)

func TestAfterValidatorBonded(t *testing.T) {
	app := paxapp.Setup(t, false, false, false)
	ctx := app.BaseApp.NewContext(false, tmproto.Header{})
	addrDels := paxapp.AddTestAddrsIncremental(app, ctx, 6, app.StakingKeeper.TokensFromConsensusPower(ctx, 200))
	valAddrs := paxapp.ConvertAddrsToValAddrs(addrDels)
	keeper := app.SlashingKeeper
	consAddr := sdk.ConsAddress(addrDels[0])

	keeper.AfterValidatorBonded(ctx, consAddr, valAddrs[0])

	// Verify the updated signing info
	signingInfo, found := keeper.GetValidatorSigningInfo(ctx, consAddr)
	assert.True(t, found)
	assert.Equal(t, ctx.BlockHeight(), signingInfo.StartHeight)
	assert.Equal(t, int64(0), signingInfo.MissedBlocksCounter)
	assert.False(t, signingInfo.Tombstoned)
	assert.Equal(t, false, signingInfo.Tombstoned)
}

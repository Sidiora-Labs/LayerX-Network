package keeper_test

import (
	"testing"

	tmproto "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	paxapp "github.com/sidiora-labs/paxeer-network/node"
	"github.com/sidiora-labs/paxeer-network/sdk/x/slashing/types"
	"github.com/stretchr/testify/assert"
)

func TestResizeMissedBlockArray(t *testing.T) {
	app := paxapp.Setup(t, false, false, false)
	ctx := app.BaseApp.NewContext(false, tmproto.Header{})
	addrDels := paxapp.AddTestAddrsIncremental(app, ctx, 6, app.StakingKeeper.TokensFromConsensusPower(ctx, 200))
	valAddrs := paxapp.ConvertAddrsToValAddrs(addrDels)

	// initial parameters for tests
	initialWindowSize := int64(10)
	newWindowSize := int64(20)
	initialIndex := int64(5)
	initialMissedBlocks := make([]uint64, initialWindowSize)
	initialSignInfo := types.ValidatorSigningInfo{
		Address:             valAddrs[0].String(),
		StartHeight:         0,
		MissedBlocksCounter: initialIndex,
		IndexOffset:         initialIndex,
	}

	// initialize keeper with mock functions as required
	k := app.SlashingKeeper

	// initialize missed info
	missedInfo := types.ValidatorMissedBlockArray{
		Address:      valAddrs[0].String(),
		WindowSize:   initialWindowSize,
		MissedBlocks: initialMissedBlocks,
	}

	// Test expand the window
	resizedMissedInfo, resizedSignInfo, newIndex := k.ResizeMissedBlockArray(missedInfo, initialSignInfo, newWindowSize, initialIndex)

	// assertions
	assert.Equal(t, newWindowSize, resizedMissedInfo.WindowSize)
	assert.Equal(t, int64(5), resizedSignInfo.MissedBlocksCounter)
	assert.Equal(t, int64(5), resizedSignInfo.IndexOffset)
	assert.Equal(t, int64(5), newIndex)

	// Now test the shrinking scenario
	shrinkedWindowSize := int64(5)
	resizedMissedInfo, resizedSignInfo, newIndex = k.ResizeMissedBlockArray(missedInfo, initialSignInfo, shrinkedWindowSize, initialIndex)

	// assertions
	assert.Equal(t, shrinkedWindowSize, resizedMissedInfo.WindowSize)
	assert.Equal(t, int64(0), resizedSignInfo.MissedBlocksCounter)
	assert.Equal(t, int64(0), resizedSignInfo.IndexOffset)
	assert.Equal(t, int64(0), newIndex)
}

package antedecorators_test

import (
	"testing"

	tmproto "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/sidiora-labs/paxeer-network/node/antedecorators"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/utils"
	"github.com/stretchr/testify/require"
)

func TestTracedDecorator(t *testing.T) {
	output = ""
	anteDecorators := []sdk.AnteDecorator{
		FakeAnteDecoratorOne{},
		FakeAnteDecoratorTwo{},
		FakeAnteDecoratorThree{},
	}
	tracedDecorators := utils.Map(anteDecorators, func(d sdk.AnteDecorator) sdk.AnteDecorator {
		return antedecorators.NewTracedAnteDecorator(d, nil)
	})
	chainedHandler := sdk.ChainAnteDecorators(tracedDecorators...)
	chainedHandler(sdk.NewContext(nil, tmproto.Header{}, false), FakeTx{}, false)
	require.Equal(t, "onetwothree", output)
}

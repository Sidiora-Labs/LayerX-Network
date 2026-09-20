package gov_test

import (
	"strings"
	"testing"

	tmproto "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"

	"github.com/sidiora-labs/paxeer-network/sdk/testutil/testdata"

	"github.com/stretchr/testify/require"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/gov"
	"github.com/sidiora-labs/paxeer-network/sdk/x/gov/keeper"
)

func TestInvalidMsg(t *testing.T) {
	k := keeper.Keeper{}
	h := gov.NewHandler(k)

	res, err := h(sdk.NewContext(nil, tmproto.Header{}, false), testdata.NewTestMsg())
	require.Error(t, err)
	require.Nil(t, res)
	require.True(t, strings.Contains(err.Error(), "unrecognized gov message type"))
}

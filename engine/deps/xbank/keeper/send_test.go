package keeper_test

import (
	"encoding/binary"
	"testing"

	"github.com/sidiora-labs/paxeer-network/engine/deps/xbank/keeper"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	paramtypes "github.com/sidiora-labs/paxeer-network/sdk/x/params/types"
	"github.com/stretchr/testify/require"
)

func TestBlockedAddr(t *testing.T) {
	k := keeper.NewBaseSendKeeper(nil, nil, nil, paramtypes.Subspace{}, map[string]bool{})
	txIndexBz := make([]byte, 8)
	binary.BigEndian.PutUint64(txIndexBz, uint64(5))
	addr := sdk.AccAddress(append(keeper.CoinbaseAddressPrefix, txIndexBz...))
	require.True(t, k.BlockedAddr(addr))
	addr[0] = 'q'
	require.False(t, k.BlockedAddr(addr))
}

package proxy

import (
	"context"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/holiman/uint256"
	"github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/stretchr/testify/require"
)

type testApp struct {
	types.BaseApplication
	checkTx func(context.Context, *types.RequestCheckTxV2) *types.ResponseCheckTxV2
}

func (app testApp) CheckTx(ctx context.Context, req *types.RequestCheckTxV2) *types.ResponseCheckTxV2 {
	return app.checkTx(ctx, req)
}

func TestCheckTxSafeReturnsErrorOnPanic(t *testing.T) {
	proxyApp := New(testApp{
		checkTx: func(context.Context, *types.RequestCheckTxV2) *types.ResponseCheckTxV2 {
			panic("boom")
		},
	}, NopMetrics())
	_, err := proxyApp.CheckTxSafe(t.Context(), &types.RequestCheckTxV2{Tx: []byte("tx")})
	require.Error(t, err)
}

func validEVMResponse() *types.ResponseCheckTxV2 {
	return &types.ResponseCheckTxV2{
		ResponseCheckTx:    &types.ResponseCheckTx{Code: types.CodeTypeOK},
		EVMHash:            common.HexToHash("0x123"),
		IsEVM:              true,
		PaxSenderAddress:   sdk.AccAddress("sender"),
		EVMRequiredBalance: *uint256.NewInt(1),
	}
}

func TestCheckTxSafeReturnsErrorOnMissingEVMHash(t *testing.T) {
	proxyApp := New(testApp{
		checkTx: func(context.Context, *types.RequestCheckTxV2) *types.ResponseCheckTxV2 {
			res := validEVMResponse()
			res.EVMHash = common.Hash{}
			return res
		},
	}, NopMetrics())
	_, err := proxyApp.CheckTxSafe(t.Context(), &types.RequestCheckTxV2{Tx: []byte("tx")})
	require.Error(t, err)
}

func TestCheckTxSafeReturnsErrorOnMissingPaxSenderAddress(t *testing.T) {
	proxyApp := New(testApp{
		checkTx: func(context.Context, *types.RequestCheckTxV2) *types.ResponseCheckTxV2 {
			res := validEVMResponse()
			res.PaxSenderAddress = nil
			return res
		},
	}, NopMetrics())
	_, err := proxyApp.CheckTxSafe(t.Context(), &types.RequestCheckTxV2{Tx: []byte("tx")})
	require.Error(t, err)
}

func TestCheckTxSafeAllowsValidEVMResponse(t *testing.T) {
	proxyApp := New(testApp{
		checkTx: func(context.Context, *types.RequestCheckTxV2) *types.ResponseCheckTxV2 {
			return validEVMResponse()
		},
	}, NopMetrics())
	_, err := proxyApp.CheckTxSafe(t.Context(), &types.RequestCheckTxV2{Tx: []byte("tx")})
	require.NoError(t, err)
}

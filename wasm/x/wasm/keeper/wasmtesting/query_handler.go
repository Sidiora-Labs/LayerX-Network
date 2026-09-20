package wasmtesting

import (
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	wasmvmtypes "github.com/sidiora-labs/paxeer-network/wasm-runtime/types"
)

type MockQueryHandler struct {
	HandleQueryFn func(ctx sdk.Context, request wasmvmtypes.QueryRequest, caller sdk.AccAddress) ([]byte, error)
}

func (m *MockQueryHandler) HandleQuery(ctx sdk.Context, caller sdk.AccAddress, request wasmvmtypes.QueryRequest) ([]byte, error) {
	if m.HandleQueryFn == nil {
		panic("not expected to be called")
	}
	return m.HandleQueryFn(ctx, request, caller)
}

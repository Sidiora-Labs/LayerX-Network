package wasmtesting

import (
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	wasmvmtypes "github.com/sidiora-labs/paxeer-network/wasm-runtime/types"
	"github.com/sidiora-labs/paxeer-network/wasm/x/wasm/types"
)

type MockMsgDispatcher struct {
	DispatchSubmessagesFn func(ctx sdk.Context, contractAddr sdk.AccAddress, ibcPort string, msgs []wasmvmtypes.SubMsg, info wasmvmtypes.MessageInfo, codeInfo types.CodeInfo) ([]byte, error)
}

func (m MockMsgDispatcher) DispatchSubmessages(ctx sdk.Context, contractAddr sdk.AccAddress, ibcPort string, msgs []wasmvmtypes.SubMsg, info wasmvmtypes.MessageInfo, codeInfo types.CodeInfo) ([]byte, error) {
	if m.DispatchSubmessagesFn == nil {
		panic("not expected to be called")
	}
	return m.DispatchSubmessagesFn(ctx, contractAddr, ibcPort, msgs, info, codeInfo)
}

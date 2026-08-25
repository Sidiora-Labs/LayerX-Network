package ante

import (
	"errors"

	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

type EVMRouterDecorator struct {
	defaultAnteHandler sdk.AnteHandler
	evmAnteHandler     sdk.AnteHandler
}

func NewEVMRouterDecorator(
	defaultAnteHandler sdk.AnteHandler,
	evmAnteHandler sdk.AnteHandler,
) *EVMRouterDecorator {
	return &EVMRouterDecorator{
		defaultAnteHandler: defaultAnteHandler,
		evmAnteHandler:     evmAnteHandler,
	}
}

func (r EVMRouterDecorator) AnteHandle(ctx sdk.Context, tx sdk.Tx, simulate bool) (sdk.Context, error) {
	if isEVM, err := IsEVMMessage(tx); err != nil {
		return ctx, err
	} else if isEVM {
		return r.evmAnteHandler(ctx, tx, simulate)
	}

	return r.defaultAnteHandler(ctx, tx, simulate)
}

func IsEVMMessage(tx sdk.Tx) (bool, error) {
	hasEVMMsg := false
	for _, msg := range tx.GetMsgs() {
		switch msg.(type) {
		case *types.MsgEVMTransaction:
			hasEVMMsg = true
		default:
			continue
		}
	}

	if hasEVMMsg && len(tx.GetMsgs()) != 1 {
		return false, errors.New("EVM tx must have exactly one message")
	}

	return hasEVMMsg, nil
}

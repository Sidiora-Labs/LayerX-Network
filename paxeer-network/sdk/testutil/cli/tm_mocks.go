package cli

import (
	"context"

	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	tmbytes "github.com/sidiora-labs/paxeer-network/consensus/libs/bytes"
	rpcclient "github.com/sidiora-labs/paxeer-network/consensus/rpc/client"
	rpcclientmock "github.com/sidiora-labs/paxeer-network/consensus/rpc/client/mock"
	"github.com/sidiora-labs/paxeer-network/consensus/rpc/coretypes"
	tmtypes "github.com/sidiora-labs/paxeer-network/consensus/types"

	"github.com/sidiora-labs/paxeer-network/sdk/client"
)

var _ client.TendermintRPC = (*MockTendermintRPC)(nil)

type MockTendermintRPC struct {
	rpcclientmock.Client

	responseQuery abci.ResponseQuery
}

// NewMockTendermintRPC returns a mock TendermintRPC implementation.
// It is used for CLI testing.
func NewMockTendermintRPC(respQuery abci.ResponseQuery, client rpcclientmock.Client) MockTendermintRPC {
	return MockTendermintRPC{
		Client:        client,
		responseQuery: respQuery,
	}
}

func (MockTendermintRPC) BroadcastTxSync(context.Context, tmtypes.Tx) (*coretypes.ResultBroadcastTx, error) {
	return &coretypes.ResultBroadcastTx{Code: 0}, nil
}

func (m MockTendermintRPC) ABCIQueryWithOptions(
	_ context.Context,
	_ string,
	_ tmbytes.HexBytes,
	_ rpcclient.ABCIQueryOptions,
) (*coretypes.ResultABCIQuery, error) {
	return &coretypes.ResultABCIQuery{Response: m.responseQuery}, nil
}

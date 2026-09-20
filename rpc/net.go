package evmrpc

import (
	"context"
	"fmt"

	"github.com/sidiora-labs/paxeer-network/consensus/libs/time"
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/sdk/client"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

type NetAPI struct {
	tmClient       client.LocalClient
	keeper         *keeper.Keeper
	ctxProvider    func(int64) sdk.Context
	connectionType ConnectionType
}

func NewNetAPI(tmClient client.LocalClient, k *keeper.Keeper, ctxProvider func(int64) sdk.Context, connectionType ConnectionType) *NetAPI {
	return &NetAPI{tmClient: tmClient, keeper: k, ctxProvider: ctxProvider, connectionType: connectionType}
}

func (i *NetAPI) Version(ctx context.Context) string {
	startTime := time.Now()
	defer recordMetrics(ctx, "net_version", i.connectionType, startTime)
	return fmt.Sprintf("%d", i.keeper.ChainID(i.ctxProvider(LatestCtxHeight)).Uint64())
}

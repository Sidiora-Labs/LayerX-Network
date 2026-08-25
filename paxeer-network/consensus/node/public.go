// Package node provides a high level wrapper around tendermint services.
package node

import (
	"context"
	"fmt"

	"github.com/paxeer-network/paxlog"
	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	"github.com/sidiora-labs/paxeer-network/consensus/config"
	"github.com/sidiora-labs/paxeer-network/consensus/internal/proxy"
	"github.com/sidiora-labs/paxeer-network/consensus/privval"
	"github.com/sidiora-labs/paxeer-network/consensus/rpc/client/local"
	tmtypes "github.com/sidiora-labs/paxeer-network/consensus/types"
	"go.opentelemetry.io/otel/sdk/trace"
)

var logger = paxlog.NewLogger("tendermint", "node")

// New constructs a tendermint node. The provided app runs in the same
// process as the tendermint node and will be wrapped in a local ABCI client
// inside this function. The final option is a pointer to a Genesis document:
// if the value is nil, the genesis document is read from the file specified
// in the config, and otherwise the node uses value of the final argument.
func New(
	ctx context.Context,
	conf *config.Config,
	restartEvent func(),
	app abci.Application,
	gen *tmtypes.GenesisDoc,
	tracerProviderOptions []trace.TracerProviderOption,
	nodeMetrics *NodeMetrics,
	consensusPolicy tmtypes.ConsensusPolicy,
) (local.NodeService, error) {
	proxyApp := proxy.New(app, nodeMetrics.proxy)
	nodeKey, err := tmtypes.LoadOrGenNodeKey(conf.NodeKeyFile())
	if err != nil {
		return nil, fmt.Errorf("failed to load or gen node key %s: %w", conf.NodeKeyFile(), err)
	}

	var genProvider genesisDocProvider
	switch gen {
	case nil:
		genProvider = defaultGenesisDocProviderFunc(conf)
	default:
		genProvider = func() (*tmtypes.GenesisDoc, error) { return gen, nil }
	}

	switch conf.Mode {
	case config.ModeFull, config.ModeValidator:
		pval, err := privval.LoadOrGenFilePV(conf.PrivValidator.KeyFile(), conf.PrivValidator.StateFile())
		if err != nil {
			return nil, err
		}

		return makeNode(
			ctx,
			conf,
			restartEvent,
			pval,
			nodeKey,
			proxyApp,
			genProvider,
			config.DefaultDBProvider,
			tracerProviderOptions,
			nodeMetrics,
			consensusPolicy,
		)
	case config.ModeSeed:
		return makeSeedNode(
			conf,
			config.DefaultDBProvider,
			nodeKey,
			genProvider,
			nodeMetrics,
		)
	default:
		return nil, fmt.Errorf("%q is not a valid mode", conf.Mode)
	}
}

package evmrpc

import (
	"errors"
	"strings"
	"sync"

	"github.com/ethereum/go-ethereum/rpc"

	tmutils "github.com/sidiora-labs/paxeer-network/consensus/libs/utils"
	evmCfg "github.com/sidiora-labs/paxeer-network/modules/evm/config"
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/node/legacyabci"
	evmrpcconfig "github.com/sidiora-labs/paxeer-network/rpc/config"
	"github.com/sidiora-labs/paxeer-network/rpc/stats"
	"github.com/sidiora-labs/paxeer-network/sdk/baseapp"
	"github.com/sidiora-labs/paxeer-network/sdk/client"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/storage/db_engine/types"
)

type ConnectionType string

var ConnectionTypeWS ConnectionType = "websocket"
var ConnectionTypeHTTP ConnectionType = "http"

const LocalAddress = "0.0.0.0"
const DefaultWebsocketMaxMessageSize = 10 * 1024 * 1024

type EVMServer interface {
	Start() error
	Stop()
}

func NewEVMHTTPServer(
	config evmrpcconfig.Config,
	tmClient client.LocalClient,
	k *keeper.Keeper,
	beginBlockKeepers legacyabci.BeginBlockKeepers,
	app *baseapp.BaseApp,
	antehandler sdk.AnteHandler,
	ctxProvider func(int64) sdk.Context,
	txConfigProvider func(int64) client.TxConfig,
	homeDir string,
	stateStore types.StateStore,
	traceCtxProviders ...TraceContextProvider,
) (EVMServer, error) {
	if tmClient == nil || k == nil || ctxProvider == nil || txConfigProvider == nil || app == nil {
		return nil, errors.New("EVM HTTP server dependencies are not configured")
	}
	ctx := ctxProvider(LatestCtxHeight)
	if config.EnableUnsafeKeyringRPC && evmCfg.IsLiveChainID(ctx) {
		return nil, errors.New("unsafe keyring RPC cannot be enabled on a live chain")
	}

	// Initialize global worker pool with configuration (metrics are embedded in pool)
	InitGlobalWorkerPool(config.WorkerPoolSize, config.WorkerQueueSize)

	// Get pool for logging and DB semaphore setup
	pool := GetGlobalWorkerPool()
	workerCount := pool.WorkerCount()
	queueSize := pool.QueueSize()

	// Set DB semaphore capacity in metrics (aligned with worker count)
	// Only set once to avoid races when multiple test servers start in parallel.
	pool.Metrics.DBSemaphoreCapacity.CompareAndSwap(0, int32(workerCount)) //nolint:gosec // G115: safe, max is 64

	debugEnabled := IsDebugMetricsEnabled()
	logger.Info("Started EVM RPC metrics exporter (interval: 5s)", "workers", workerCount, "queue", queueSize, "db_semaphore", workerCount, "debug_stdout", debugEnabled)
	if !debugEnabled {
		logger.Info("To enable debug metrics output to stdout, set EVM_DEBUG_METRICS=true")
	}

	// Initialize RPC tracker
	stats.InitRPCTracker(ctxProvider(LatestCtxHeight).Context(), config.RPCStatsInterval)

	httpServer := NewHTTPServer(rpc.HTTPTimeouts{
		ReadTimeout:       config.ReadTimeout,
		ReadHeaderTimeout: config.ReadHeaderTimeout,
		WriteTimeout:      config.WriteTimeout,
		IdleTimeout:       config.IdleTimeout,
	})
	methodTimeout := tmutils.Some(httpServer.timeouts.WriteTimeout)
	if err := httpServer.SetListenAddr(LocalAddress, config.HTTPPort); err != nil {
		return nil, err
	}
	simulateConfig := &SimulateConfig{
		GasCap:                       config.SimulationGasLimit,
		EVMTimeout:                   config.SimulationEVMTimeout,
		MaxConcurrentSimulationCalls: config.MaxConcurrentSimulationCalls,
	}
	watermarks := NewWatermarkManager(tmClient, ctxProvider, stateStore, k.ReceiptStore())

	globalBlockCache := NewBlockCache(3000)
	cacheCreationMutex := &sync.Mutex{}
	sendAPI := NewSendAPI(tmClient, txConfigProvider, &SendConfig{slow: config.Slow, keyringEnabled: config.EnableUnsafeKeyringRPC}, k, beginBlockKeepers, ctxProvider, homeDir, simulateConfig, app, antehandler, ConnectionTypeHTTP, methodTimeout, globalBlockCache, cacheCreationMutex, watermarks)

	traceCtxProvider := defaultTraceContextProvider(ctxProvider)
	if len(traceCtxProviders) > 0 && traceCtxProviders[0] != nil {
		traceCtxProvider = traceCtxProviders[0]
	}
	txAPI := NewTransactionAPI(tmClient, k, ctxProvider, txConfigProvider, homeDir, ConnectionTypeHTTP, methodTimeout, watermarks, globalBlockCache, cacheCreationMutex, config.EnableUnsafeKeyringRPC)
	debugAPI := NewDebugAPI(tmClient, k, beginBlockKeepers, ctxProvider, txConfigProvider, simulateConfig, app, antehandler, ConnectionTypeHTTP, config, globalBlockCache, cacheCreationMutex, watermarks)
	debugAPI.backend.SetTraceContextProvider(traceCtxProvider)
	if config.TraceBakeEnabled {
		StartTraceBakerForDebugAPI(debugAPI, TraceBakerConfig{
			Workers:      config.TraceBakeWorkers,
			QueueSize:    config.TraceBakeQueueSize,
			Tracers:      config.TraceBakeTracers,
			WindowBlocks: config.TraceBakeWindowBlocks,
			TipFn:        func() int64 { return ctxProvider(LatestCtxHeight).BlockHeight() },
		})
	}
	isPanicOrSyntheticTxFunc := debugAPI.isPanicOrSyntheticTx
	paxLegacyAllowlist := BuildPaxLegacyEnabledSet(config.EnabledLegacyPaxApis)

	paxTxAPI := NewPaxTransactionAPI(tmClient, k, ctxProvider, txConfigProvider, homeDir, ConnectionTypeHTTP, methodTimeout, isPanicOrSyntheticTxFunc, watermarks, globalBlockCache, cacheCreationMutex, config.EnableUnsafeKeyringRPC)
	paxDebugAPI := NewPaxDebugAPI(tmClient, k, beginBlockKeepers, ctxProvider, txConfigProvider, simulateConfig, app, antehandler, ConnectionTypeHTTP, config, globalBlockCache, cacheCreationMutex, watermarks)
	paxDebugAPI.backend.SetTraceContextProvider(traceCtxProvider)

	// DB semaphore aligned with worker count
	dbReadSemaphore := make(chan struct{}, workerCount)
	globalLogSlicePool := NewLogSlicePool()
	apis := []rpc.API{
		{
			Namespace: "echo",
			Service:   NewEchoAPI(),
		},
		{
			Namespace: "eth",
			Service:   NewBlockAPI(tmClient, k, ctxProvider, txConfigProvider, ConnectionTypeHTTP, watermarks, globalBlockCache, cacheCreationMutex),
		},
		{
			Namespace: "pax",
			Service:   NewPaxBlockAPI(tmClient, k, ctxProvider, txConfigProvider, ConnectionTypeHTTP, watermarks, globalBlockCache, cacheCreationMutex),
		},
		{
			Namespace: "pax2",
			Service:   NewPax2BlockAPI(tmClient, k, ctxProvider, txConfigProvider, ConnectionTypeHTTP, watermarks, globalBlockCache, cacheCreationMutex),
		},
		{
			Namespace: "eth",
			Service:   txAPI,
		},
		{
			Namespace: "pax",
			Service:   paxTxAPI,
		},
		{
			Namespace: "eth",
			Service:   NewStateAPI(tmClient, k, ctxProvider, ConnectionTypeHTTP, watermarks),
		},
		{
			Namespace: "eth",
			Service:   NewInfoAPI(tmClient, k, ctxProvider, txConfigProvider, homeDir, config.MaxBlocksForLog, ConnectionTypeHTTP, txConfigProvider(LatestCtxHeight).TxDecoder(), watermarks, config.EnableUnsafeKeyringRPC),
		},
		{
			Namespace: "eth",
			Service:   sendAPI,
		},
		{
			Namespace: "eth",
			Service:   NewSimulationAPI(ctxProvider, k, beginBlockKeepers, txConfigProvider, tmClient, simulateConfig, app, antehandler, ConnectionTypeHTTP, globalBlockCache, cacheCreationMutex, watermarks),
		},
		{
			Namespace: "net",
			Service:   NewNetAPI(tmClient, k, ctxProvider, ConnectionTypeHTTP),
		},
		{
			Namespace: "eth",
			Service: NewFilterAPI(
				tmClient,
				k,
				ctxProvider,
				txConfigProvider,
				&FilterConfig{timeout: config.FilterTimeout, maxLog: config.MaxLogNoBlock, maxBlock: config.MaxBlocksForLog},
				ConnectionTypeHTTP,
				"eth",
				dbReadSemaphore,
				globalBlockCache,
				cacheCreationMutex,
				globalLogSlicePool,
				watermarks,
			),
		},
		{
			Namespace: "pax",
			Service: NewFilterAPI(
				tmClient,
				k,
				ctxProvider,
				txConfigProvider,
				&FilterConfig{timeout: config.FilterTimeout, maxLog: config.MaxLogNoBlock, maxBlock: config.MaxBlocksForLog},
				ConnectionTypeHTTP,
				"pax",
				dbReadSemaphore,
				globalBlockCache,
				cacheCreationMutex,
				globalLogSlicePool,
				watermarks,
			),
		},
		{
			Namespace: "pax",
			Service:   NewAssociationAPI(tmClient, k, ctxProvider, txConfigProvider, ConnectionTypeHTTP, watermarks),
		},
		{
			Namespace: "txpool",
			Service:   NewTxPoolAPI(tmClient, k, ctxProvider, txConfigProvider, &TxPoolConfig{maxNumTxs: int(config.MaxTxPoolTxs)}, ConnectionTypeHTTP), //nolint:gosec
		},
		{
			Namespace: "web3",
			Service:   &Web3API{},
		},
		{
			Namespace: "debug",
			Service:   debugAPI,
		},
		{
			Namespace: "pax",
			Service:   paxDebugAPI,
		},
	}
	// Test API can only exist on non-live chain IDs.  These APIs instrument certain overrides.
	if config.EnableTestAPI && !evmCfg.IsLiveChainID(ctx) {
		logger.Info("Enabling Test EVM APIs")
		apis = append(apis, rpc.API{
			Namespace: "test",
			Service:   NewTestAPI(),
		})
	} else {
		logger.Info("Disabling Test EVM APIs", "liveChainID", evmCfg.IsLiveChainID(ctx), "enableTestAPI", config.EnableTestAPI)
	}

	if err := httpServer.EnableRPC(apis, HTTPConfig{
		CorsAllowedOrigins: strings.Split(config.CORSOrigins, ","),
		Vhosts:             []string{"*"},
		DenyList:           config.DenyList,
		PaxLegacyAllowlist: paxLegacyAllowlist,
	}); err != nil {
		return nil, err
	}

	return httpServer, nil
}

func NewEVMWebSocketServer(
	config evmrpcconfig.Config,
	tmClient client.LocalClient,
	k *keeper.Keeper,
	beginBlockKeepers legacyabci.BeginBlockKeepers,
	app *baseapp.BaseApp,
	antehandler sdk.AnteHandler,
	ctxProvider func(int64) sdk.Context,
	txConfigProvider func(int64) client.TxConfig,
	homeDir string,
	stateStore types.StateStore,
	blockHeaderNotifier *BlockHeaderNotifier,
) (EVMServer, error) {
	if tmClient == nil || k == nil || ctxProvider == nil || txConfigProvider == nil || app == nil {
		return nil, errors.New("EVM WebSocket server dependencies are not configured")
	}
	ctx := ctxProvider(LatestCtxHeight)
	if config.EnableUnsafeKeyringRPC && evmCfg.IsLiveChainID(ctx) {
		return nil, errors.New("unsafe keyring RPC cannot be enabled on a live chain")
	}
	// Initialize global worker pool with configuration (metrics are embedded in pool)
	// This is idempotent - if HTTP server already initialized it, this is a no-op
	InitGlobalWorkerPool(config.WorkerPoolSize, config.WorkerQueueSize)

	// Initialize WebSocket tracker.
	stats.InitWSTracker(ctxProvider(LatestCtxHeight).Context(), config.RPCStatsInterval)

	httpServer := NewHTTPServer(rpc.HTTPTimeouts{
		ReadTimeout:       config.ReadTimeout,
		ReadHeaderTimeout: config.ReadHeaderTimeout,
		WriteTimeout:      config.WriteTimeout,
		IdleTimeout:       config.IdleTimeout,
	})
	methodTimeout := tmutils.Some(httpServer.timeouts.WriteTimeout)
	if err := httpServer.SetListenAddr(LocalAddress, config.WSPort); err != nil {
		return nil, err
	}
	simulateConfig := &SimulateConfig{
		GasCap:                       config.SimulationGasLimit,
		EVMTimeout:                   config.SimulationEVMTimeout,
		MaxConcurrentSimulationCalls: config.MaxConcurrentSimulationCalls,
	}
	watermarks := NewWatermarkManager(tmClient, ctxProvider, stateStore, k.ReceiptStore())
	// DB semaphore aligned with worker count
	dbReadSemaphore := make(chan struct{}, GetGlobalWorkerPool().WorkerCount())
	globalBlockCache := NewBlockCache(3000)
	cacheCreationMutex := &sync.Mutex{}
	globalLogSlicePool := NewLogSlicePool()
	subscriptionAPI, err := NewSubscriptionAPI(tmClient, k, ctxProvider, &LogFetcher{
		tmClient:           tmClient,
		k:                  k,
		ctxProvider:        ctxProvider,
		txConfigProvider:   txConfigProvider,
		dbReadSemaphore:    dbReadSemaphore,
		globalBlockCache:   globalBlockCache,
		cacheCreationMutex: cacheCreationMutex,
		globalLogSlicePool: globalLogSlicePool,
		watermarks:         watermarks,
	}, &SubscriptionConfig{subscriptionCapacity: 100, newHeadLimit: config.MaxSubscriptionsNewHead}, &FilterConfig{timeout: config.FilterTimeout, maxLog: config.MaxLogNoBlock, maxBlock: config.MaxBlocksForLog}, ConnectionTypeWS, blockHeaderNotifier)
	if err != nil {
		return nil, err
	}
	apis := []rpc.API{
		{
			Namespace: "echo",
			Service:   NewEchoAPI(),
		},
		{
			Namespace: "eth",
			Service:   NewBlockAPI(tmClient, k, ctxProvider, txConfigProvider, ConnectionTypeWS, watermarks, globalBlockCache, cacheCreationMutex),
		},
		{
			Namespace: "eth",
			Service:   NewTransactionAPI(tmClient, k, ctxProvider, txConfigProvider, homeDir, ConnectionTypeWS, methodTimeout, watermarks, globalBlockCache, cacheCreationMutex, config.EnableUnsafeKeyringRPC),
		},
		{
			Namespace: "eth",
			Service:   NewStateAPI(tmClient, k, ctxProvider, ConnectionTypeWS, watermarks),
		},
		{
			Namespace: "eth",
			Service:   NewInfoAPI(tmClient, k, ctxProvider, txConfigProvider, homeDir, config.MaxBlocksForLog, ConnectionTypeWS, txConfigProvider(LatestCtxHeight).TxDecoder(), watermarks, config.EnableUnsafeKeyringRPC),
		},
		{
			Namespace: "eth",
			Service:   NewSendAPI(tmClient, txConfigProvider, &SendConfig{slow: config.Slow, keyringEnabled: config.EnableUnsafeKeyringRPC}, k, beginBlockKeepers, ctxProvider, homeDir, simulateConfig, app, antehandler, ConnectionTypeWS, methodTimeout, globalBlockCache, cacheCreationMutex, watermarks),
		},
		{
			Namespace: "eth",
			Service:   NewSimulationAPI(ctxProvider, k, beginBlockKeepers, txConfigProvider, tmClient, simulateConfig, app, antehandler, ConnectionTypeWS, globalBlockCache, cacheCreationMutex, watermarks),
		},
		{
			Namespace: "net",
			Service:   NewNetAPI(tmClient, k, ctxProvider, ConnectionTypeWS),
		},
		{
			Namespace: "eth",
			Service:   subscriptionAPI,
		},
		{
			Namespace: "web3",
			Service:   &Web3API{},
		},
	}

	wsConfig := WsConfig{Origins: strings.Split(config.WSOrigins, ",")}
	wsConfig.readLimit = DefaultWebsocketMaxMessageSize
	if err := httpServer.EnableWS(apis, wsConfig); err != nil {
		return nil, err
	}

	return httpServer, nil
}

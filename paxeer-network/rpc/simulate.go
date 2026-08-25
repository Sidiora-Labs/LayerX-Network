package evmrpc

import (
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"math"
	"math/big"
	"strings"
	"sync"
	"time"

	"golang.org/x/sync/semaphore"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/common/hexutil"
	"github.com/ethereum/go-ethereum/consensus"
	"github.com/ethereum/go-ethereum/consensus/ethash"
	"github.com/ethereum/go-ethereum/core"
	ethtypes "github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/ethereum/go-ethereum/eth"
	"github.com/ethereum/go-ethereum/eth/tracers"
	"github.com/ethereum/go-ethereum/eth/tracers/tracersutils"
	"github.com/ethereum/go-ethereum/ethdb"
	"github.com/ethereum/go-ethereum/export"
	"github.com/ethereum/go-ethereum/params"
	"github.com/ethereum/go-ethereum/rpc"
	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	"github.com/sidiora-labs/paxeer-network/consensus/rpc/coretypes"
	tmtypes "github.com/sidiora-labs/paxeer-network/consensus/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types/ethtx"
	"github.com/sidiora-labs/paxeer-network/node/legacyabci"
	"github.com/sidiora-labs/paxeer-network/precompiles/wasmd"
	"github.com/sidiora-labs/paxeer-network/sdk/baseapp"
	"github.com/sidiora-labs/paxeer-network/sdk/client"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/utils"
)

type CtxIsWasmdPrecompileCallKeyType string

const CtxIsWasmdPrecompileCallKey CtxIsWasmdPrecompileCallKeyType = "CtxIsWasmdPrecompileCallKey"

type SimulationAPI struct {
	backend        *Backend
	connectionType ConnectionType
	requestLimiter *semaphore.Weighted
}

func NewSimulationAPI(
	ctxProvider func(int64) sdk.Context,
	keeper *keeper.Keeper,
	beginBlockKeepers legacyabci.BeginBlockKeepers,
	txConfigProvider func(int64) client.TxConfig,
	tmClient client.LocalClient,
	config *SimulateConfig,
	app *baseapp.BaseApp,
	antehandler sdk.AnteHandler,
	connectionType ConnectionType,
	globalBlockCache BlockCache,
	cacheCreationMutex *sync.Mutex,
	watermarks *WatermarkManager,
) *SimulationAPI {
	api := &SimulationAPI{
		backend:        NewBackend(ctxProvider, keeper, beginBlockKeepers, txConfigProvider, tmClient, config, app, antehandler, globalBlockCache, cacheCreationMutex, watermarks),
		connectionType: connectionType,
	}
	if config.MaxConcurrentSimulationCalls > 0 {
		api.requestLimiter = semaphore.NewWeighted(int64(config.MaxConcurrentSimulationCalls))
	}
	return api
}

type AccessListResult struct {
	Accesslist *ethtypes.AccessList `json:"accessList"`
	Error      string               `json:"error,omitempty"`
	GasUsed    hexutil.Uint64       `json:"gasUsed"`
}

func (s *SimulationAPI) CreateAccessList(ctx context.Context, args export.TransactionArgs, blockNrOrHash *rpc.BlockNumberOrHash) (result *AccessListResult, returnErr error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, "eth_createAccessList", s.connectionType, startTime, returnErr, recover())
	}()
	bNrOrHash := rpc.BlockNumberOrHashWithNumber(rpc.PendingBlockNumber)
	if blockNrOrHash != nil {
		bNrOrHash = *blockNrOrHash
	}
	ctx = context.WithValue(ctx, CtxIsWasmdPrecompileCallKey, wasmd.IsWasmdCall(args.To))
	acl, gasUsed, vmerr, err := export.AccessList(ctx, s.backend, bNrOrHash, args, nil)
	if err != nil {
		return nil, err
	}
	result = &AccessListResult{Accesslist: &acl, GasUsed: hexutil.Uint64(gasUsed)}
	if vmerr != nil {
		result.Error = vmerr.Error()
	}
	return result, nil
}

func (s *SimulationAPI) EstimateGas(ctx context.Context, args export.TransactionArgs, blockNrOrHash *rpc.BlockNumberOrHash, overrides *export.StateOverride) (result hexutil.Uint64, returnErr error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, "eth_estimateGas", s.connectionType, startTime, returnErr, recover())
	}()
	/* ---------- fail‑fast limiter ---------- */
	if s.requestLimiter != nil {
		if !s.requestLimiter.TryAcquire(1) {
			returnErr = errors.New("eth_estimateGas rejected due to rate limit: server busy")
			return
		}
		defer s.requestLimiter.Release(1)
	}
	bNrOrHash := rpc.BlockNumberOrHashWithNumber(rpc.LatestBlockNumber)
	if blockNrOrHash != nil {
		bNrOrHash = *blockNrOrHash
	}
	ctx = context.WithValue(ctx, CtxIsWasmdPrecompileCallKey, wasmd.IsWasmdCall(args.To))
	estimate, err := export.DoEstimateGas(ctx, s.backend, args, bNrOrHash, overrides, nil, s.backend.RPCGasCap())
	return estimate, err
}

func (s *SimulationAPI) EstimateGasAfterCalls(ctx context.Context, args export.TransactionArgs, calls []export.TransactionArgs, blockNrOrHash *rpc.BlockNumberOrHash, overrides *export.StateOverride) (result hexutil.Uint64, returnErr error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, "eth_estimateGasAfterCalls", s.connectionType, startTime, returnErr, recover())
	}()
	/* ---------- fail‑fast limiter ---------- */
	if s.requestLimiter != nil {
		if !s.requestLimiter.TryAcquire(1) {
			returnErr = errors.New("eth_estimateGasAfterCalls rejected due to rate limit: server busy")
			return
		}
		defer s.requestLimiter.Release(1)
	}
	bNrOrHash := rpc.BlockNumberOrHashWithNumber(rpc.LatestBlockNumber)
	if blockNrOrHash != nil {
		bNrOrHash = *blockNrOrHash
	}
	ctx = context.WithValue(ctx, CtxIsWasmdPrecompileCallKey, wasmd.IsWasmdCall(args.To))
	estimate, err := export.DoEstimateGasAfterCalls(ctx, s.backend, args, calls, bNrOrHash, overrides, s.backend.RPCEVMTimeout(), s.backend.RPCGasCap())
	return estimate, err
}

func (s *SimulationAPI) Call(ctx context.Context, args export.TransactionArgs, blockNrOrHash *rpc.BlockNumberOrHash, overrides *export.StateOverride, blockOverrides *export.BlockOverrides) (result hexutil.Bytes, returnErr error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, "eth_call", s.connectionType, startTime, returnErr, recover())
	}()
	/* ---------- fail‑fast limiter ---------- */
	if s.requestLimiter != nil {
		if !s.requestLimiter.TryAcquire(1) {
			returnErr = errors.New("eth_call rejected due to rate limit: server busy")
			return
		}
		defer s.requestLimiter.Release(1)
	}
	defer func() {
		if r := recover(); r != nil {
			if strings.Contains(fmt.Sprintf("%s", r), "Int overflow") {
				returnErr = errors.New("error: balance override overflow")
			} else {
				returnErr = fmt.Errorf("something went wrong: %v", r)
			}
		}
	}()
	if blockNrOrHash == nil {
		latest := rpc.BlockNumberOrHashWithNumber(rpc.LatestBlockNumber)
		blockNrOrHash = &latest
	}
	ctx = context.WithValue(ctx, CtxIsWasmdPrecompileCallKey, wasmd.IsWasmdCall(args.To))
	callResult, err := export.DoCall(ctx, s.backend, args, *blockNrOrHash, overrides, blockOverrides, s.backend.RPCEVMTimeout(), s.backend.RPCGasCap())
	if err != nil {
		return nil, err
	}
	// If the result contains a revert reason, try to unpack and return it.
	if len(callResult.Revert()) > 0 {
		return nil, NewRevertError(callResult)
	}
	return callResult.Return(), callResult.Err
}

func NewRevertError(result *core.ExecutionResult) *RevertError {
	reason, errUnpack := abi.UnpackRevert(result.Revert())
	err := errors.New("execution reverted")
	if errUnpack == nil {
		err = fmt.Errorf("execution reverted: %v", reason)
	}
	return &RevertError{
		error:  err,
		reason: hexutil.Encode(result.Revert()),
	}
}

// RevertError is an API error that encompasses an EVM revertal with JSON error
// code and a binary data blob.
type RevertError struct {
	error
	reason string // revert reason hex encoded
}

// ErrorCode returns the JSON error code for a revertal.
// See: https://github.com/ethereum/wiki/wiki/JSON-RPC-Error-Codes-Improvement-Proposal
func (e *RevertError) ErrorCode() int {
	return 3
}

// ErrorData returns the hex encoded revert reason.
func (e *RevertError) ErrorData() interface{} {
	return e.reason
}

type SimulateConfig struct {
	GasCap                       uint64
	EVMTimeout                   time.Duration
	MaxConcurrentSimulationCalls int
}

var _ tracers.Backend = (*Backend)(nil)

type Backend struct {
	*eth.EthAPIBackend
	ctxProvider        func(int64) sdk.Context
	traceCtxProvider   TraceContextProvider
	txConfigProvider   func(int64) client.TxConfig
	keeper             *keeper.Keeper
	tmClient           client.LocalClient
	config             *SimulateConfig
	app                *baseapp.BaseApp
	beginBlockKeepers  legacyabci.BeginBlockKeepers
	antehandler        sdk.AnteHandler
	globalBlockCache   BlockCache
	cacheCreationMutex *sync.Mutex
	watermarks         *WatermarkManager
	headerMu           sync.RWMutex
	lastHeader         *ethtypes.Header
}

type TraceContextProvider func(int64) (sdk.Context, func())

const currentHeaderLookupTimeout = 2 * time.Second

func NewBackend(
	ctxProvider func(int64) sdk.Context,
	keeper *keeper.Keeper,
	beginBlockKeepers legacyabci.BeginBlockKeepers,
	txConfigProvider func(int64) client.TxConfig,
	tmClient client.LocalClient,
	config *SimulateConfig,
	app *baseapp.BaseApp,
	antehandler sdk.AnteHandler,
	globalBlockCache BlockCache,
	cacheCreationMutex *sync.Mutex,
	watermarks *WatermarkManager,
) *Backend {
	return &Backend{
		ctxProvider:        ctxProvider,
		traceCtxProvider:   defaultTraceContextProvider(ctxProvider),
		keeper:             keeper,
		beginBlockKeepers:  beginBlockKeepers,
		txConfigProvider:   txConfigProvider,
		tmClient:           tmClient,
		config:             config,
		app:                app,
		antehandler:        antehandler,
		globalBlockCache:   globalBlockCache,
		cacheCreationMutex: cacheCreationMutex,
		watermarks:         watermarks,
	}
}

func defaultTraceContextProvider(ctxProvider func(int64) sdk.Context) TraceContextProvider {
	return func(height int64) (sdk.Context, func()) {
		return ctxProvider(height), func() {}
	}
}

func (b *Backend) isV65ActiveAtHeight(height int64) bool {
	ctx := b.ctxProvider(LatestCtxHeight).WithGasMeter(sdk.NewInfiniteGasMeter(1, 1))
	return b.keeper.UpgradeKeeper().IsUpgradeActiveAtHeight(ctx, "v6.5", height)
}

func (b *Backend) SetTraceContextProvider(provider TraceContextProvider) {
	if provider != nil {
		b.traceCtxProvider = provider
	}
}

func (b *Backend) StateAndHeaderByNumberOrHash(ctx context.Context, blockNrOrHash rpc.BlockNumberOrHash) (vm.StateDB, *ethtypes.Header, error) {
	tmBlock, isLatestBlock, err := b.getBlockByNumberOrHash(ctx, blockNrOrHash)
	if err != nil {
		return nil, nil, err
	}
	height := tmBlock.Block.Height
	isWasmdCall, ok := ctx.Value(CtxIsWasmdPrecompileCallKey).(bool)
	sdkCtx := b.ctxProvider(height).WithIsEVM(true).WithEVMEntryViaWasmdPrecompile(ok && isWasmdCall)
	if !isLatestBlock {
		// no need to check version for latest block
		if err := CheckVersion(sdkCtx, b.keeper); err != nil {
			return nil, nil, err
		}
	}
	header, err := b.getHeader(ctx, tmBlock)
	if err != nil {
		return nil, nil, err
	}
	return state.NewDBImpl(sdkCtx, b.keeper, true), header, nil
}

func (b *Backend) GetTransaction(ctx context.Context, txHash common.Hash) (found bool, tx *ethtypes.Transaction, blockHash common.Hash, blockNumber uint64, index uint64, err error) {
	sdkCtx := b.ctxProvider(LatestCtxHeight)
	receipt, err := b.keeper.GetReceipt(sdkCtx, txHash)
	if err != nil {
		return false, nil, common.Hash{}, 0, 0, err
	}
	if receipt.BlockNumber > uint64(math.MaxInt64) {
		return false, nil, common.Hash{}, 0, 0, errors.New("block number exceeds int64 max value")
	}

	txHeight := int64(receipt.BlockNumber)
	block, err := blockByNumberRespectingWatermarks(ctx, b.tmClient, b.watermarks, &txHeight, 1)
	if err != nil {
		return false, nil, common.Hash{}, 0, 0, err
	}
	if int(receipt.TransactionIndex) >= len(block.Block.Txs) {
		return false, nil, common.Hash{}, 0, 0, errors.New("transaction index out of range")
	}
	txIndex := hexutil.Uint(receipt.TransactionIndex)
	tmTx := block.Block.Txs[txIndex]
	tx = getEthTxForTxBz(tmTx, traceCompatTxDecoder(b.txConfigProvider(block.Block.Height), b.isV65ActiveAtHeight(block.Block.Height)))
	// Use BlockID.Hash rather than Header.Hash(): under CometBFT they
	// are equal, but under Autobahn the Block.Header returned by /block
	// is sparse (the GigaRouter's translateGlobalBlock only populates
	// ChainID/Height/Time), so Header.Hash() recomputes a Merkle root
	// that doesn't match any stored value — and downstream
	// debug_traceTransaction fails with "block not found by hash" when
	// it tries to round-trip this value through BlockByHash.
	// BlockID.Hash carries the actual block hash that the EVM receipt
	// store recorded during FinalizeBlock: same on both engines,
	// correct under both.
	blockHash = common.BytesToHash(block.BlockID.Hash)
	return true, tx, blockHash, uint64(txHeight), uint64(txIndex), nil //nolint:gosec
}

func (b *Backend) ChainDb() ethdb.Database {
	return unsupportedEthereumChainDatabase
}

func (b *Backend) ConvertBlockNumber(ctx context.Context, bn rpc.BlockNumber) (int64, error) {
	blockNum := bn.Int64()
	switch blockNum {
	case rpc.SafeBlockNumber.Int64(), rpc.FinalizedBlockNumber.Int64(), rpc.LatestBlockNumber.Int64():
		if b.ctxProvider == nil {
			return 0, errors.New("state context provider is not configured")
		}
		blockNum = b.ctxProvider(LatestCtxHeight).BlockHeight()
	case rpc.EarliestBlockNumber.Int64():
		if b.tmClient == nil {
			return 0, errors.New("consensus client is not configured")
		}
		genesisRes, err := b.tmClient.Genesis(ctx)
		if err != nil {
			return 0, fmt.Errorf("get genesis information: %w", err)
		}
		if genesisRes == nil || genesisRes.Genesis == nil {
			return 0, errors.New("consensus client returned empty genesis information")
		}
		blockNum = genesisRes.Genesis.InitialHeight
	case rpc.PendingBlockNumber.Int64():
		return 0, errors.New("tracing on the pending block is not supported")
	default:
		if blockNum < 0 {
			return 0, fmt.Errorf("unsupported block number %d", blockNum)
		}
	}
	return blockNum, nil
}

func (b *Backend) BlockByNumber(ctx context.Context, bn rpc.BlockNumber) (*ethtypes.Block, []tracersutils.TraceBlockMetadata, error) {
	blockNum, err := b.ConvertBlockNumber(ctx, bn)
	if err != nil {
		return nil, nil, err
	}
	tmBlock, err := blockByNumberRespectingWatermarks(ctx, b.tmClient, b.watermarks, &blockNum, 1)
	if err != nil {
		return nil, nil, err
	}
	sdkCtx := b.ctxProvider(LatestCtxHeight)
	var txs []*ethtypes.Transaction
	var metadata []tracersutils.TraceBlockMetadata
	traceTxConfigProvider := traceCompatTxConfigProvider(b.txConfigProvider, b.isV65ActiveAtHeight)
	msgs := filterTransactions(b.keeper, b.ctxProvider, traceTxConfigProvider, tmBlock, false, false, b.cacheCreationMutex, b.globalBlockCache)
	idxToMsgs := make(map[int]sdk.Msg, len(msgs))
	for _, msg := range msgs {
		idxToMsgs[msg.index] = msg.msg
	}
	for i := range tmBlock.Block.Txs {
		decoded, err := traceCompatTxDecoder(b.txConfigProvider(tmBlock.Block.Height), b.isV65ActiveAtHeight(tmBlock.Block.Height))(tmBlock.Block.Txs[i])
		if err != nil {
			return nil, nil, err
		}
		isPrioritized := utils.IsTxPrioritized(decoded)
		if isPrioritized {
			continue
		}
		shouldTrace := false
		if msg, ok := idxToMsgs[i]; ok {
			switch m := msg.(type) {
			case *types.MsgEVMTransaction:
				if m.IsAssociateTx() {
					continue
				}
				ethtx, _ := m.AsTransaction()
				if ethtx == nil {
					// AsTransaction may return nil if it fails to unpack the tx data.
					continue
				}
				receipt, found := getOrSetCachedReceipt(b.cacheCreationMutex, b.globalBlockCache, sdkCtx, b.keeper, tmBlock, ethtx.Hash())
				if !found {
					continue
				}
				TraceReceiptIfApplicable(ctx, receipt)
				shouldTrace = true
				metadata = append(metadata, tracersutils.TraceBlockMetadata{
					ShouldIncludeInTraceResult: true,
					IdxInEthBlock:              len(txs),
				})
				txs = append(txs, ethtx)
			}
		}
		if !shouldTrace {
			txBytes := tmBlock.Block.Txs[i]
			txHash := sha256.Sum256(txBytes)
			metadata = append(metadata, tracersutils.TraceBlockMetadata{
				ShouldIncludeInTraceResult: false,
				IdxInEthBlock:              -1,
				TraceRunnable: func(sd vm.StateDB) {
					typedStateDB := state.GetDBImpl(sd)
					_ = b.app.DeliverTx(typedStateDB.Ctx(), abci.RequestDeliverTxV2{Tx: txBytes}, decoded, txHash)
				},
			})
		}
	}
	header, err := b.getHeader(ctx, tmBlock)
	if err != nil {
		return nil, nil, err
	}
	block := &ethtypes.Block{
		Header_: header,
		Txs:     txs,
	}
	block.OverwriteHash(common.BytesToHash(tmBlock.BlockID.Hash))
	return block, metadata, nil
}

func (b *Backend) BlockByHash(ctx context.Context, hash common.Hash) (*ethtypes.Block, []tracersutils.TraceBlockMetadata, error) {
	tmBlock, err := blockByHashRespectingWatermarks(ctx, b.tmClient, b.watermarks, hash.Bytes(), 1)
	if err != nil {
		return nil, nil, err
	}
	blockNumber := rpc.BlockNumber(tmBlock.Block.Height)
	return b.BlockByNumber(ctx, blockNumber)
}

func (b *Backend) RPCGasCap() uint64 { return b.config.GasCap }

func (b *Backend) RPCEVMTimeout() time.Duration { return b.config.EVMTimeout }

func (b *Backend) chainConfigForHeight(height int64) *params.ChainConfig {
	ctx := b.ctxProvider(height)
	sstore := b.keeper.GetSstoreSetGasEIP2200(ctx)
	return types.DefaultChainConfig().EthereumConfigWithSstore(b.keeper.ChainID(ctx), &sstore)
}

func (b *Backend) ChainConfig() *params.ChainConfig {
	return b.chainConfigForHeight(LatestCtxHeight)
}

func (b *Backend) ChainConfigAtHeight(height int64) *params.ChainConfig {
	return b.chainConfigForHeight(height)
}

func (b *Backend) GetPoolNonce(_ context.Context, addr common.Address) (uint64, error) {
	return state.NewDBImpl(b.ctxProvider(LatestCtxHeight), b.keeper, true).GetNonce(addr), nil
}

func (b *Backend) Engine() consensus.Engine {
	return &Engine{ctxProvider: b.ctxProvider, keeper: b.keeper}
}

func (b *Backend) HeaderByNumber(ctx context.Context, bn rpc.BlockNumber) (*ethtypes.Header, error) {
	tmBlock, _, err := b.getBlockByNumberOrHash(ctx, rpc.BlockNumberOrHashWithNumber(bn))
	if err != nil {
		return nil, err
	}
	return b.getHeader(ctx, tmBlock)
}

func (b *Backend) StateAtTransaction(ctx context.Context, block *ethtypes.Block, txIndex int, reexec uint64) (*ethtypes.Transaction, vm.BlockContext, vm.StateDB, tracers.StateReleaseFunc, error) {
	emptyRelease := func() {}
	stateDB, txs, release, err := b.replayTransactionTillIndex(ctx, block, txIndex-1, b.traceCtxProvider)
	if err != nil {
		return nil, vm.BlockContext{}, nil, emptyRelease, err
	}
	success := false
	defer func() {
		if !success {
			release()
		}
	}()
	blockContext, err := b.keeper.GetVMBlockContext(stateDB.(*state.DBImpl).Ctx(), b.keeper.GetGasPool())
	if err != nil {
		return nil, vm.BlockContext{}, nil, emptyRelease, err
	}
	if txIndex > len(txs)-1 {
		return nil, vm.BlockContext{}, nil, emptyRelease, errors.New("transaction not found")
	}
	tx := txs[txIndex]
	sdkTx, err := traceCompatTxDecoder(b.txConfigProvider(block.Number().Int64()), b.isV65ActiveAtHeight(block.Number().Int64()))(tx)
	if err != nil {
		return nil, vm.BlockContext{}, nil, emptyRelease, fmt.Errorf("decode transaction %d at block %d: %w", txIndex, block.Number().Int64(), err)
	}
	if utils.IsTxPrioritized(sdkTx) {
		return nil, vm.BlockContext{}, nil, emptyRelease, errors.New("cannot trace oracle tx")
	}
	var evmMsg *types.MsgEVMTransaction
	if msgs := sdkTx.GetMsgs(); len(msgs) != 1 {
		return nil, vm.BlockContext{}, nil, emptyRelease, fmt.Errorf("cannot replay non-EVM transaction %d at block %d", txIndex, block.Number().Int64())
	} else if msg, ok := msgs[0].(*types.MsgEVMTransaction); !ok {
		return nil, vm.BlockContext{}, nil, emptyRelease, fmt.Errorf("cannot replay non-EVM transaction %d at block %d", txIndex, block.Number().Int64())
	} else {
		evmMsg = msg
	}
	ethTx, txData := evmMsg.AsTransaction()
	if ethTx == nil {
		if txData == nil {
			return nil, vm.BlockContext{}, nil, emptyRelease, fmt.Errorf("transaction %d at block %d contains malformed EVM data", txIndex, block.Number().Int64())
		}
		return nil, vm.BlockContext{}, nil, emptyRelease, fmt.Errorf("transaction %d at block %d has no Ethereum transaction representation", txIndex, block.Number().Int64())
	}
	success = true
	return ethTx, *blockContext, stateDB, release, nil
}

func (b *Backend) ReplayTransactionTillIndex(ctx context.Context, block *ethtypes.Block, txIndex int) (vm.StateDB, tmtypes.Txs, error) {
	stateDB, txs, _, err := b.replayTransactionTillIndex(ctx, block, txIndex, defaultTraceContextProvider(b.ctxProvider))
	return stateDB, txs, err
}

func (b *Backend) replayTransactionTillIndex(ctx context.Context, block *ethtypes.Block, txIndex int, ctxProvider TraceContextProvider) (vm.StateDB, tmtypes.Txs, tracers.StateReleaseFunc, error) {
	emptyRelease := func() {}
	// Short circuit if it's genesis block.
	if block.Number().Int64() == 0 {
		return nil, nil, emptyRelease, errors.New("no transaction in genesis")
	}
	sdkCtx, tmBlock, release, err := b.initializeBlock(ctx, block, ctxProvider)
	if err != nil {
		return nil, nil, emptyRelease, err
	}
	success := false
	defer func() {
		if !success {
			release()
		}
	}()
	if txIndex > len(tmBlock.Block.Txs)-1 {
		return nil, nil, emptyRelease, errors.New("did not find transaction")
	}
	if txIndex < 0 {
		success = true
		return state.NewDBImpl(sdkCtx.WithIsEVM(true), b.keeper, true), tmBlock.Block.Txs, release, nil
	}
	for idx, tx := range tmBlock.Block.Txs {
		if idx > txIndex {
			break
		}
		sdkTx, err := traceCompatTxDecoder(b.txConfigProvider(block.Number().Int64()), b.isV65ActiveAtHeight(block.Number().Int64()))(tx)
		if err != nil {
			return nil, nil, emptyRelease, fmt.Errorf("decode transaction %d at block %d: %w", idx, block.Number().Int64(), err)
		}
		if utils.IsTxPrioritized(sdkTx) {
			continue
		}
		_ = b.app.DeliverTx(sdkCtx, abci.RequestDeliverTxV2{Tx: tx}, sdkTx, sha256.Sum256(tx))
	}
	success = true
	return state.NewDBImpl(sdkCtx.WithIsEVM(true), b.keeper, true), tmBlock.Block.Txs, release, nil
}

func (b *Backend) StateAtBlock(ctx context.Context, block *ethtypes.Block, reexec uint64, base vm.StateDB, readOnly bool, preferDisk bool) (vm.StateDB, tracers.StateReleaseFunc, error) {
	emptyRelease := func() {}
	sdkCtx, _, release, err := b.initializeBlock(ctx, block, b.traceCtxProvider)
	if err != nil {
		return nil, emptyRelease, err
	}
	statedb := state.NewDBImpl(sdkCtx, b.keeper, true)
	return statedb, release, nil
}

func (b *Backend) initializeBlock(ctx context.Context, block *ethtypes.Block, ctxProvider TraceContextProvider) (sdk.Context, *coretypes.ResultBlock, tracers.StateReleaseFunc, error) {
	emptyRelease := func() {}
	// get the parent block using block.parentHash
	prevBlockHeight := block.Number().Int64() - 1

	blockNumber := block.Number().Int64()
	tmBlock, err := blockByNumberRespectingWatermarks(ctx, b.tmClient, b.watermarks, &blockNumber, 1)
	if err != nil {
		return sdk.Context{}, nil, emptyRelease, fmt.Errorf("cannot find block %d from tendermint", blockNumber)
	}
	validators, err := b.loadAllValidators(ctx, prevBlockHeight)
	if err != nil {
		return sdk.Context{}, nil, emptyRelease, fmt.Errorf("failed to load validators for block %d from tendermint: %w", prevBlockHeight, err)
	}
	reqBeginBlock := tmBlock.Block.ToReqBeginBlock(validators)
	reqBeginBlock.Simulate = true
	baseCtx, baseRelease := ctxProvider(prevBlockHeight)
	sdkCtx := baseCtx.WithBlockHeight(blockNumber).WithBlockTime(tmBlock.Block.Time)
	legacyabci.BeginBlock(sdkCtx, blockNumber, reqBeginBlock.LastCommitInfo.Votes, tmBlock.Block.Evidence.ToABCI(), b.beginBlockKeepers)
	nextCtx, nextRelease := ctxProvider(sdkCtx.BlockHeight())
	sdkCtx = sdkCtx.WithNextMs(
		nextCtx.MultiStore(),
		[]string{"oracle", "oracle_mem"},
	)
	return sdkCtx, tmBlock, func() {
		nextRelease()
		baseRelease()
	}, nil
}

func (b *Backend) loadAllValidators(ctx context.Context, height int64) ([]*tmtypes.Validator, error) {
	if b.tmClient == nil {
		return nil, errors.New("consensus client is not configured")
	}
	const (
		validatorsPerPage = 100
		maxValidatorPages = 100
	)
	validators := make([]*tmtypes.Validator, 0)
	expectedTotal := -1
	for page := 1; page <= maxValidatorPages; page++ {
		perPage := validatorsPerPage
		res, err := b.tmClient.Validators(ctx, &height, &page, &perPage)
		if err != nil {
			return nil, err
		}
		if res == nil {
			return nil, errors.New("consensus client returned an empty validator response")
		}
		if err := TraceTendermintIfApplicable(ctx, "Validators", []string{
			stringifyInt64Ptr(&height), fmt.Sprintf("page=%d", page), fmt.Sprintf("per_page=%d", perPage),
		}, res); err != nil {
			return nil, err
		}
		if expectedTotal < 0 {
			expectedTotal = res.Total
			if expectedTotal <= 0 {
				return nil, fmt.Errorf("validator set at height %d is empty", height)
			}
		} else if res.Total != expectedTotal {
			return nil, fmt.Errorf("validator total changed while paging height %d: expected %d, got %d", height, expectedTotal, res.Total)
		}
		if len(res.Validators) == 0 {
			return nil, fmt.Errorf("validator page %d at height %d is empty before total %d was reached", page, height, expectedTotal)
		}
		for index, validator := range res.Validators {
			if validator == nil {
				return nil, fmt.Errorf("validator page %d at height %d contains nil validator %d", page, height, index)
			}
		}
		validators = append(validators, res.Validators...)
		if len(validators) > expectedTotal {
			return nil, fmt.Errorf("validator pages at height %d exceeded declared total %d", height, expectedTotal)
		}
		if len(validators) == expectedTotal {
			return validators, nil
		}
	}
	return nil, fmt.Errorf("validator set at height %d exceeds supported page limit of %d validators", height, validatorsPerPage*maxValidatorPages)
}

func (b *Backend) GetEVM(_ context.Context, msg *core.Message, stateDB vm.StateDB, h *ethtypes.Header, vmConfig *vm.Config, blockCtx *vm.BlockContext) *vm.EVM {
	txContext := core.NewEVMTxContext(msg)
	if blockCtx == nil {
		blockCtx, _ = b.keeper.GetVMBlockContext(b.ctxProvider(LatestCtxHeight).WithIsEVM(true).WithEVMEntryViaWasmdPrecompile(wasmd.IsWasmdCall(msg.To)), b.keeper.GetGasPool())
	}
	height := h.Number.Int64()
	chainCfg := b.chainConfigForHeight(height)
	evm := vm.NewEVM(*blockCtx, stateDB, chainCfg, *vmConfig, b.keeper.CustomPrecompiles(b.ctxProvider(height)))
	evm.SetTxContext(txContext)
	return evm
}

func (b *Backend) CurrentHeader() *ethtypes.Header {
	if b == nil || b.ctxProvider == nil {
		return nil
	}
	height := b.ctxProvider(LatestCtxHeight).BlockHeight()
	ctx, cancel := context.WithTimeout(context.Background(), currentHeaderLookupTimeout)
	defer cancel()
	if tmBlock, err := blockByNumberRespectingWatermarks(ctx, b.tmClient, b.watermarks, &height, 1); err == nil {
		if header, headerErr := b.getHeader(ctx, tmBlock); headerErr == nil {
			return header
		}
	}
	b.headerMu.RLock()
	defer b.headerMu.RUnlock()
	if b.lastHeader == nil {
		return nil
	}
	return ethtypes.CopyHeader(b.lastHeader)
}

func (b *Backend) SuggestGasTipCap(context.Context) (*big.Int, error) {
	return utils.Big0, nil
}

// getBlockByNumberOrHash resolves blockNrOrHash to a Tendermint ResultBlock in one RPC path
// (by hash or by number, including latest). Callers pass the result to getHeader.
func (b *Backend) getBlockByNumberOrHash(ctx context.Context, blockNrOrHash rpc.BlockNumberOrHash) (*coretypes.ResultBlock, bool, error) {
	var (
		block         *coretypes.ResultBlock
		err           error
		isLatestBlock bool
	)

	if blockNrOrHash.BlockHash != nil {
		block, err = blockByHashRespectingWatermarks(ctx, b.tmClient, b.watermarks, blockNrOrHash.BlockHash[:], 1)
		if err != nil {
			return nil, false, err
		}
		return block, false, nil
	}

	var blockNumberPtr *int64
	if blockNrOrHash.BlockNumber != nil {
		blockNumberPtr, err = getBlockNumber(ctx, b.tmClient, *blockNrOrHash.BlockNumber)
		if err != nil {
			return nil, false, err
		}
		if blockNumberPtr == nil {
			isLatestBlock = true
		}
	} else {
		isLatestBlock = true
	}
	block, err = blockByNumberRespectingWatermarks(ctx, b.tmClient, b.watermarks, blockNumberPtr, 1)
	if err != nil {
		return nil, false, err
	}
	return block, isLatestBlock, nil
}

func (b *Backend) getHeader(ctx context.Context, tmBlock *coretypes.ResultBlock) (*ethtypes.Header, error) {
	if tmBlock == nil || tmBlock.Block == nil {
		return nil, errors.New("cannot construct EVM header from an empty consensus block")
	}
	height := tmBlock.Block.Height
	if height < 0 {
		return nil, fmt.Errorf("block height %d is negative", height)
	}
	if tmBlock.Block.Time.Unix() < 0 {
		return nil, fmt.Errorf("block %d timestamp predates Unix epoch", height)
	}
	zeroExcessBlobGas := uint64(0)
	baseFee := b.keeper.GetNextBaseFeePerGas(b.ctxProvider(height - 1)).TruncateInt().BigInt()
	sdkCtx := b.ctxProvider(height)
	if sdkCtx.ChainID() == "pacific-1" && sdkCtx.BlockHeight() < b.keeper.UpgradeKeeper().GetDoneHeight(sdkCtx.WithGasMeter(sdk.NewInfiniteGasMeter(1, 1)), "6.2.0") {
		baseFee = nil
	}
	cp := sdkCtx.ConsensusParams()
	if cp == nil || cp.Block == nil {
		return nil, fmt.Errorf("consensus block parameters are unavailable at height %d", height)
	}
	gasLimit, err := encodeHeadGasLimit(cp.Block.MaxGas)
	if err != nil {
		return nil, fmt.Errorf("height %d: %w", height, err)
	}

	header := &ethtypes.Header{
		Difficulty:    common.Big0,
		Number:        big.NewInt(height),
		BaseFee:       baseFee,
		GasLimit:      uint64(gasLimit),
		Time:          uint64(tmBlock.Block.Time.Unix()),
		ExcessBlobGas: &zeroExcessBlobGas,
		ParentHash:    common.BytesToHash(tmBlock.Block.LastBlockID.Hash),
		Root:          common.BytesToHash(tmBlock.Block.AppHash),
		TxHash:        common.BytesToHash(tmBlock.Block.DataHash),
		ReceiptHash:   common.BytesToHash(tmBlock.Block.LastResultsHash),
		Coinbase:      common.BytesToAddress(tmBlock.Block.ProposerAddress),
	}
	b.headerMu.Lock()
	if b.lastHeader == nil || b.lastHeader.Number.Cmp(header.Number) <= 0 {
		b.lastHeader = ethtypes.CopyHeader(header)
	}
	b.headerMu.Unlock()
	return header, nil
}

func (b *Backend) GetCustomPrecompiles(h int64) map[common.Address]vm.PrecompiledContract {
	return b.keeper.CustomPrecompiles(b.ctxProvider(h))
}

func (b *Backend) PrepareTx(statedb vm.StateDB, tx *ethtypes.Transaction) error {
	typedStateDB := state.GetDBImpl(statedb)
	if typedStateDB == nil {
		return errors.New("unsupported EVM state database")
	}
	typedStateDB.CleanupForTracer()
	ctx, _ := b.keeper.PrepareCtxForEVMTransaction(typedStateDB.Ctx(), tx)
	ctx = ctx.WithIsEVM(true)
	if noSignatureSet(tx) {
		// skip ante if no signature is set
		return nil
	}
	txData, err := ethtx.NewTxDataFromTx(tx)
	if err != nil {
		return fmt.Errorf("transaction cannot be converted to TxData due to %s", err)
	}
	msg, err := types.NewMsgEVMTransaction(txData)
	if err != nil {
		return fmt.Errorf("transaction cannot be converted to MsgEVMTransaction due to %s", err)
	}
	tb := b.txConfigProvider(ctx.BlockHeight()).NewTxBuilder()
	if err := tb.SetMsgs(msg); err != nil {
		return fmt.Errorf("transaction message cannot be set: %w", err)
	}
	newCtx, err := b.antehandler(ctx, tb.GetTx(), false)
	if err != nil {
		return fmt.Errorf("transaction failed ante handler due to %s", err)
	}
	typedStateDB.WithCtx(newCtx)
	return nil
}

// PrepareTxNoFlush is like PrepareTx but uses ResetForTracer instead of
// CleanupForTracer, avoiding CacheMultiStore flushes. This is required in the
// parallel block trace path where copies of the statedb are concurrently read
// by worker goroutines; flushing would write to shared CacheMultiStore layers
// and cause data races.
func (b *Backend) PrepareTxNoFlush(statedb vm.StateDB, tx *ethtypes.Transaction) error {
	typedStateDB := state.GetDBImpl(statedb)
	if typedStateDB == nil {
		return errors.New("unsupported EVM state database")
	}
	typedStateDB.ResetForTracer()
	ctx, _ := b.keeper.PrepareCtxForEVMTransaction(typedStateDB.Ctx(), tx)
	ctx = ctx.WithIsEVM(true)
	if noSignatureSet(tx) {
		return nil
	}
	txData, err := ethtx.NewTxDataFromTx(tx)
	if err != nil {
		return fmt.Errorf("transaction cannot be converted to TxData due to %s", err)
	}
	msg, err := types.NewMsgEVMTransaction(txData)
	if err != nil {
		return fmt.Errorf("transaction cannot be converted to MsgEVMTransaction due to %s", err)
	}
	tb := b.txConfigProvider(ctx.BlockHeight()).NewTxBuilder()
	if err := tb.SetMsgs(msg); err != nil {
		return fmt.Errorf("transaction message cannot be set: %w", err)
	}
	newCtx, err := b.antehandler(ctx, tb.GetTx(), false)
	if err != nil {
		return fmt.Errorf("transaction failed ante handler due to %s", err)
	}
	typedStateDB.WithCtx(newCtx)
	return nil
}

func (b *Backend) GetBlockContext(ctx context.Context, block *ethtypes.Block, statedb vm.StateDB, backend export.ChainContextBackend) (vm.BlockContext, error) {
	typedStateDB := state.GetDBImpl(statedb)
	if typedStateDB == nil {
		return vm.BlockContext{}, errors.New("unsupported EVM state database")
	}
	blockCtx, err := b.keeper.GetVMBlockContext(typedStateDB.Ctx(), b.keeper.GetGasPool())
	if err != nil {
		return vm.BlockContext{}, fmt.Errorf("failed to construct EVM block context: %w", err)
	}
	return *blockCtx, nil
}

func noSignatureSet(tx *ethtypes.Transaction) bool {
	isBigIntEmpty := func(b *big.Int) bool {
		return b == nil || b.Cmp(utils.Big0) == 0 || b.Cmp(&big.Int{}) == 0
	}
	v, r, s := tx.RawSignatureValues()
	return isBigIntEmpty(v) && isBigIntEmpty(r) && isBigIntEmpty(s)
}

type Engine struct {
	*ethash.Ethash
	ctxProvider func(int64) sdk.Context
	keeper      *keeper.Keeper
}

func (e *Engine) Author(*ethtypes.Header) (common.Address, error) {
	return e.keeper.GetFeeCollectorAddress(e.ctxProvider(LatestCtxHeight))
}

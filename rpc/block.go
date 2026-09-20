package evmrpc

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"math"
	"math/big"
	"sync"
	"time"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/common/bitutil"
	"github.com/ethereum/go-ethereum/common/hexutil"
	ethtypes "github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/export"
	"github.com/ethereum/go-ethereum/rpc"
	"github.com/sidiora-labs/paxeer-network/consensus/rpc/coretypes"
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/sdk/client"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	banktypes "github.com/sidiora-labs/paxeer-network/sdk/x/bank/types"
	wasmtypes "github.com/sidiora-labs/paxeer-network/wasm/x/wasm/types"
)

const (
	EthNamespace  = "eth"
	PaxNamespace  = "pax"
	Pax2Namespace = "pax2"
)

type BlockAPI struct {
	tmClient             client.LocalClient
	keeper               *keeper.Keeper
	ctxProvider          func(int64) sdk.Context
	txConfigProvider     func(int64) client.TxConfig
	connectionType       ConnectionType
	namespace            string
	includeShellReceipts bool
	includeBankTransfers bool
	watermarks           *WatermarkManager
	globalBlockCache     BlockCache
	cacheCreationMutex   *sync.Mutex
}

type PaxBlockAPI struct {
	*BlockAPI
}

func NewBlockAPI(tmClient client.LocalClient, k *keeper.Keeper, ctxProvider func(int64) sdk.Context, txConfigProvider func(int64) client.TxConfig, connectionType ConnectionType, watermarks *WatermarkManager, globalBlockCache BlockCache, cacheCreationMutex *sync.Mutex) *BlockAPI {
	return &BlockAPI{
		tmClient:             tmClient,
		keeper:               k,
		ctxProvider:          ctxProvider,
		txConfigProvider:     txConfigProvider,
		connectionType:       connectionType,
		includeShellReceipts: false,
		includeBankTransfers: false,
		namespace:            EthNamespace,
		watermarks:           watermarks,
		globalBlockCache:     globalBlockCache,
		cacheCreationMutex:   cacheCreationMutex,
	}
}

func NewPaxBlockAPI(
	tmClient client.LocalClient,
	k *keeper.Keeper,
	ctxProvider func(int64) sdk.Context,
	txConfigProvider func(int64) client.TxConfig,
	connectionType ConnectionType,
	watermarks *WatermarkManager,
	globalBlockCache BlockCache,
	cacheCreationMutex *sync.Mutex,
) *PaxBlockAPI {
	blockAPI := &BlockAPI{
		tmClient:             tmClient,
		keeper:               k,
		ctxProvider:          ctxProvider,
		txConfigProvider:     txConfigProvider,
		connectionType:       connectionType,
		includeShellReceipts: true,
		includeBankTransfers: false,
		namespace:            PaxNamespace,
		watermarks:           watermarks,
		globalBlockCache:     globalBlockCache,
		cacheCreationMutex:   cacheCreationMutex,
	}
	return &PaxBlockAPI{
		BlockAPI: blockAPI,
	}
}

func NewPax2BlockAPI(
	tmClient client.LocalClient,
	k *keeper.Keeper,
	ctxProvider func(int64) sdk.Context,
	txConfigProvider func(int64) client.TxConfig,
	connectionType ConnectionType,
	watermarks *WatermarkManager,
	globalBlockCache BlockCache,
	cacheCreationMutex *sync.Mutex,
) *PaxBlockAPI {
	blockAPI := NewPaxBlockAPI(tmClient, k, ctxProvider, txConfigProvider, connectionType, watermarks, globalBlockCache, cacheCreationMutex)
	blockAPI.namespace = Pax2Namespace
	blockAPI.includeBankTransfers = true
	return blockAPI
}

func (a *PaxBlockAPI) GetBlockByNumberExcludeTraceFail(ctx context.Context, number rpc.BlockNumber, fullTx bool) (result map[string]interface{}, returnErr error) {
	// Exclude synthetic txs (filterTransactions drops them) and ante-failure
	// stub receipts (EncodeTmBlock drops them via excludeUntraceable).
	return a.getBlockByNumber(ctx, number, fullTx, false, true)
}

func (a *PaxBlockAPI) GetBlockByHashExcludeTraceFail(ctx context.Context, blockHash common.Hash, fullTx bool) (result map[string]interface{}, returnErr error) {
	// See note on GetBlockByNumberExcludeTraceFail.
	return a.getBlockByHash(ctx, blockHash, fullTx, false, true)
}

func (a *BlockAPI) GetBlockTransactionCountByNumber(ctx context.Context, number rpc.BlockNumber) (result *hexutil.Uint, returnErr error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, fmt.Sprintf("%s_getBlockTransactionCountByNumber", a.namespace), a.connectionType, startTime, returnErr, recover())
	}()
	numberPtr, err := getBlockNumber(ctx, a.tmClient, number)
	if err != nil {
		return nil, err
	}
	// Ethereum JSON-RPC: non-existent / future numeric block => null, not an error.
	block, err := blockByNumberOrNullForJSONRPC(ctx, a.tmClient, a.watermarks, numberPtr, 1)
	if err != nil {
		return nil, err
	}
	if block == nil {
		return nil, nil
	}
	if err = a.watermarks.EnsureReceiptHeightAvailable(ctx, block.Block.Height); err != nil {
		return nil, err
	}
	if err = validateBlockExecutionReceipts(a.keeper, a.ctxProvider, a.txConfigProvider, block, a.includeShellReceipts, a.cacheCreationMutex, a.globalBlockCache); err != nil {
		return nil, err
	}
	return a.getEvmTxCount(block), nil
}

func (a *BlockAPI) GetBlockTransactionCountByHash(ctx context.Context, blockHash common.Hash) (result *hexutil.Uint, returnErr error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, fmt.Sprintf("%s_getBlockTransactionCountByHash", a.namespace), a.connectionType, startTime, returnErr, recover())
	}()
	// Ethereum JSON-RPC: non-existent block hash => null, not an error.
	block, err := blockByHashOrNullForJSONRPC(ctx, a.tmClient, a.watermarks, blockHash[:], 1)
	if err != nil {
		return nil, err
	}
	if block == nil {
		return nil, nil
	}
	if err = a.watermarks.EnsureReceiptHeightAvailable(ctx, block.Block.Height); err != nil {
		return nil, err
	}
	if err = validateBlockExecutionReceipts(a.keeper, a.ctxProvider, a.txConfigProvider, block, a.includeShellReceipts, a.cacheCreationMutex, a.globalBlockCache); err != nil {
		return nil, err
	}
	return a.getEvmTxCount(block), nil
}

func (a *BlockAPI) GetBlockByHash(ctx context.Context, blockHash common.Hash, fullTx bool) (result map[string]interface{}, returnErr error) {
	// used for both: eth_ and pax_ namespaces
	return a.getBlockByHash(ctx, blockHash, fullTx, a.includeShellReceipts, false)
}

func (a *BlockAPI) getBlockByHash(ctx context.Context, blockHash common.Hash, fullTx bool, includeSyntheticTxs bool, excludeUntraceable bool) (result map[string]interface{}, returnErr error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, fmt.Sprintf("%s_getBlockByHash", a.namespace), a.connectionType, startTime, returnErr, recover())
	}()

	// Ethereum spec: empty or non-existent block hash returns result=null, not error.
	if blockHash == (common.Hash{}) {
		return nil, nil
	}
	// Ethereum JSON-RPC: non-existent block hash (unknown OR above safe latest)
	// => null, not an error. The helper handles both cases.
	block, err := blockByHashOrNullForJSONRPC(ctx, a.tmClient, a.watermarks, blockHash[:], 1)
	if err != nil {
		return nil, err
	}
	if block == nil {
		return nil, nil
	}

	// Validate EVM block height for pacific-1 chain
	sdkCtx := a.ctxProvider(LatestCtxHeight)
	if err := ValidateEVMBlockHeight(sdkCtx.ChainID(), block.Block.Height); err != nil {
		return nil, err
	}

	return EncodeTmBlock(a.ctxProvider, a.txConfigProvider, block, a.keeper, fullTx, a.includeBankTransfers, includeSyntheticTxs, excludeUntraceable, a.globalBlockCache, a.cacheCreationMutex)
}

func (a *BlockAPI) GetBlockByNumber(ctx context.Context, number rpc.BlockNumber, fullTx bool) (result map[string]interface{}, returnErr error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, fmt.Sprintf("%s_getBlockByNumber", a.namespace), a.connectionType, startTime, returnErr, recover())
	}()
	return a.getBlockByNumber(ctx, number, fullTx, a.includeShellReceipts, false)
}

func (a *BlockAPI) getBlockByNumber(
	ctx context.Context,
	number rpc.BlockNumber,
	fullTx bool,
	includeSyntheticTxs bool,
	excludeUntraceable bool,
) (result map[string]interface{}, returnErr error) {
	numberPtr, err := getBlockNumber(ctx, a.tmClient, number)
	if err != nil {
		return nil, err
	}

	// Validate EVM block height for pacific-1 chain
	if numberPtr != nil {
		sdkCtx := a.ctxProvider(LatestCtxHeight)
		if err := ValidateEVMBlockHeight(sdkCtx.ChainID(), *numberPtr); err != nil {
			return nil, err
		}
	}

	// Ethereum JSON-RPC: non-existent / future numeric block => null, not an error.
	block, err := blockByNumberOrNullForJSONRPC(ctx, a.tmClient, a.watermarks, numberPtr, 1)
	if err != nil {
		return nil, err
	}
	if block == nil {
		return nil, nil
	}
	return EncodeTmBlock(a.ctxProvider, a.txConfigProvider, block, a.keeper, fullTx, a.includeBankTransfers, includeSyntheticTxs, excludeUntraceable, a.globalBlockCache, a.cacheCreationMutex)
}

func (a *BlockAPI) GetBlockReceipts(ctx context.Context, blockNrOrHash rpc.BlockNumberOrHash) (result []map[string]interface{}, returnErr error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, fmt.Sprintf("%s_getBlockReceipts", a.namespace), a.connectionType, startTime, returnErr, recover())
	}()
	// Ethereum spec: empty or non-existent block hash returns result=null, not error.
	if blockNrOrHash.BlockHash != nil && *blockNrOrHash.BlockHash == (common.Hash{}) {
		return nil, nil
	}
	// Ethereum JSON-RPC: non-existent / above-watermark block => null, not an error.
	// Dispatch on hash vs number directly so a nil heightPtr from getBlockNumber
	// (the "latest"/"safe"/"finalized"/"pending" tags) resolves to the safe-latest
	// height via blockByNumberOrNullForJSONRPC rather than being misread as
	// "block doesn't exist".
	var (
		block *coretypes.ResultBlock
		err   error
	)
	if blockNrOrHash.BlockHash != nil {
		block, err = blockByHashOrNullForJSONRPC(ctx, a.tmClient, a.watermarks, blockNrOrHash.BlockHash[:], 1)
	} else {
		var numberPtr *int64
		if blockNrOrHash.BlockNumber != nil {
			numberPtr, err = getBlockNumber(ctx, a.tmClient, *blockNrOrHash.BlockNumber)
		}
		if err == nil {
			block, err = blockByNumberOrNullForJSONRPC(ctx, a.tmClient, a.watermarks, numberPtr, 1)
		}
	}
	if err != nil {
		return nil, err
	}
	if block == nil {
		return nil, nil
	}

	height := block.Block.Height
	includeSynthetic := shouldIncludeSynthetic(a.namespace)
	if err := validateBlockExecutionReceipts(a.keeper, a.ctxProvider, a.txConfigProvider, block, includeSynthetic, a.cacheCreationMutex, a.globalBlockCache); err != nil {
		return nil, err
	}

	txHashes := getTxHashesFromBlock(a.ctxProvider, a.txConfigProvider, a.keeper, block, includeSynthetic, a.cacheCreationMutex, a.globalBlockCache)

	// Get tx receipts for all hashes in parallel
	wg := sync.WaitGroup{}
	mtx := sync.Mutex{}
	allReceipts := make([]map[string]interface{}, len(txHashes))
	for i, hash := range txHashes {
		wg.Add(1)
		go func(i int, hash typedTxHash) {
			defer wg.Done()
			defer func() {
				if recovered := recover(); recovered != nil {
					mtx.Lock()
					returnErr = fmt.Errorf("encode receipt %d: %v", i, recovered)
					mtx.Unlock()
				}
			}()
			receipt, err := getOrSetCachedReceiptErr(a.cacheCreationMutex, a.globalBlockCache, a.ctxProvider(height), a.keeper, block, hash.hash)
			if err != nil {
				mtx.Lock()
				returnErr = fmt.Errorf("reload receipt %d: %w", i, err)
				mtx.Unlock()
				return
			}
			if receipt == nil {
				mtx.Lock()
				returnErr = fmt.Errorf("receipt %d lookup returned nil", i)
				mtx.Unlock()
				return
			}
			encodedReceipt, err := encodeReceipt(a.ctxProvider, a.txConfigProvider, receipt, a.keeper, block, a.includeShellReceipts, a.globalBlockCache, a.cacheCreationMutex)
			if err != nil {
				mtx.Lock()
				returnErr = err
				mtx.Unlock()
			}
			allReceipts[i] = encodedReceipt
		}(i, hash)
	}
	wg.Wait()
	if returnErr != nil {
		return nil, returnErr
	}
	for i, receipt := range allReceipts {
		if len(receipt) == 0 {
			return nil, fmt.Errorf("receipt %d was not encoded", i)
		}
		receipt["transactionIndex"] = hexutil.Uint64(i) //nolint:gosec
	}
	return allReceipts, nil
}

// EncodeTmBlock renders a tendermint block as an eth_getBlockBy* response.
//
// excludeUntraceable, when true, drops EVM txs whose receipt is an
// ante-deferred stub (EffectiveGasPrice==0 && GasUsed==0). modules/evm/keeper/abci.go
// writes such stubs for txs that passed the nonce check but failed a later
// ante step (insufficient funds, insufficient fee, etc.); they never reached
// the VM and have no meaningful trace. Used by the *ExcludeTraceFail block
// endpoints to satisfy rpc/README.md's "included in blocks but not
// executed" filter; the regular eth_getBlockBy* endpoints pass false so
// these txs still surface in normal block responses (per PR #2343's
// TestAnteFailureOthers — users want to see them).
func EncodeTmBlock(
	ctxProvider func(int64) sdk.Context,
	txConfigProvider func(int64) client.TxConfig,
	block *coretypes.ResultBlock,
	k *keeper.Keeper,
	fullTx bool,
	includeBankTransfers bool,
	includeSyntheticTxs bool,
	excludeUntraceable bool,
	globalBlockCache BlockCache,
	cacheCreationMutex *sync.Mutex,
) (map[string]interface{}, error) {
	if block == nil || block.Block == nil {
		return nil, errors.New("cannot encode an empty consensus block")
	}
	if block.Block.Height < 0 {
		return nil, fmt.Errorf("block height %d is negative", block.Block.Height)
	}
	blockTimeUnix := block.Block.Time.Unix()
	if blockTimeUnix < 0 {
		return nil, fmt.Errorf("block %d has a pre-epoch timestamp", block.Block.Height)
	}
	if err := validateBlockExecutionReceipts(k, ctxProvider, txConfigProvider, block, includeSyntheticTxs, cacheCreationMutex, globalBlockCache); err != nil {
		return nil, err
	}
	number := big.NewInt(block.Block.Height)
	blockhash := common.HexToHash(block.BlockID.Hash.String())
	lastHash := common.HexToHash(block.Block.LastBlockID.Hash.String())
	appHash := common.HexToHash(block.Block.AppHash.String())
	txHash := common.HexToHash(block.Block.DataHash.String())
	resultHash := common.HexToHash(block.Block.LastResultsHash.String())
	miner := common.HexToAddress(block.Block.ProposerAddress.String())
	ctx := ctxProvider(block.Block.Height)
	var baseFeePerGas *big.Int
	if block.Block.Height > 1 {
		baseFeePerGas = k.GetNextBaseFeePerGas(ctxProvider(block.Block.Height - 1)).TruncateInt().BigInt()
	} else {
		baseFeePerGas = types.DefaultMinFeePerGas.TruncateInt().BigInt()
	}
	var blockGasUsed uint64
	chainConfig := types.DefaultChainConfig().EthereumConfig(k.ChainID(ctx))
	transactions := []interface{}{}
	latestCtx := ctxProvider(LatestCtxHeight)

	msgs := filterTransactions(k, ctxProvider, txConfigProvider, block, includeSyntheticTxs, includeBankTransfers, cacheCreationMutex, globalBlockCache)

	blockBloom := make([]byte, ethtypes.BloomByteLength)
	for _, msg := range msgs {
		switch m := msg.msg.(type) {
		case *types.MsgEVMTransaction:
			ethtx, _ := m.AsTransaction()
			if ethtx == nil {
				return nil, fmt.Errorf("block %d contains malformed EVM transaction at index %d", block.Block.Height, msg.index)
			}
			hash := ethtx.Hash()
			receipt, err := getOrSetCachedReceiptErr(cacheCreationMutex, globalBlockCache, latestCtx, k, block, hash)
			if err != nil {
				return nil, fmt.Errorf("load block %d transaction %d receipt: %w", block.Block.Height, msg.index, err)
			}
			if receipt == nil {
				return nil, fmt.Errorf("block %d transaction %d receipt lookup returned nil", block.Block.Height, msg.index)
			}
			// Untraceable receipt — tx never reached the VM (ante-deferred
			// stub) or is chain-generated synthetic. filterTransactions's
			// isReceiptFromAnteError only catches the nonce-error subset
			// post-v5.8.0 (per PR #2343, which keeps insufficient-funds
			// receipts visible to the regular eth_getBlockBy* endpoints);
			// *ExcludeTraceFail needs the broader discriminator. See
			// isReceiptUntraceable for the shared definition used at every
			// *ExcludeTraceFail site.
			if excludeUntraceable && isReceiptUntraceable(receipt) {
				continue
			}
			if !fullTx {
				transactions = append(transactions, hash.Hex())
			} else {
				blockUnix := uint64(blockTimeUnix)
				newTx := export.NewRPCTransaction(ethtx, blockhash, number.Uint64(), blockUnix, uint64(len(transactions)), baseFeePerGas, chainConfig)
				replaceFrom(newTx, receipt)
				transactions = append(transactions, newTx)
			}
			var bloom ethtypes.Bloom
			bloom.SetBytes(receipt.LogsBloom)
			bitutil.ORBytes(blockBloom, blockBloom, bloom[:])
			// derive gas used from receipt as TxResult.GasUsed may not be accurate
			// for ante-failing EVM txs.
			if err := addBlockGasUsed(&blockGasUsed, receipt.GasUsed); err != nil {
				return nil, fmt.Errorf("block %d transaction %d: %w", block.Block.Height, msg.index, err)
			}
		case *wasmtypes.MsgExecuteContract:
			th := sha256.Sum256(block.Block.Txs[msg.index])
			receipt, err := getOrSetCachedReceiptErr(cacheCreationMutex, globalBlockCache, latestCtx, k, block, th)
			if err != nil {
				return nil, fmt.Errorf("load block %d transaction %d receipt: %w", block.Block.Height, msg.index, err)
			}
			if receipt == nil {
				return nil, fmt.Errorf("block %d transaction %d receipt lookup returned nil", block.Block.Height, msg.index)
			}
			if !fullTx {
				transactions = append(transactions, "0x"+hex.EncodeToString(th[:]))
			} else {
				ti := uint64(len(transactions))
				var to common.Address
				ercAddress, _, exists := k.GetAnyPointeeInfo(ctx, m.Contract)
				if exists {
					to = ercAddress
				} else {
					contractAddress, err := sdk.AccAddressFromBech32(m.Contract)
					if err != nil {
						return nil, fmt.Errorf("block %d contains invalid contract address at transaction %d: %w", block.Block.Height, msg.index, err)
					}
					to = k.GetEVMAddressOrDefault(ctx, contractAddress)
				}
				transactions = append(transactions, &export.RPCTransaction{
					BlockHash:        &blockhash,
					BlockNumber:      (*hexutil.Big)(number),
					From:             common.HexToAddress(receipt.From),
					To:               &to,
					Input:            m.Msg.Bytes(),
					Hash:             th,
					TransactionIndex: (*hexutil.Uint64)(&ti),
				})
			}
			var bloom ethtypes.Bloom
			bloom.SetBytes(receipt.LogsBloom)
			bitutil.ORBytes(blockBloom, blockBloom, bloom[:])
			if err := addBlockGasUsed(&blockGasUsed, receipt.GasUsed); err != nil {
				return nil, fmt.Errorf("block %d transaction %d: %w", block.Block.Height, msg.index, err)
			}
		case *banktypes.MsgSend:
			th := sha256.Sum256(block.Block.Txs[msg.index])
			receipt, _ := getOrSetCachedReceipt(cacheCreationMutex, globalBlockCache, latestCtx, k, block, th)
			if !fullTx {
				transactions = append(transactions, "0x"+hex.EncodeToString(th[:]))
			} else {
				rpcTx := &export.RPCTransaction{
					BlockHash:   &blockhash,
					BlockNumber: (*hexutil.Big)(number),
					Hash:        th,
				}
				senderPaxAddr, err := sdk.AccAddressFromBech32(m.FromAddress)
				if err != nil {
					return nil, fmt.Errorf("block %d contains invalid sender address at transaction %d: %w", block.Block.Height, msg.index, err)
				}
				rpcTx.From = k.GetEVMAddressOrDefault(ctx, senderPaxAddr)
				recipientPaxAddr, err := sdk.AccAddressFromBech32(m.ToAddress)
				if err != nil {
					return nil, fmt.Errorf("block %d contains invalid recipient address at transaction %d: %w", block.Block.Height, msg.index, err)
				}
				recipientEvmAddr := k.GetEVMAddressOrDefault(ctx, recipientPaxAddr)
				rpcTx.To = &recipientEvmAddr
				amt := m.Amount.AmountOf("uhpx").Mul(state.SdkUhpxToSweiMultiplier)
				rpcTx.Value = (*hexutil.Big)(amt.BigInt())
				ti := uint64(len(transactions))
				rpcTx.TransactionIndex = (*hexutil.Uint64)(&ti)
				transactions = append(transactions, rpcTx)
			}
			if receipt != nil {
				if err := addBlockGasUsed(&blockGasUsed, receipt.GasUsed); err != nil {
					return nil, fmt.Errorf("block %d transaction %d: %w", block.Block.Height, msg.index, err)
				}
			}
		}
	}
	if len(transactions) == 0 {
		txHash = ethtypes.EmptyTxsHash
	}

	// Source block.gasLimit from the active ConsensusParams in the SDK
	// context — same place the EVM runtime reads block.gaslimit from
	// (modules/evm/keeper/keeper.go's BlockContext.GasLimit), so
	// eth_getBlockByNumber.gasLimit and the GASLIMIT opcode return the
	// same number.
	cp := ctx.ConsensusParams()
	if cp == nil || cp.Block == nil {
		return nil, fmt.Errorf("block %d consensus block parameters are unavailable", block.Block.Height)
	}
	gasLimit, err := encodeHeadGasLimit(cp.Block.MaxGas)
	if err != nil {
		return nil, fmt.Errorf("block %d: %w", block.Block.Height, err)
	}
	result := map[string]interface{}{
		"number":           (*hexutil.Big)(number),
		"hash":             blockhash,
		"parentHash":       lastHash,
		"nonce":            ethtypes.BlockNonce{},   // inapplicable to Pax
		"mixHash":          common.Hash{},           // inapplicable to Pax
		"sha3Uncles":       ethtypes.EmptyUncleHash, // inapplicable to Pax
		"logsBloom":        ethtypes.BytesToBloom(blockBloom),
		"stateRoot":        appHash,
		"miner":            miner,
		"difficulty":       (*hexutil.Big)(big.NewInt(0)), // inapplicable to Pax
		"extraData":        hexutil.Bytes{},               // inapplicable to Pax
		"gasLimit":         gasLimit,
		"gasUsed":          hexutil.Uint64(blockGasUsed),
		"timestamp":        hexutil.Uint64(block.Block.Time.Unix()), //nolint:gosec
		"transactionsRoot": txHash,
		"receiptsRoot":     resultHash,
		"size":             hexutil.Uint64(block.Block.Size()), //nolint:gosec
		"uncles":           []common.Hash{},                    // inapplicable to Pax
		"transactions":     transactions,
		"baseFeePerGas":    (*hexutil.Big)(baseFeePerGas),
	}
	if fullTx {
		result["totalDifficulty"] = (*hexutil.Big)(big.NewInt(0)) // inapplicable to Pax
	}
	return result, nil
}

func addBlockGasUsed(total *uint64, gasUsed uint64) error {
	if total == nil {
		return fmt.Errorf("gas accumulator is not configured")
	}
	if math.MaxUint64-*total < gasUsed {
		return fmt.Errorf("gas usage overflows uint64")
	}
	*total += gasUsed
	return nil
}

func FullBloom() ethtypes.Bloom {
	bz := []byte{}
	for i := 0; i < ethtypes.BloomByteLength; i++ {
		bz = append(bz, 255)
	}
	return ethtypes.BytesToBloom(bz)
}

// getEvmTxCount returns the same transaction count as EncodeTmBlock exposes: filterTransactions
// plus the same per-msg rules as EncodeTmBlock (EVM messages need GetReceipt to succeed).
func (a *BlockAPI) getEvmTxCount(block *coretypes.ResultBlock) *hexutil.Uint {
	n := countBlockTxsLikeEncodeTmBlock(
		a.ctxProvider,
		a.txConfigProvider,
		block,
		a.keeper,
		a.includeShellReceipts,
		a.includeBankTransfers,
		a.cacheCreationMutex,
		a.globalBlockCache,
	)
	cntHex := hexutil.Uint(n) //nolint:gosec
	return &cntHex
}

func countBlockTxsLikeEncodeTmBlock(
	ctxProvider func(int64) sdk.Context,
	txConfigProvider func(int64) client.TxConfig,
	block *coretypes.ResultBlock,
	k *keeper.Keeper,
	includeShellReceipts bool,
	includeBankTransfers bool,
	cacheCreationMutex *sync.Mutex,
	globalBlockCache BlockCache,
) int {
	msgs := filterTransactions(k, ctxProvider, txConfigProvider, block, includeShellReceipts, includeBankTransfers, cacheCreationMutex, globalBlockCache)
	n := 0
	for _, msg := range msgs {
		switch msg.msg.(type) {
		case *types.MsgEVMTransaction:
			n++
		case *wasmtypes.MsgExecuteContract:
			n++
		case *banktypes.MsgSend:
			n++
		}
	}
	return n
}

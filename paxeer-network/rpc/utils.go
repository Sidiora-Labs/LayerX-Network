package evmrpc

import (
	"context"
	"crypto/ecdsa"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"math/big"
	"runtime/debug"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/ethereum/go-ethereum/rpc"
	"github.com/sidiora-labs/paxeer-network/consensus/libs/bytes"
	"github.com/sidiora-labs/paxeer-network/consensus/rpc/coretypes"
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/rpc/rpcutils"
	"github.com/sidiora-labs/paxeer-network/rpc/stats"
	"github.com/sidiora-labs/paxeer-network/sdk/client"
	"github.com/sidiora-labs/paxeer-network/sdk/client/config"
	"github.com/sidiora-labs/paxeer-network/sdk/codec/legacy"
	"github.com/sidiora-labs/paxeer-network/sdk/crypto/hd"
	"github.com/sidiora-labs/paxeer-network/sdk/crypto/keyring"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	banktypes "github.com/sidiora-labs/paxeer-network/sdk/x/bank/types"
	receiptstore "github.com/sidiora-labs/paxeer-network/storage/ledger_db/receipt"
	utilmetrics "github.com/sidiora-labs/paxeer-network/utils/metrics"
	wasmtypes "github.com/sidiora-labs/paxeer-network/wasm/x/wasm/types"
	"golang.org/x/mod/semver"
)

const LatestCtxHeight int64 = -1

// EVM launch block heights for different chains
const Pacific1EVMLaunchHeight int64 = 79123881

// ErrBlockNotFoundByHash is returned when no block exists for the given hash (e.g. empty or unknown hash).
// Ethereum-compatible RPCs should return result: null for this case instead of an error.
var ErrBlockNotFoundByHash = errors.New("block not found by hash")

// GetBlockNumberByNrOrHash returns the height of the block with the given number or hash.
func GetBlockNumberByNrOrHash(ctx context.Context, tmClient client.LocalClient, wm *WatermarkManager, blockNrOrHash rpc.BlockNumberOrHash) (*int64, error) {
	if blockNrOrHash.BlockHash != nil {
		block, err := blockByHashRespectingWatermarks(ctx, tmClient, wm, blockNrOrHash.BlockHash[:], 1)
		if err != nil {
			return nil, err
		}
		height := block.Block.Height
		return &height, nil
	}
	return getBlockNumber(ctx, tmClient, *blockNrOrHash.BlockNumber)
}

func getBlockNumber(ctx context.Context, tmClient client.LocalClient, number rpc.BlockNumber) (*int64, error) {
	var numberPtr *int64
	switch number {
	case rpc.SafeBlockNumber, rpc.FinalizedBlockNumber, rpc.LatestBlockNumber, rpc.PendingBlockNumber:
		numberPtr = nil // requesting Block with nil means the latest block
	case rpc.EarliestBlockNumber:
		if tmClient == nil {
			return nil, errors.New("consensus client is not configured")
		}
		genesisRes, err := tmClient.Genesis(ctx)
		if err != nil {
			return nil, err
		}
		if genesisRes == nil || genesisRes.Genesis == nil {
			return nil, errors.New("consensus client returned empty genesis information")
		}
		if err := TraceTendermintIfApplicable(ctx, "Genesis", []string{}, genesisRes); err != nil {
			return nil, err
		}
		numberPtr = &genesisRes.Genesis.InitialHeight
	default:
		numberI64 := number.Int64()
		numberPtr = &numberI64
	}
	return numberPtr, nil
}

func getHeightFromBigIntBlockNumber(latest int64, blockNumber *big.Int) (int64, error) {
	if blockNumber == nil {
		return 0, errors.New("block number is required")
	}
	if !blockNumber.IsInt64() {
		return 0, fmt.Errorf("block number %s exceeds int64", blockNumber.String())
	}
	number := blockNumber.Int64()
	switch number {
	case rpc.FinalizedBlockNumber.Int64(), rpc.LatestBlockNumber.Int64(), rpc.SafeBlockNumber.Int64(), rpc.PendingBlockNumber.Int64():
		return latest, nil
	default:
		return number, nil
	}
}

func getTestKeyring(homeDir string) (keyring.Keyring, error) {
	clientCtx := client.Context{}.WithViper("").WithHomeDir(homeDir)
	clientCtx, err := config.ReadFromClientConfig(clientCtx)
	if err != nil {
		return nil, err
	}
	return client.NewKeyringFromBackend(clientCtx, keyring.BackendTest)
}

func getAddressPrivKeyMap(kb keyring.Keyring) map[string]*ecdsa.PrivateKey {
	res := map[string]*ecdsa.PrivateKey{}
	keys, err := kb.List()
	if err != nil {
		return res
	}
	for _, key := range keys {
		localInfo, ok := key.(keyring.LocalInfo)
		if !ok {
			// will only show local key
			continue
		}
		if localInfo.GetAlgo() != hd.Secp256k1Type {
			fmt.Printf("Skipping address %s because it isn't signed with secp256k1\n", localInfo.Name)
			continue
		}
		priv, err := legacy.PrivKeyFromBytes([]byte(localInfo.PrivKeyArmor))
		if err != nil {
			continue
		}
		privHex := hex.EncodeToString(priv.Bytes())
		privKey, err := crypto.HexToECDSA(privHex)
		if err != nil {
			continue
		}
		address := crypto.PubkeyToAddress(privKey.PublicKey)
		res[address.Hex()] = privKey
	}
	return res
}

func blockByNumberWithRetry(ctx context.Context, client client.LocalClient, height *int64, maxRetries int) (*coretypes.ResultBlock, error) {
	if client == nil {
		return nil, errors.New("consensus client is not configured")
	}
	blockRes, err := client.Block(ctx, height)
	var retryCount = 0
	for err != nil && retryCount < maxRetries {
		// retry once, since application DB and block DB are not committed atomically so it's possible for
		// receipt to exist while block results aren't committed yet
		if err := waitForRPCRetry(ctx, time.Second); err != nil {
			return nil, err
		}
		blockRes, err = client.Block(ctx, height)
		retryCount++
	}
	if err != nil {
		return nil, err
	}
	if blockRes == nil || blockRes.Block == nil {
		return nil, fmt.Errorf("could not find block for height %s", stringifyInt64Ptr(height))
	}
	if err := TraceTendermintIfApplicable(ctx, "Block", []string{stringifyInt64Ptr(height)}, blockRes); err != nil {
		return nil, err
	}
	return blockRes, err
}

func blockByHash(ctx context.Context, client client.LocalClient, hash bytes.HexBytes) (*coretypes.ResultBlock, error) {
	return blockByHashWithRetry(ctx, client, hash, 0)
}

func blockByHashWithRetry(ctx context.Context, client client.LocalClient, hash bytes.HexBytes, maxRetries int) (*coretypes.ResultBlock, error) {
	if client == nil {
		return nil, errors.New("consensus client is not configured")
	}
	blockRes, err := client.BlockByHash(ctx, hash)
	var retryCount = 0
	for err != nil && retryCount < maxRetries {
		// retry once, since application DB and block DB are not committed atomically so it's possible for
		// receipt to exist while block results aren't committed yet
		if err := waitForRPCRetry(ctx, time.Second); err != nil {
			return nil, err
		}
		blockRes, err = client.BlockByHash(ctx, hash)
		retryCount++
	}
	if err != nil {
		return nil, err
	}
	if blockRes == nil || blockRes.Block == nil {
		return nil, ErrBlockNotFoundByHash
	}
	if err := TraceTendermintIfApplicable(ctx, "BlockByHash", []string{hash.String()}, blockRes); err != nil {
		return nil, err
	}
	return blockRes, err
}

func waitForRPCRetry(ctx context.Context, delay time.Duration) error {
	timer := time.NewTimer(delay)
	defer timer.Stop()
	select {
	case <-timer.C:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

// ValidateEVMBlockHeight checks if the requested block height is valid for EVM queries
func ValidateEVMBlockHeight(chainID string, blockHeight int64) error {
	// Only validate for pacific-1 chain
	if chainID != "pacific-1" {
		return nil
	}
	if blockHeight < Pacific1EVMLaunchHeight {
		return fmt.Errorf("EVM is only supported from block %d onwards", Pacific1EVMLaunchHeight)
	}
	return nil
}

type indexedMsg struct {
	msg   sdk.Msg
	index int
}

func validateBlockExecutionReceipts(
	k *keeper.Keeper,
	ctxProvider func(int64) sdk.Context,
	txConfigProvider func(int64) client.TxConfig,
	block *coretypes.ResultBlock,
	includeSynthetic bool,
	cacheCreationMutex *sync.Mutex,
	globalBlockCache BlockCache,
) error {
	if block == nil || block.Block == nil {
		return errors.New("cannot validate receipts for an empty consensus block")
	}
	blockHeight := block.Block.Height
	if blockHeight < 0 {
		return fmt.Errorf("cannot validate receipts for negative block height %d", blockHeight)
	}
	decoder := txConfigProvider(blockHeight).TxDecoder()
	latestCtx := ctxProvider(LatestCtxHeight)
	for txIndex, txBytes := range block.Block.Txs {
		tx, err := decoder(txBytes)
		if err != nil {
			return fmt.Errorf("decode consensus transaction %d in block %d: %w", txIndex, block.Block.Height, err)
		}
		for messageIndex, message := range tx.GetMsgs() {
			var hash common.Hash
			switch typed := message.(type) {
			case *types.MsgEVMTransaction:
				if typed.IsAssociateTx() {
					continue
				}
				transaction, _ := typed.AsTransaction()
				if transaction == nil {
					return fmt.Errorf("consensus transaction %d message %d in block %d contains malformed EVM data", txIndex, messageIndex, block.Block.Height)
				}
				if _, err := rpcutils.RecoverEVMSender(transaction, block.Block.Height, block.Block.Time.Unix()); err != nil {
					return fmt.Errorf("recover consensus transaction %d message %d sender in block %d: %w", txIndex, messageIndex, block.Block.Height, err)
				}
				hash = transaction.Hash()
			case *wasmtypes.MsgExecuteContract:
				if !includeSynthetic {
					continue
				}
				hash = common.Hash(sha256.Sum256(txBytes))
			default:
				continue
			}
			receipt, err := getOrSetCachedReceiptErr(cacheCreationMutex, globalBlockCache, latestCtx, k, block, hash)
			if err != nil {
				if errors.Is(err, receiptstore.ErrNotFound) {
					return fmt.Errorf("block %d transaction %d message %d has no canonical receipt: %w", block.Block.Height, txIndex, messageIndex, err)
				}
				return fmt.Errorf("load block %d transaction %d message %d receipt: %w", block.Block.Height, txIndex, messageIndex, err)
			}
			if receipt == nil {
				return fmt.Errorf("block %d transaction %d message %d receipt lookup returned nil", block.Block.Height, txIndex, messageIndex)
			}
			if receipt.BlockNumber != uint64(blockHeight) {
				return fmt.Errorf("block %d transaction %d message %d receipt belongs to block %d", block.Block.Height, txIndex, messageIndex, receipt.BlockNumber)
			}
		}
	}
	return nil
}

func filterTransactions(
	k *keeper.Keeper,
	ctxProvider func(int64) sdk.Context,
	txConfigProvider func(int64) client.TxConfig,
	block *coretypes.ResultBlock,
	includeSyntheticTxs bool,
	includeBankTransfers bool,
	cacheCreationMutex *sync.Mutex,
	globalBlockCache BlockCache,
) []indexedMsg {
	txs := []indexedMsg{}
	txCounts := make(map[string]uint64)
	startOfBlockNonce := make(map[string]uint64)
	txConfig := txConfigProvider(block.Block.Height)
	latestCtx := ctxProvider(LatestCtxHeight)
	ctx := ctxProvider(block.Block.Height)
	prevCtx := ctxProvider(block.Block.Height - 1)
	for i, tx := range block.Block.Txs {
		sdkTx, err := txConfig.TxDecoder()(tx)
		if err != nil {
			continue
		}
		for _, msg := range sdkTx.GetMsgs() {
			switch m := msg.(type) {
			case *types.MsgEVMTransaction:
				if m.IsAssociateTx() {
					continue
				}
				ethtx, _ := m.AsTransaction()
				if ethtx == nil {
					continue
				}
				hash := ethtx.Hash()
				sender, err := rpcutils.RecoverEVMSender(ethtx, block.Block.Height, block.Block.Time.Unix())
				if err != nil {
					continue
				}
				receipt, found := getOrSetCachedReceipt(cacheCreationMutex, globalBlockCache, latestCtx, k, block, hash)
				if !found || receipt == nil || receipt.BlockNumber != uint64(block.Block.Height) || isReceiptFromAnteError(ctx, receipt) { //nolint:gosec
					continue
				}
				txCount := txCounts[sender.Hex()]
				if receipt.Status == 0 && receipt.EffectiveGasPrice == 0 {
					// check if the transaction bumped nonce. If not, exclude it
					if _, ok := startOfBlockNonce[sender.Hex()]; !ok {
						startOfBlockNonce[sender.Hex()] = k.GetNonce(prevCtx, common.HexToAddress(sender.Hex()))
					}
					if txCount+startOfBlockNonce[sender.Hex()] != ethtx.Nonce() {
						continue
					}
				}
				if !includeSyntheticTxs && receipt.TxType == types.ShellEVMTxType {
					continue
				}
				txCounts[sender.Hex()] = txCount + 1
				txs = append(txs, indexedMsg{index: i, msg: msg})
			case *wasmtypes.MsgExecuteContract:
				if !includeSyntheticTxs {
					continue
				}
				th := sha256.Sum256(block.Block.Txs[i])
				_, found := getOrSetCachedReceipt(cacheCreationMutex, globalBlockCache, latestCtx, k, block, th)
				if !found {
					continue
				}
				txs = append(txs, indexedMsg{index: i, msg: msg})
			case *banktypes.MsgSend:
				if !includeBankTransfers {
					continue
				}
				txs = append(txs, indexedMsg{index: i, msg: msg})
			}
		}
	}
	return txs
}

func recordMetrics(ctx context.Context, apiMethod string, connectionType ConnectionType, startTime time.Time) {
	recordMetricsWithError(ctx, apiMethod, connectionType, startTime, nil, nil)
}

func recordMetricsWithError(ctx context.Context, apiMethod string, connectionType ConnectionType, startTime time.Time, err error, panicValue any) {
	success := panicValue == nil && err == nil

	// these are only metrics that are specifically typed errors for tracking.
	if err != nil {
		utilmetrics.IncrementErrorMetrics(apiMethod, err)
	}

	recordRPCLatency(ctx, apiMethod, string(connectionType), success, err, panicValue != nil, startTime)
	// TODO(PLT-326): remove legacy dual-emit once dashboards are migrated to evmrpc_* OTEL metrics. Use metrics.requestLatencySeconds histogram instead.
	utilmetrics.IncrementRpcRequestCounter(apiMethod, string(connectionType), success)
	utilmetrics.MeasureRpcRequestLatency(apiMethod, string(connectionType), startTime)
	stats.RecordAPIInvocation(apiMethod, string(connectionType), startTime, success)

	if panicValue != nil {
		panic(panicValue)
	}
}

func CheckVersion(ctx sdk.Context, k *keeper.Keeper) error {
	if !evmExists(ctx, k) {
		return fmt.Errorf("evm module does not exist on height %d", ctx.BlockHeight())
	}
	if !bankExists(ctx, k) {
		return fmt.Errorf("bank module does not exist on height %d", ctx.BlockHeight())
	}
	return nil
}

func bankExists(ctx sdk.Context, k *keeper.Keeper) bool {
	return ctx.KVStore(k.BankKeeper().GetStoreKey()).VersionExists(ctx.BlockHeight())
}

func evmExists(ctx sdk.Context, k *keeper.Keeper) bool {
	return ctx.KVStore(k.GetStoreKey()).VersionExists(ctx.BlockHeight())
}

func shouldIncludeSynthetic(namespace string) bool {
	if namespace != "eth" && namespace != "pax" {
		panic(fmt.Sprintf("unknown namespace %s", namespace))
	}
	return namespace == "pax"
}

type typedTxHash struct {
	hash  common.Hash
	isEvm bool
}

func getTxHashesFromBlock(
	ctxProvider func(int64) sdk.Context,
	txConfigProvider func(int64) client.TxConfig,
	k *keeper.Keeper,
	block *coretypes.ResultBlock,
	shouldIncludeSynthetic bool,
	cacheCreationMutex *sync.Mutex,
	globalBlockCache BlockCache,
) []typedTxHash {
	txHashes := []typedTxHash{}
	for _, tx := range filterTransactions(k, ctxProvider, txConfigProvider, block, shouldIncludeSynthetic, false, cacheCreationMutex, globalBlockCache) {
		switch tx.msg.(type) {
		case *types.MsgEVMTransaction:
			ethtx, _ := tx.msg.(*types.MsgEVMTransaction).AsTransaction()
			txHashes = append(txHashes, typedTxHash{hash: ethtx.Hash(), isEvm: true})
		case *wasmtypes.MsgExecuteContract:
			txHashes = append(txHashes, typedTxHash{hash: common.Hash(sha256.Sum256(block.Block.Txs[tx.index])), isEvm: false})
		}
	}
	return txHashes
}

func isReceiptFromAnteError(ctx sdk.Context, receipt *types.Receipt) bool {
	// hacky heuristic
	if semver.Compare(ctx.ClosestUpgradeName(), "v5.8.0") < 0 {
		return receipt.EffectiveGasPrice == 0
	}
	return receipt.EffectiveGasPrice == 0 && (strings.Contains(receipt.VmError, core.ErrNonceTooHigh.Error()) ||
		strings.Contains(receipt.VmError, core.ErrNonceTooLow.Error()))
}

// isReceiptUntraceable returns true if the receipt represents a tx whose
// trace would be empty or meaningless. Shared discriminator used by every
// *ExcludeTraceFail site (tx, block, trace) so they filter the same set.
//
//   - TxType == ShellEVMTxType: chain-generated synthetic, no real EVM
//     execution. node/receipt.go writes these for wasm txs to surface CW20
//     events on the EVM side; they have no trace.
//   - EffectiveGasPrice == 0 && GasUsed == 0: ante-deferred stub receipt
//     from modules/evm/keeper/abci.go — the tx bumped its nonce in ante but
//     never reached the VM. WriteReceipt for any executed tx sets both
//     fields > 0 (intrinsic gas at minimum, msg.GasPrice for the fee on
//     a chain with positive min fee), so reverts and OOG pass through.
//
// This is intentionally narrower than isReceiptFromAnteError's
// post-v5.8.0 branch: that helper is tuned to keep insufficient-funds
// receipts visible to the regular eth_getBlockBy* endpoints (per
// PR #2343). *ExcludeTraceFail wants the opposite per rpc/README.md.
func isReceiptUntraceable(receipt *types.Receipt) bool {
	return receipt.TxType == types.ShellEVMTxType ||
		(receipt.EffectiveGasPrice == 0 && receipt.GasUsed == 0)
}

type ParallelRunner struct {
	Done  sync.WaitGroup
	Queue chan func()
}

var panicHook atomic.Value

func SetPanicHook(h func(interface{})) {
	panicHook.Store(h)
}

func NewParallelRunner(cnt int, capacity int) *ParallelRunner {
	pr := &ParallelRunner{
		Done:  sync.WaitGroup{},
		Queue: make(chan func(), capacity),
	}
	pr.Done.Add(cnt)
	for i := 0; i < cnt; i++ {
		go func() {
			defer pr.Done.Done()
			defer recoverAndLog()
			for f := range pr.Queue {
				runWithRecovery(f)
			}
		}()
	}
	return pr
}

func runWithRecovery(f func()) {
	defer recoverAndLog()
	f()
}

func recoverAndLog() {
	if e := recover(); e != nil {
		fmt.Printf("Panic recovered: %s\n", e)
		debug.PrintStack()
		if v := panicHook.Load(); v != nil {
			if hook, ok := v.(func(interface{})); ok && hook != nil {
				hook(e)
			}
		}
	}
}

func must[V any](v V, err error) V {
	if err != nil {
		panic(err)
	}
	return v
}

package evmrpc

import (
	"context"
	"errors"
	"fmt"
	"math"
	"math/big"
	"slices"
	"time"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/common/hexutil"
	gmath "github.com/ethereum/go-ethereum/common/math"
	"github.com/ethereum/go-ethereum/rpc"
	"github.com/sidiora-labs/paxeer-network/consensus/rpc/coretypes"
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	evmtypes "github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/sdk/client"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

const defaultPriorityFeePerGas = 1000000000 // 1gwei
const defaultThresholdPercentage = 80       // 80%

type InfoAPI struct {
	tmClient         client.LocalClient
	keeper           *keeper.Keeper
	ctxProvider      func(int64) sdk.Context
	txConfigProvider func(int64) client.TxConfig
	homeDir          string
	connectionType   ConnectionType
	maxBlocks        int64
	txDecoder        sdk.TxDecoder
	watermarks       *WatermarkManager
	keyringEnabled   bool
}

func NewInfoAPI(tmClient client.LocalClient, k *keeper.Keeper, ctxProvider func(int64) sdk.Context, txConfigProvider func(int64) client.TxConfig, homeDir string, maxBlocks int64, connectionType ConnectionType, txDecoder sdk.TxDecoder, watermarks *WatermarkManager, enableKeyring ...bool) *InfoAPI {
	keyringEnabled := len(enableKeyring) > 0 && enableKeyring[0]
	return &InfoAPI{tmClient: tmClient, keeper: k, ctxProvider: ctxProvider, txConfigProvider: txConfigProvider, homeDir: homeDir, connectionType: connectionType, maxBlocks: maxBlocks, txDecoder: txDecoder, watermarks: watermarks, keyringEnabled: keyringEnabled}
}

type FeeHistoryResult struct {
	OldestBlock  *hexutil.Big     `json:"oldestBlock"`
	Reward       [][]*hexutil.Big `json:"reward,omitempty"`
	BaseFee      []*hexutil.Big   `json:"baseFeePerGas,omitempty"`
	GasUsedRatio []float64        `json:"gasUsedRatio"`
}

func (i *InfoAPI) BlockNumber(ctx context.Context) (result hexutil.Uint64, returnErr error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, "eth_BlockNumber", i.connectionType, startTime, returnErr, recover())
	}()
	height, err := i.latestHeight(ctx)
	if err != nil {
		return 0, err
	}
	if height < 0 {
		return 0, fmt.Errorf("latest height %d is negative", height)
	}
	return hexutil.Uint64(height), nil
}

//nolint:revive
func (i *InfoAPI) ChainId(ctx context.Context) *hexutil.Big {
	startTime := time.Now()
	defer recordMetrics(ctx, "eth_ChainId", i.connectionType, startTime)
	return (*hexutil.Big)(i.keeper.ChainID(i.ctxProvider(LatestCtxHeight)))
}

func (i *InfoAPI) Coinbase(ctx context.Context) (addr common.Address, err error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, "eth_Coinbase", i.connectionType, startTime, err, recover())
	}()
	return i.keeper.GetFeeCollectorAddress(i.ctxProvider(LatestCtxHeight))
}

func (i *InfoAPI) Accounts(ctx context.Context) (result []common.Address, returnErr error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, "eth_Accounts", i.connectionType, startTime, returnErr, recover())
	}()
	if !i.keyringEnabled {
		return nil, &ErrEVMNotSupported{Msg: "eth_accounts is disabled; use an external wallet"}
	}
	kb, err := getTestKeyring(i.homeDir)
	if err != nil {
		return []common.Address{}, err
	}
	for addr := range getAddressPrivKeyMap(kb) {
		result = append(result, common.HexToAddress(addr))
	}
	return result, nil
}

func (i *InfoAPI) GasPrice(ctx context.Context) (result *hexutil.Big, returnErr error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, "eth_GasPrice", i.connectionType, startTime, returnErr, recover())
	}()
	baseFee := i.keeper.GetNextBaseFeePerGas(i.ctxProvider(LatestCtxHeight)).TruncateInt().BigInt()
	totalGasUsed, err := i.getCongestionData(ctx, nil)
	if err != nil {
		return nil, err
	}
	feeHist, err := i.FeeHistory(ctx, 1, rpc.LatestBlockNumber, []float64{50})
	if err != nil {
		return nil, err
	}
	var medianRewardPrevBlock *big.Int
	if len(feeHist.Reward) == 0 || len(feeHist.Reward[0]) == 0 {
		medianRewardPrevBlock = big.NewInt(defaultPriorityFeePerGas)
	} else {
		medianRewardPrevBlock = feeHist.Reward[0][0].ToInt()
	}
	return i.GasPriceHelper(ctx, baseFee, totalGasUsed, medianRewardPrevBlock)
}

// Helper function useful for testing
func (i *InfoAPI) GasPriceHelper(ctx context.Context, baseFee *big.Int, totalGasUsedPrevBlock uint64, medianRewardPrevBlock *big.Int) (*hexutil.Big, error) {
	isChainCongested, err := i.isChainCongested(totalGasUsedPrevBlock)
	if err != nil {
		return nil, err
	}
	if !isChainCongested {
		// chain is not congested, increase base fee by 10% to get the gas price to get a tx included in a timely manner
		gasPrice := new(big.Int).Mul(baseFee, big.NewInt(110))
		gasPrice.Div(gasPrice, big.NewInt(100))
		return (*hexutil.Big)(gasPrice), nil
	}
	// chain is congested, return the 50%-tile reward as the priority fee per gas
	gasPrice := new(big.Int).Add(medianRewardPrevBlock, baseFee)
	return (*hexutil.Big)(gasPrice), nil

}

// lastBlock is inclusive
func (i *InfoAPI) FeeHistory(ctx context.Context, blockCount gmath.HexOrDecimal64, lastBlock rpc.BlockNumber, rewardPercentiles []float64) (result *FeeHistoryResult, returnErr error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, "eth_feeHistory", i.connectionType, startTime, returnErr, recover())
	}()
	result = &FeeHistoryResult{}

	// logic consistent with go-ethereum's validation (block < 1 means no block)
	if blockCount < 1 {
		return result, nil
	}
	if i.maxBlocks <= 0 {
		return nil, errors.New("fee history block limit must be positive")
	}

	// default go-ethereum max block history is 1024
	// https://github.com/ethereum/go-ethereum/blob/master/eth/gasprice/feehistory.go#L235
	maxBlocksD64 := gmath.HexOrDecimal64(i.maxBlocks) //nolint:gosec
	if blockCount > maxBlocksD64 {
		blockCount = maxBlocksD64
	}

	// if someone needs more than 100 reward percentiles, we can discuss, but it's not likely
	if len(rewardPercentiles) > 100 {
		return nil, errors.New("rewardPercentiles length must be less than or equal to 100")
	}

	// validate reward percentiles
	for i, p := range rewardPercentiles {
		if p < 0 || p > 100 || (i > 0 && p <= rewardPercentiles[i-1]) {
			return nil, errors.New("invalid reward percentiles: must be ascending and between 0 and 100")
		}
	}

	lastBlockNumber := lastBlock.Int64()
	if i.tmClient == nil {
		return nil, errors.New("consensus client is not configured")
	}
	genesis, err := i.tmClient.Genesis(ctx)
	if err != nil {
		return nil, err
	}
	if genesis == nil || genesis.Genesis == nil {
		return nil, errors.New("consensus client returned empty genesis information")
	}
	genesisHeight := genesis.Genesis.InitialHeight
	latestHeight, err := i.latestHeight(ctx)
	if err != nil {
		return nil, err
	}
	if latestHeight < genesisHeight {
		return nil, fmt.Errorf("latest height %d precedes genesis height %d", latestHeight, genesisHeight)
	}
	earliestHeight, err := i.earliestHeight(ctx)
	if err != nil {
		return nil, err
	}
	if earliestHeight < genesisHeight {
		earliestHeight = genesisHeight
	}
	if earliestHeight > latestHeight {
		return nil, fmt.Errorf("earliest height %d exceeds latest height %d", earliestHeight, latestHeight)
	}
	switch lastBlock {
	case rpc.SafeBlockNumber, rpc.FinalizedBlockNumber, rpc.LatestBlockNumber, rpc.PendingBlockNumber:
		lastBlockNumber = latestHeight
	case rpc.EarliestBlockNumber:
		lastBlockNumber = earliestHeight
	default:
		if lastBlockNumber > latestHeight {
			return nil, fmt.Errorf("requested last block %d is not yet available; safe latest is %d", lastBlockNumber, latestHeight)
		}
	}

	if lastBlockNumber < earliestHeight {
		return nil, errors.New("requested last block is before earliest available height")
	}

	if uint64(lastBlockNumber-earliestHeight) < uint64(blockCount) { //nolint:gosec
		result.OldestBlock = (*hexutil.Big)(big.NewInt(earliestHeight))
	} else {
		result.OldestBlock = (*hexutil.Big)(big.NewInt(lastBlockNumber - int64(blockCount) + 1)) //nolint:gosec
	}

	result.Reward = [][]*hexutil.Big{}
	result.GasUsedRatio = []float64{}
	// True only after we append header base fee for lastBlockNumber (avoids redundant CheckVersion and
	// avoids appending a child base fee when the last block had no header entry, e.g. pruned base fee).
	lastBlockHeaderBaseFeeAppended := false
	// Potentially parallelize the following logic
	for blockNum := result.OldestBlock.ToInt().Int64(); ; blockNum++ {
		var gasUsedRatio float64

		sdkCtx := i.ctxProvider(blockNum)
		versionExists := CheckVersion(sdkCtx, i.keeper) == nil
		if !versionExists {
			return nil, fmt.Errorf("EVM state is unavailable at height %d", blockNum)
		}
		calculatedRatio, err := i.CalculateGasUsedRatio(ctx, blockNum)
		if err != nil {
			return nil, fmt.Errorf("calculate gas-used ratio for block %d: %w", blockNum, err)
		}
		gasUsedRatio = calculatedRatio
		result.GasUsedRatio = append(result.GasUsedRatio, gasUsedRatio)

		baseFee, err := i.getHeaderBaseFee(blockNum)
		if err != nil {
			return nil, err
		}
		if baseFee == nil {
			return nil, fmt.Errorf("base fee is unavailable at height %d", blockNum)
		}
		result.BaseFee = append(result.BaseFee, (*hexutil.Big)(baseFee))
		if blockNum == lastBlockNumber {
			lastBlockHeaderBaseFeeAppended = true
		}
		height := blockNum
		block, err := blockByNumberRespectingWatermarks(ctx, i.tmClient, i.watermarks, &height, 1)
		if err != nil {
			return nil, fmt.Errorf("load canonical block %d: %w", blockNum, err)
		}
		rewards, err := i.getRewards(block, baseFee, rewardPercentiles)
		if err != nil {
			return nil, err
		}
		result.Reward = append(result.Reward, rewards)
		if blockNum == lastBlockNumber {
			break
		}
	}

	// execution-apis eth_feeHistory / go-ethereum: baseFeePerGas has one more element than gasUsedRatio,
	// the projected base fee for the child of the newest block in the range.
	if lastBlockHeaderBaseFeeAppended {
		childBF, err := i.getChildBaseFeeAfter(lastBlockNumber)
		if err != nil {
			return nil, err
		}
		if childBF == nil {
			return nil, fmt.Errorf("child base fee is unavailable after height %d", lastBlockNumber)
		}
		result.BaseFee = append(result.BaseFee, (*hexutil.Big)(childBF))
	}
	if len(result.BaseFee) != len(result.GasUsedRatio)+1 || len(result.Reward) != len(result.GasUsedRatio) {
		return nil, errors.New("fee history is incomplete")
	}

	return result, nil
}

func (i *InfoAPI) MaxPriorityFeePerGas(ctx context.Context) (fee *hexutil.Big, returnErr error) {
	// Checks the most recent block. If it has high gas used, it will return the reward of the 50% percentile.
	// Otherwise, since the previous block has low gas used, a user shouldn't need to tip a high amount to get included,
	// so a default value is returned.
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, "eth_maxPriorityFeePerGas", i.connectionType, startTime, returnErr, recover())
	}()
	totalGasUsed, err := i.getCongestionData(ctx, nil)
	if err != nil {
		return nil, err
	}
	isChainCongested, err := i.isChainCongested(totalGasUsed)
	if err != nil {
		return nil, err
	}
	if !isChainCongested {
		// chain is not congested, return 1gwei as the default priority fee per gas
		return (*hexutil.Big)(big.NewInt(defaultPriorityFeePerGas)), nil
	}
	// chain is congested, return the 50%-tile reward as the priority fee per gas
	feeHist, err := i.FeeHistory(ctx, 1, rpc.LatestBlockNumber, []float64{50})
	if err != nil {
		return nil, err
	}
	if len(feeHist.Reward) == 0 || len(feeHist.Reward[0]) == 0 {
		// if there is no EVM tx in the most recent block, return 0
		return (*hexutil.Big)(big.NewInt(0)), nil
	}
	return (*hexutil.Big)(feeHist.Reward[0][0].ToInt()), nil
}

func (i *InfoAPI) BlobBaseFee(ctx context.Context) (result *hexutil.Big, returnErr error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, "eth_BlobBaseFee", i.connectionType, startTime, returnErr, recover())
	}()
	return nil, &ErrEVMNotSupported{Msg: "blobs not supported on this chain"}
}

// Syncing implements eth_syncing. It is intentionally registered (not removed): the RPC returns
// JSON-RPC error -32000 with a clear message instead of -32601 method not found. Ethereum returns
// false or a sync object; Pax does not expose sync semantics on this API.
func (i *InfoAPI) Syncing(ctx context.Context) (result any, returnErr error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, "eth_Syncing", i.connectionType, startTime, returnErr, recover())
	}()
	return nil, &ErrEVMNotSupported{Msg: "eth_syncing is not supported on Pax EVM RPC"}
}

// getHeaderBaseFee returns the base fee per gas for txs in block blockNum (same as eth block header
// and encodeRPCTransaction: GetNextBaseFee at parent committed height).
func (i *InfoAPI) getHeaderBaseFee(blockNum int64) (res *big.Int, returnErr error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			res = nil
			returnErr = fmt.Errorf("get header base fee for block %d: %v", blockNum, recovered)
		}
	}()
	if blockNum <= 1 {
		return evmtypes.DefaultMinFeePerGas.TruncateInt().BigInt(), nil
	}
	baseFee := i.keeper.GetNextBaseFeePerGas(i.ctxProvider(blockNum - 1))
	res = baseFee.TruncateInt().BigInt()
	return res, nil
}

// getChildBaseFeeAfter returns the base fee for the block after parentBlockNum (GetNextBaseFee at end of parentBlockNum).
func (i *InfoAPI) getChildBaseFeeAfter(parentBlockNum int64) (res *big.Int, returnErr error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			res = nil
			returnErr = fmt.Errorf("get child base fee after block %d: %v", parentBlockNum, recovered)
		}
	}()
	if parentBlockNum < 1 {
		return evmtypes.DefaultMinFeePerGas.TruncateInt().BigInt(), nil
	}
	baseFee := i.keeper.GetNextBaseFeePerGas(i.ctxProvider(parentBlockNum))
	res = baseFee.TruncateInt().BigInt()
	return res, nil
}

type GasAndReward struct {
	GasUsed uint64
	Reward  *big.Int
}

func (i *InfoAPI) getRewards(block *coretypes.ResultBlock, baseFee *big.Int, rewardPercentiles []float64) ([]*hexutil.Big, error) {
	if block == nil || block.Block == nil {
		return nil, errors.New("cannot calculate rewards without a canonical block")
	}
	blockHeight := block.Block.Height
	if blockHeight < 0 {
		return nil, fmt.Errorf("cannot calculate rewards for negative block height %d", blockHeight)
	}
	if baseFee == nil || baseFee.Sign() < 0 {
		return nil, errors.New("cannot calculate rewards with an invalid base fee")
	}
	GasAndRewards := []GasAndReward{}
	totalEVMGasUsed := uint64(0)
	for _, txbz := range block.Block.Txs {
		ethtx := getEthTxForTxBz(txbz, i.txConfigProvider(block.Block.Height).TxDecoder())
		if ethtx == nil {
			// not evm tx
			continue
		}
		// okay to get from latest since receipt is immutable
		receipt, err := i.keeper.GetReceipt(i.ctxProvider(LatestCtxHeight), ethtx.Hash())
		if err != nil {
			return nil, fmt.Errorf("load receipt for %s: %w", ethtx.Hash().Hex(), err)
		}
		if receipt == nil {
			return nil, fmt.Errorf("receipt lookup for %s returned nil", ethtx.Hash().Hex())
		}
		if receipt.BlockNumber != uint64(blockHeight) {
			return nil, fmt.Errorf("receipt for %s belongs to block %d, expected %d", ethtx.Hash().Hex(), receipt.BlockNumber, blockHeight)
		}
		receiptEffectiveGasPrice := new(big.Int).SetUint64(receipt.EffectiveGasPrice)
		if receiptEffectiveGasPrice.Cmp(baseFee) < 0 {
			// if effective gas price is 0, it's expected behavior for txs that failed ante.
			// if it's not zero but still smaller than baseFee then something is wrong.
			if receiptEffectiveGasPrice.Cmp(common.Big0) != 0 {
				return nil, fmt.Errorf("receipt for %s has gas price %s below base fee %s", ethtx.Hash().Hex(), receiptEffectiveGasPrice, baseFee)
			}
			continue
		}
		reward := new(big.Int).Sub(new(big.Int).SetUint64(receipt.EffectiveGasPrice), baseFee)
		GasAndRewards = append(GasAndRewards, GasAndReward{GasUsed: receipt.GasUsed, Reward: reward})
		if math.MaxUint64-totalEVMGasUsed < receipt.GasUsed {
			return nil, fmt.Errorf("gas-used total overflows for block %d", block.Block.Height)
		}
		totalEVMGasUsed += receipt.GasUsed
	}
	return CalculatePercentiles(rewardPercentiles, GasAndRewards, totalEVMGasUsed), nil
}

func (i *InfoAPI) getCongestionData(ctx context.Context, height *int64) (blockGasUsed uint64, err error) {
	block, err := blockByNumberRespectingWatermarks(ctx, i.tmClient, i.watermarks, height, 1)
	if err != nil {
		return 0, err
	}
	if block == nil || block.Block == nil {
		return 0, errors.New("canonical block response is empty")
	}
	if block.Block.Height < 0 {
		return 0, fmt.Errorf("canonical block height %d is negative", block.Block.Height)
	}
	totalEVMGasUsed := uint64(0)
	for _, txbz := range block.Block.Txs {
		ethtx := getEthTxForTxBz(txbz, i.txConfigProvider(block.Block.Height).TxDecoder())
		if ethtx == nil {
			// not evm tx
			continue
		}
		// okay to get from latest since receipt is immutable
		receipt, err := i.keeper.GetReceiptWithRetry(i.ctxProvider(LatestCtxHeight), ethtx.Hash(), 3)
		if err != nil {
			return 0, err
		}
		if receipt == nil {
			return 0, fmt.Errorf("receipt lookup for %s returned nil", ethtx.Hash().Hex())
		}
		// We've had issues where is included in a block and fails but then is retried and included in a later block, overwriting the receipt.
		// This is a temporary fix to ensure we only consider receipts that are included in the block we're querying.
		if receipt.BlockNumber != uint64(block.Block.Height) { //nolint:gosec
			return 0, fmt.Errorf("receipt for %s belongs to block %d, expected %d", ethtx.Hash().Hex(), receipt.BlockNumber, block.Block.Height)
		}
		if math.MaxUint64-totalEVMGasUsed < receipt.GasUsed {
			return 0, fmt.Errorf("gas-used total overflows for block %d", block.Block.Height)
		}
		totalEVMGasUsed += receipt.GasUsed
	}
	return totalEVMGasUsed, nil
}

// CalculateGasUsedRatio calculates the actual gas used ratio for a specific block
func (i *InfoAPI) CalculateGasUsedRatio(ctx context.Context, blockHeight int64) (float64, error) {
	block, err := blockByNumberRespectingWatermarks(ctx, i.tmClient, i.watermarks, &blockHeight, 1)
	if err != nil {
		return 0, err
	}
	if block == nil || block.Block == nil {
		return 0, errors.New("canonical block response is empty")
	}
	if block.Block.Height < 0 {
		return 0, fmt.Errorf("canonical block height %d is negative", block.Block.Height)
	}

	sdkCtx := i.ctxProvider(blockHeight)
	if sdkCtx.ConsensusParams() == nil || sdkCtx.ConsensusParams().Block == nil {
		return 0, fmt.Errorf("consensus block parameters are unavailable at height %d", blockHeight)
	}
	gasLimit, err := encodeHeadGasLimit(sdkCtx.ConsensusParams().Block.MaxGas)
	if err != nil {
		return 0, fmt.Errorf("invalid gas limit at height %d: %w", blockHeight, err)
	}
	gasLimitValue := uint64(gasLimit)
	if gasLimitValue == 0 {
		return 0, fmt.Errorf("gas limit is zero at height %d", blockHeight)
	}

	// Calculate total gas used by EVM transactions in this block
	totalEVMGasUsed := uint64(0)
	for _, txbz := range block.Block.Txs {
		ethtx := getEthTxForTxBz(txbz, i.txDecoder)
		if ethtx == nil {
			// not evm tx
			continue
		}
		// okay to get from latest since receipt is immutable
		receipt, err := i.keeper.GetReceiptWithRetry(i.ctxProvider(LatestCtxHeight), ethtx.Hash(), 3)
		if err != nil {
			return 0, err
		}
		if receipt == nil {
			return 0, fmt.Errorf("receipt lookup for %s returned nil", ethtx.Hash().Hex())
		}
		// We've had issues where tx is included in a block and fails but then is retried and included in a later block, overwriting the receipt.
		// This is a temporary fix to ensure we only consider receipts that are included in the block we're querying.
		if receipt.BlockNumber != uint64(block.Block.Height) { //nolint:gosec
			return 0, fmt.Errorf("receipt for %s belongs to block %d, expected %d", ethtx.Hash().Hex(), receipt.BlockNumber, block.Block.Height)
		}
		if math.MaxUint64-totalEVMGasUsed < receipt.GasUsed {
			return 0, fmt.Errorf("gas-used total overflows for block %d", block.Block.Height)
		}
		totalEVMGasUsed += receipt.GasUsed
	}

	if totalEVMGasUsed > gasLimitValue {
		return 0, fmt.Errorf("gas used %d exceeds block gas limit %d at height %d", totalEVMGasUsed, gasLimitValue, blockHeight)
	}
	return float64(totalEVMGasUsed) / float64(gasLimitValue), nil
}

func (i *InfoAPI) latestHeight(ctx context.Context) (int64, error) {
	return i.watermarks.LatestHeight(ctx)
}

func (i *InfoAPI) earliestHeight(ctx context.Context) (int64, error) {
	return i.watermarks.EarliestHeight(ctx)
}

// Following go-ethereum implementation
// Specifically, the reward value at a percentile of p% will be the reward value of the
// lowest-rewarded transaction such that the sum of its gasUsed value and gasUsed values
// of all lower-rewarded transactions is no less than (total gasUsed * p%).
func CalculatePercentiles(rewardPercentiles []float64, GasAndRewards []GasAndReward, totalEVMGasUsed uint64) []*hexutil.Big {
	slices.SortStableFunc(GasAndRewards, func(a, b GasAndReward) int {
		return a.Reward.Cmp(b.Reward)
	})
	res := []*hexutil.Big{}
	if len(GasAndRewards) == 0 {
		// Return array of zeros for each percentile when no transactions exist
		for range rewardPercentiles {
			res = append(res, (*hexutil.Big)(big.NewInt(0)))
		}
		return res
	}
	var txIndex int
	sumGasUsed := GasAndRewards[0].GasUsed
	for _, p := range rewardPercentiles {
		thresholdGasUsed := uint64(float64(totalEVMGasUsed) * p / 100)
		for sumGasUsed < thresholdGasUsed && txIndex < len(GasAndRewards)-1 {
			txIndex++
			sumGasUsed += GasAndRewards[txIndex].GasUsed
		}
		res = append(res, (*hexutil.Big)(GasAndRewards[txIndex].Reward))
	}
	return res
}

func (i *InfoAPI) isChainCongested(totalGasUsed uint64) (bool, error) {
	sdkCtx := i.ctxProvider(LatestCtxHeight)
	if sdkCtx.ConsensusParams() == nil || sdkCtx.ConsensusParams().Block == nil {
		return false, errors.New("current consensus block parameters are unavailable")
	}
	gasLimit, err := encodeHeadGasLimit(sdkCtx.ConsensusParams().Block.MaxGas)
	if err != nil {
		return false, err
	}
	gasLimitValue := uint64(gasLimit)
	if gasLimitValue == 0 {
		return false, errors.New("current block gas limit is zero")
	}
	thresholdPercentage := uint64(defaultThresholdPercentage)
	if gasLimitValue > math.MaxUint64/thresholdPercentage {
		return false, errors.New("current block gas limit is too large to calculate congestion threshold")
	}
	threshold := gasLimitValue * thresholdPercentage / 100
	return totalGasUsed > threshold, nil
}

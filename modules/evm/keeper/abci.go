package keeper

import (
	"fmt"
	"math"
	"time"

	ethtypes "github.com/ethereum/go-ethereum/core/types"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core"
	"github.com/ethereum/go-ethereum/core/vm"
	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/sdk/telemetry"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
	"github.com/sidiora-labs/paxeer-network/utils"
	utilmetrics "github.com/sidiora-labs/paxeer-network/utils/metrics"
)

func (k *Keeper) BeginBlock(ctx sdk.Context) {
	beginBlockerStart := time.Now()
	defer func() {
		telemetry.ModuleMeasureSince(types.ModuleName, beginBlockerStart, telemetry.MetricKeyBeginBlocker) // TODO(PLT-330): remove once evm_abci_begin_blocker_duration_seconds verified
		evmKeeperMetrics.beginBlockerDuration.Record(ctx.Context(), time.Since(beginBlockerStart).Seconds())
	}()
	// clear tx/tx responses from last block
	if !ctx.IsTracing() {
		k.SetMsgs([]*types.MsgEVMTransaction{})
		k.SetTxResults([]*abci.ExecTxResult{})
	}
	// mock beacon root if replaying
	if k.EthReplayConfig.Enabled {
		if beaconRoot := k.ReplayBlock.BeaconRoot(); beaconRoot != nil {
			blockCtx, err := k.GetVMBlockContext(ctx, core.GasPool(math.MaxUint64))
			if err != nil {
				panic(err)
			}
			statedb := state.NewDBImpl(ctx, k, false)
			vmenv := vm.NewEVM(*blockCtx, statedb, types.DefaultChainConfig().EthereumConfig(k.ChainID(ctx)), vm.Config{}, k.CustomPrecompiles(ctx))
			core.ProcessBeaconBlockRoot(*beaconRoot, vmenv)
			_, err = statedb.Finalize()
			if err != nil {
				panic(err)
			}
		}
	}
	if k.EthBlockTestConfig.Enabled {
		parentHash := common.BytesToHash(ctx.BlockHeader().LastBlockId.Hash)
		blockCtx, err := k.GetVMBlockContext(ctx, core.GasPool(math.MaxUint64))
		if err != nil {
			panic(err)
		}
		statedb := state.NewDBImpl(ctx, k, false)
		vmenv := vm.NewEVM(*blockCtx, statedb, types.DefaultChainConfig().EthereumConfig(k.ChainID(ctx)), vm.Config{}, k.CustomPrecompiles(ctx))
		core.ProcessParentBlockHash(parentHash, vmenv)
		_, err = statedb.Finalize()
		if err != nil {
			panic(err)
		}
	}
}

func failEndBlockOnError(operation string, err error) {
	if err != nil {
		panic(fmt.Errorf("end block: %s: %w", operation, err))
	}
}

func (k *Keeper) EndBlock(ctx sdk.Context, height int64, blockGasUsed int64) {
	endBlockerStart := time.Now()
	defer func() {
		telemetry.ModuleMeasureSince(types.ModuleName, endBlockerStart, telemetry.MetricKeyEndBlocker) // TODO(PLT-330): remove once evm_abci_end_blocker_duration_seconds verified
		evmKeeperMetrics.endBlockerDuration.Record(ctx.Context(), time.Since(endBlockerStart).Seconds())
	}()
	// Bake height-1: at EndBlock(N) the indexer's safe latest is N-1. When
	// the snapshot store is wired, also Put a memiavl snapshot keyed by
	// its committed version (= N-1, since Commit fires after EndBlock);
	// the baker tracing block H looks up snapshot[H-1].
	if !ctx.IsTracing() && height > 1 {
		if k.traceSnapshotStore != nil && k.traceSnapshotCapture != nil {
			if snap := k.traceSnapshotCapture(); snap != nil {
				k.traceSnapshotStore.Put(snap.Version(), snap)
			}
		}
		k.traceDB.Enqueue(height - 1)
	}
	// TODO: remove after all TxHashes have been removed
	k.RemoveFirstNTxHashes(ctx, DefaultTxHashesToRemove)

	// Migrate legacy EVM receipts to receipt.db in small batches every N blocks
	if ctx.BlockHeight()%LegacyReceiptMigrationInterval == 0 {
		if migrated, err := k.MigrateLegacyReceiptsBatch(ctx, LegacyReceiptMigrationBatchSize); err != nil {
			logger.Error("failed migrating legacy receipts", "err", err)
		} else if migrated > 0 {
			logger.Info("migrated legacy EVM receipts to receipt.db", "count", migrated)
		}
	}

	if scanned, deleted := k.PruneZeroStorageSlots(ctx, ZeroStorageCleanupBatchSize); deleted > 0 {
		logger.Info("pruned zero-value contract storage slots while scanning keys", "pruned-count", deleted, "key-count", scanned)
	}

	newBaseFee := k.AdjustDynamicBaseFeePerGas(ctx, uint64(blockGasUsed)) // nolint:gosec
	if newBaseFee != nil {
		baseFeeBI := newBaseFee.TruncateInt().BigInt()
		utilmetrics.GaugeEvmBlockBaseFee(baseFeeBI, height) // TODO(PLT-330): remove once evm_block_base_fee verified
		evmKeeperMetrics.blockBaseFee.Record(ctx.Context(), bigIntToFloat64(baseFeeBI))
	}
	var coinbase sdk.AccAddress
	if k.EthBlockTestConfig.Enabled {
		blocks := k.BlockTest.Json.Blocks
		block, err := blocks[ctx.BlockHeight()-1].Decode()
		if err != nil {
			panic(err)
		}
		coinbase = k.GetPaxAddressOrDefault(ctx, block.Header_.Coinbase)
	} else if k.EthReplayConfig.Enabled {
		coinbase = k.GetPaxAddressOrDefault(ctx, k.ReplayBlock.Header_.Coinbase)
		k.SetReplayedHeight(ctx)
	} else {
		coinbase = k.AccountKeeper().GetModuleAddress(authtypes.FeeCollectorName)
	}
	evmTxDeferredInfoList := k.GetAllEVMTxDeferredInfo(ctx)
	denom := k.GetBaseDenom(ctx)
	surplus, err := k.GetAnteSurplusSum(ctx)
	failEndBlockOnError("sum ante surplus", err)
	for _, deferredInfo := range evmTxDeferredInfoList {
		txHash := common.BytesToHash(deferredInfo.TxHash)
		if deferredInfo.Error != "" && txHash.Cmp(ethtypes.EmptyTxsHash) != 0 {
			if !k.GetNonceBumped(ctx, deferredInfo.TxIndex) {
				continue
			}
			err := k.SetTransientReceipt(ctx, txHash, &types.Receipt{
				TxHashHex:        txHash.Hex(),
				TransactionIndex: deferredInfo.TxIndex,
				VmError:          deferredInfo.Error,
				BlockNumber:      uint64(ctx.BlockHeight()), // nolint:gosec
			})
			failEndBlockOnError(fmt.Sprintf("persist failed receipt for transaction %s", txHash.Hex()), err)
			continue
		}
		idx := int(deferredInfo.TxIndex)
		coinbaseAddress := state.GetCoinbaseAddress(idx)
		uhpxBalance := k.BankKeeper().GetBalance(ctx, coinbaseAddress, denom).Amount
		lockedUhpxBalance := k.BankKeeper().LockedCoins(ctx, coinbaseAddress).AmountOf(denom)
		balance := uhpxBalance.Sub(lockedUhpxBalance)
		weiBalance := k.BankKeeper().GetWeiBalance(ctx, coinbaseAddress)
		if !balance.IsZero() || !weiBalance.IsZero() {
			err := k.BankKeeper().SendCoinsAndWei(ctx, coinbaseAddress, coinbase, balance, weiBalance)
			failEndBlockOnError(fmt.Sprintf("sweep coinbase surplus from %s", coinbaseAddress), err)
		}
		surplus = surplus.Add(deferredInfo.Surplus)
	}
	if surplus.IsPositive() {
		surplusUhpx, surplusWei := state.SplitUhpxWeiAmount(surplus.BigInt())
		if surplusUhpx.GT(sdk.ZeroInt()) {
			err := k.BankKeeper().AddCoins(ctx, k.AccountKeeper().GetModuleAddress(types.ModuleName), sdk.NewCoins(sdk.NewCoin(k.GetBaseDenom(ctx), surplusUhpx)), true)
			failEndBlockOnError(fmt.Sprintf("credit uhpx surplus %s to EVM module account", surplusUhpx), err)
		}
		if surplusWei.GT(sdk.ZeroInt()) {
			err := k.BankKeeper().AddWei(ctx, k.AccountKeeper().GetModuleAddress(types.ModuleName), surplusWei)
			failEndBlockOnError(fmt.Sprintf("credit wei surplus %s to EVM module account", surplusWei), err)
		}
	}
	allBlooms := utils.Map(evmTxDeferredInfoList, func(i *types.DeferredInfo) ethtypes.Bloom { return ethtypes.BytesToBloom(i.TxBloom) })
	evmOnlyBlooms := make([]ethtypes.Bloom, 0, len(evmTxDeferredInfoList))
	for _, di := range evmTxDeferredInfoList {
		if len(di.TxHash) == 0 {
			continue
		}
		r, err := k.GetTransientReceipt(ctx, common.BytesToHash(di.TxHash), uint64(di.TxIndex))
		if err != nil {
			continue
		}
		// Only EVM receipts in this block that are not synthetic
		if r.TxType == types.ShellEVMTxType || r.BlockNumber != uint64(ctx.BlockHeight()) { //nolint:gosec
			continue
		}
		if len(r.Logs) == 0 {
			continue
		}
		// Re-create a per-tx bloom from EVM-only logs (exclude synthetic receipts but not synthetic logs)
		evmOnlyBloom := ethtypes.CreateBloom(&ethtypes.Receipt{
			Logs: GetLogsForTx(r, 0),
		})
		evmOnlyBlooms = append(evmOnlyBlooms, evmOnlyBloom)
	}
	k.SetBlockBloom(ctx, allBlooms)
	k.SetEvmOnlyBlockBloom(ctx, evmOnlyBlooms)
}

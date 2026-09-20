package legacyabci

import (
	"fmt"
	"time"

	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	evmante "github.com/sidiora-labs/paxeer-network/modules/evm/ante"
	evmkeeper "github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/trace"

	gometrics "github.com/armon/go-metrics"
	ibckeeper "github.com/sidiora-labs/paxeer-network/interchain/modules/core/keeper"
	oraclekeeper "github.com/sidiora-labs/paxeer-network/modules/oracle/keeper"
	"github.com/sidiora-labs/paxeer-network/node/ante"
	"github.com/sidiora-labs/paxeer-network/sdk/client"
	"github.com/sidiora-labs/paxeer-network/sdk/telemetry"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
	"github.com/sidiora-labs/paxeer-network/sdk/utils/tracing"
	authkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/auth/keeper"
	bankkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/bank/keeper"
	feegrantkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/feegrant/keeper"
	paramskeeper "github.com/sidiora-labs/paxeer-network/sdk/x/params/keeper"
	upgradekeeper "github.com/sidiora-labs/paxeer-network/sdk/x/upgrade/keeper"
	otelmetric "go.opentelemetry.io/otel/metric"
)

var defaultRecoveryMiddleware = newDefaultRecoveryMiddleware()

type CheckTxKeepers struct {
	AccountKeeper  authkeeper.AccountKeeper
	BankKeeper     bankkeeper.Keeper
	FeeGrantKeeper *feegrantkeeper.Keeper
	IBCKeeper      *ibckeeper.Keeper
	OracleKeeper   oraclekeeper.Keeper
	EvmKeeper      *evmkeeper.Keeper
	ParamsKeeper   paramskeeper.Keeper
	UpgradeKeeper  *upgradekeeper.Keeper
}

func CheckTx(
	ctx sdk.Context,
	tx sdk.Tx,
	txConfig client.TxConfig,
	keepers *CheckTxKeepers,
	checksum [32]byte,
	contextCacher func(sdk.Context) (sdk.Context, sdk.CacheMultiStore),
	latestCtxGetter func() sdk.Context,
	tracingInfo *tracing.Info,
) (
	gInfo sdk.GasInfo,
	result *sdk.Result,
	txCtx sdk.Context,
	err error,
) {
	label := "check"
	if ctx.IsReCheckTx() {
		label = "recheck"
	}
	txStart := time.Now()
	defer func() {
		legacyAbciMetrics.txDuration.Record(ctx.Context(), time.Since(txStart).Seconds(), otelmetric.WithAttributes(attribute.String("mode", label)))
		// TODO(PLT-343): remove once tx_duration verified
		telemetry.MeasureThroughputSinceWithLabels(
			telemetry.TxCount,
			[]gometrics.Label{
				telemetry.NewLabel("mode", label),
			},
			txStart,
		)
	}()
	spanCtx, span := tracingInfo.StartWithContext("CheckTx", ctx.TraceSpanContext())
	defer span.End()
	ctx = ctx.WithTraceSpanContext(spanCtx)
	span.SetAttributes(attribute.String("txHash", fmt.Sprintf("%X", checksum)))
	var gasWanted uint64
	var gasEstimate uint64

	blockGasMeter := ctx.GasMeter()
	defer func() {
		if r := recover(); r != nil {
			recoveryMW := newOutOfGasRecoveryMiddleware(gasWanted, ctx, defaultRecoveryMiddleware)
			err, result = processRecovery(r, recoveryMW), nil
		}
		if ctx.GasMeter() == blockGasMeter {
			return
		}
		gInfo = sdk.GasInfo{GasWanted: gasWanted, GasUsed: ctx.GasMeter().GasConsumed(), GasEstimate: gasEstimate}
	}()

	if tx == nil {
		return sdk.GasInfo{}, nil, ctx, sdkerrors.Wrap(sdkerrors.ErrTxDecode, "tx decode error")
	}

	var anteSpan trace.Span
	// trace AnteHandler
	_, anteSpan = tracingInfo.StartWithContext("AnteHandler", ctx.TraceSpanContext())
	defer anteSpan.End()
	anteCtx, _ := contextCacher(ctx)
	anteCtx = anteCtx.WithEventManager(sdk.NewEventManager())
	var newCtx sdk.Context
	if isEVM, evmerr := evmante.IsEVMMessage(tx); evmerr != nil {
		err = evmerr
	} else if isEVM {
		newCtx, err = ante.EvmCheckTxAnte(anteCtx, tx, keepers.UpgradeKeeper, keepers.EvmKeeper)
	} else {
		newCtx, err = ante.CosmosCheckTxAnte(anteCtx, txConfig, tx, keepers.ParamsKeeper, keepers.OracleKeeper, keepers.EvmKeeper, keepers.AccountKeeper, keepers.BankKeeper, keepers.FeeGrantKeeper, keepers.IBCKeeper)
	}
	if !newCtx.IsZero() {
		ctx = newCtx
	}

	if err != nil {
		return gInfo, nil, ctx, err
	}
	// GasMeter expected to be set in AnteHandler
	gasWanted = ctx.GasMeter().Limit()
	gasEstimate = ctx.GasEstimate()
	anteSpan.End()

	return gInfo, &sdk.Result{Events: []abci.Event{}}, ctx, err
}

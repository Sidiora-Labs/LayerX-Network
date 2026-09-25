package app_test

import (
	"crypto/sha256"
	"encoding/hex"
	"go/ast"
	"go/parser"
	"go/token"
	"math/big"
	"testing"

	"github.com/sidiora-labs/paxeer-network/sdk/client"
	"github.com/sidiora-labs/paxeer-network/sdk/client/tx"
	cryptotypes "github.com/sidiora-labs/paxeer-network/sdk/crypto/types"
	"github.com/sidiora-labs/paxeer-network/sdk/testutil/testdata"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/types/tx/signing"
	wasmtypes "github.com/sidiora-labs/paxeer-network/wasm/x/wasm/types"

	ethtypes "github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	evmtypes "github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types/ethtx"
	oracletypes "github.com/sidiora-labs/paxeer-network/modules/oracle/types"
	app "github.com/sidiora-labs/paxeer-network/node"
	"github.com/sidiora-labs/paxeer-network/node/antedecorators"
	"github.com/sidiora-labs/paxeer-network/node/apptesting"
	"github.com/sidiora-labs/paxeer-network/sdk/utils/tracing"
	"github.com/sidiora-labs/paxeer-network/sdk/x/auth/ante"
	xauthsigning "github.com/sidiora-labs/paxeer-network/sdk/x/auth/signing"
	banktypes "github.com/sidiora-labs/paxeer-network/sdk/x/bank/types"
	paramtypes "github.com/sidiora-labs/paxeer-network/sdk/x/params/types"
	stakingtypes "github.com/sidiora-labs/paxeer-network/sdk/x/staking/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
	"github.com/stretchr/testify/suite"
	"go.opentelemetry.io/otel"
)

// AnteTestSuite is a test suite to be used with ante handler tests.
type AnteTestSuite struct {
	apptesting.KeeperTestHelper

	anteHandler sdk.AnteHandler
	clientCtx   client.Context
	txBuilder   client.TxBuilder
	testAcc     sdk.AccAddress
	testAccPriv cryptotypes.PrivKey
}

func TestKeeperTestSuite(t *testing.T) {
	suite.Run(t, new(AnteTestSuite))
}

// SetupTest setups a new test, with new app, context, and anteHandler.
func (suite *AnteTestSuite) SetupTest(isCheckTx bool) {
	suite.Setup()

	// keys and addresses
	suite.testAccPriv, _, suite.testAcc = testdata.KeyTestPubAddr()
	initalBalance := sdk.Coins{sdk.NewInt64Coin("uhpx", 100000000000)}
	suite.FundAcc(suite.testAcc, initalBalance)

	suite.Ctx = suite.Ctx.WithBlockHeight(1)

	// Set up TxConfig.
	encodingConfig := app.MakeEncodingConfig()
	// We're using TestMsg encoding in some tests, so register it here.
	encodingConfig.Amino.RegisterConcrete(&testdata.TestMsg{}, "testdata.TestMsg", nil)
	testdata.RegisterInterfaces(encodingConfig.InterfaceRegistry)

	suite.clientCtx = client.Context{}.
		WithTxConfig(encodingConfig.TxConfig)

	wasmConfig := wasmtypes.DefaultWasmConfig()
	defaultTracer, _ := tracing.DefaultTracerProvider()
	otel.SetTracerProvider(defaultTracer)
	tr := defaultTracer.Tracer("component-main")

	tracingInfo := tracing.NewTracingInfo(tr, true)
	antehandler, _, err := app.NewAnteHandler(
		app.HandlerOptions{
			HandlerOptions: ante.HandlerOptions{
				AccountKeeper:   suite.App.AccountKeeper,
				BankKeeper:      suite.App.BankKeeper,
				FeegrantKeeper:  suite.App.FeeGrantKeeper,
				ParamsKeeper:    suite.App.ParamsKeeper,
				SignModeHandler: suite.clientCtx.TxConfig.SignModeHandler(),
				SigGasConsumer:  ante.DefaultSigVerificationGasConsumer,
				// BatchVerifier:   app.batchVerifier,
			},
			IBCKeeper:       suite.App.IBCKeeper,
			WasmConfig:      &wasmConfig,
			WasmKeeper:      &suite.App.WasmKeeper,
			OracleKeeper:    &suite.App.OracleKeeper,
			TracingInfo:     tracingInfo,
			EVMKeeper:       &suite.App.EvmKeeper,
			LatestCtxGetter: func() sdk.Context { return suite.Ctx },
		},
	)

	suite.Require().NoError(err)
	suite.anteHandler = antehandler
}

// CreateTestTx is a helper function to create a tx given multiple inputs.
func (suite *AnteTestSuite) CreateTestTx(privs []cryptotypes.PrivKey, accNums []uint64, accSeqs []uint64, chainID string) (xauthsigning.Tx, error) {
	// First round: we gather all the signer infos. We use the "set empty
	// signature" hack to do that.
	var sigsV2 []signing.SignatureV2
	for i, priv := range privs {
		sigV2 := signing.SignatureV2{
			PubKey: priv.PubKey(),
			Data: &signing.SingleSignatureData{
				SignMode:  suite.clientCtx.TxConfig.SignModeHandler().DefaultMode(),
				Signature: nil,
			},
			Sequence: accSeqs[i],
		}

		sigsV2 = append(sigsV2, sigV2)
	}
	err := suite.txBuilder.SetSignatures(sigsV2...)
	if err != nil {
		return nil, err
	}

	// Second round: all signer infos are set, so each signer can sign.
	sigsV2 = []signing.SignatureV2{}
	for i, priv := range privs {
		signerData := xauthsigning.SignerData{
			ChainID:       chainID,
			AccountNumber: accNums[i],
			Sequence:      accSeqs[i],
		}
		sigV2, err := tx.SignWithPrivKey(
			suite.clientCtx.TxConfig.SignModeHandler().DefaultMode(), signerData,
			suite.txBuilder, priv, suite.clientCtx.TxConfig, accSeqs[i])
		if err != nil {
			return nil, err
		}

		sigsV2 = append(sigsV2, sigV2)
	}
	err = suite.txBuilder.SetSignatures(sigsV2...)
	if err != nil {
		return nil, err
	}

	return suite.txBuilder.GetTx(), nil
}

func TestEvmAnteErrorHandler(t *testing.T) {
	ctx := testkeeper.EVMTestApp.GetContextForDeliverTx([]byte{})
	privKey := testkeeper.MockPrivateKey()
	testPrivHex := hex.EncodeToString(privKey.Bytes())
	key, _ := crypto.HexToECDSA(testPrivHex)
	txData := ethtypes.LegacyTx{
		GasPrice: big.NewInt(1000000000000),
		Gas:      200000,
		To:       nil,
		Value:    big.NewInt(0),
		Data:     []byte{},
		Nonce:    1, // will cause ante error
	}
	chainID := testkeeper.EVMTestApp.EvmKeeper.ChainID(ctx)
	chainCfg := evmtypes.DefaultChainConfig()
	ethCfg := chainCfg.EthereumConfig(chainID)
	blockNum := big.NewInt(ctx.BlockHeight())
	signer := ethtypes.MakeSigner(ethCfg, blockNum, uint64(ctx.BlockTime().Unix()))
	tx, err := ethtypes.SignTx(ethtypes.NewTx(&txData), signer, key)
	require.Nil(t, err)
	txwrapper, err := ethtx.NewLegacyTx(tx)
	require.Nil(t, err)
	req, err := evmtypes.NewMsgEVMTransaction(txwrapper)
	require.Nil(t, err)
	builder := testkeeper.EVMTestApp.GetTxConfig().NewTxBuilder()
	builder.SetMsgs(req)
	txToSend := builder.GetTx()
	encodedTx, err := testkeeper.EVMTestApp.GetTxConfig().TxEncoder()(txToSend)
	require.Nil(t, err)

	addr, _ := testkeeper.PrivateKeyToAddresses(privKey)
	testkeeper.EVMTestApp.BankKeeper.AddCoins(ctx, addr, sdk.NewCoins(sdk.NewCoin("uhpx", sdk.NewInt(100000000000))), true)
	res := testkeeper.EVMTestApp.DeliverTx(ctx, abci.RequestDeliverTxV2{Tx: encodedTx}, txToSend, sha256.Sum256(encodedTx))
	require.NotEqual(t, 0, res.Code)
	testkeeper.EVMTestApp.EvmKeeper.SetTxResults([]*abci.ExecTxResult{{
		Code: res.Code,
		Log:  "nonce too high",
	}})
	testkeeper.EVMTestApp.EvmKeeper.SetMsgs([]*evmtypes.MsgEVMTransaction{req})
	deferredInfo := testkeeper.EVMTestApp.EvmKeeper.GetAllEVMTxDeferredInfo(ctx)
	require.Equal(t, 1, len(deferredInfo))
	require.Contains(t, deferredInfo[0].Error, "nonce too high")
}

func TestFeeDenomDecoratorOrder(t *testing.T) {
	file, err := parser.ParseFile(token.NewFileSet(), "ante.go", nil, 0)
	require.NoError(t, err)
	var constructors []string
	ast.Inspect(file, func(n ast.Node) bool {
		assignment, ok := n.(*ast.AssignStmt)
		if !ok || len(assignment.Lhs) != 1 {
			return true
		}
		name, ok := assignment.Lhs[0].(*ast.Ident)
		if !ok || name.Name != "anteDecorators" {
			return true
		}
		list := assignment.Rhs[0].(*ast.CompositeLit)
		for _, element := range list.Elts {
			if identifier, ok := element.(*ast.Ident); ok {
				constructors = append(constructors, identifier.Name)
				continue
			}
			call := element.(*ast.CallExpr)
			constructors = append(constructors, call.Fun.(*ast.SelectorExpr).Sel.Name)
			if call.Fun.(*ast.SelectorExpr).Sel.Name == "NewGaslessDecorator" {
				wrapped := call.Args[0].(*ast.CompositeLit)
				require.Len(t, wrapped.Elts, 1)
				deduct := wrapped.Elts[0].(*ast.CallExpr)
				require.Equal(t, "NewDeductFeeDecorator", deduct.Fun.(*ast.SelectorExpr).Sel.Name)
				checker := deduct.Args[4].(*ast.CallExpr)
				require.Equal(t, "NewFeeDenomTxFeeChecker", checker.Fun.(*ast.SelectorExpr).Sel.Name)
				require.Equal(t, "TxFeeChecker", checker.Args[1].(*ast.SelectorExpr).Sel.Name)
			}
		}
		return false
	})
	require.Equal(t, []string{
		"NewSetUpContextDecorator", "NewGaslessDecorator", "NewLimitSimulationGasDecorator",
		"NewRejectExtensionOptionsDecorator", "NewSpammingPreventionDecorator", "NewOracleVoteAloneDecorator",
		"NewValidateBasicDecorator", "NewTxTimeoutHeightDecorator", "NewValidateMemoDecorator",
		"NewConsumeGasForTxSizeDecorator", "NewPriorityDecorator", "NewSetPubKeyDecorator",
		"NewValidateSigCountDecorator", "NewSigGasConsumeDecorator", "sequentialVerifyDecorator",
		"NewIncrementSequenceDecorator", "NewEVMAddressDecorator", "NewAuthzNestedMessageDecorator", "NewAnteDecorator",
	}, constructors)
}

func feeDenomAnteSuite(t *testing.T) *AnteTestSuite {
	t.Helper()
	s := new(AnteTestSuite)
	s.SetT(t)
	s.SetupTest(true)
	s.Ctx = s.Ctx.WithIsCheckTx(true).WithTxIndex(0)
	params := s.App.EvmKeeper.GetParams(s.Ctx)
	params.FeeTokenEnabled = true
	params.AllowedFeeDenoms = []evmtypes.AllowedFeeDenom{{Denom: "usid", Rate: sdk.NewDec(3_000_000), RateUpdateHeight: 1}}
	s.App.EvmKeeper.SetParams(s.Ctx, params)
	s.App.ParamsKeeper.SetFeesParams(s.Ctx, paramtypes.FeesParams{})
	s.FundAcc(s.testAcc, sdk.NewCoins(sdk.NewInt64Coin("usid", 100)))
	s.txBuilder = s.clientCtx.TxConfig.NewTxBuilder()
	s.txBuilder.SetGasLimit(1_000_000)
	return s
}

func feeDenomSignedTx(t *testing.T, s *AnteTestSuite) xauthsigning.Tx {
	t.Helper()
	account := s.App.AccountKeeper.GetAccount(s.Ctx, s.testAcc)
	tx, err := s.CreateTestTx([]cryptotypes.PrivKey{s.testAccPriv}, []uint64{account.GetAccountNumber()}, []uint64{account.GetSequence()}, s.Ctx.ChainID())
	require.NoError(t, err)
	return tx
}

func TestFeeDenomAssembledDeductionAndPriority(t *testing.T) {
	s := feeDenomAnteSuite(t)
	require.NoError(t, s.txBuilder.SetMsgs(&banktypes.MsgSend{FromAddress: s.testAcc.String(), ToAddress: s.testAcc.String(), Amount: sdk.NewCoins(sdk.NewInt64Coin("uhpx", 1))}))
	s.txBuilder.SetFeeAmount(sdk.NewCoins(sdk.NewInt64Coin("usid", 30)))
	result, err := s.anteHandler(s.Ctx, feeDenomSignedTx(t, s), false)
	require.NoError(t, err)
	require.Equal(t, sdk.NewInt(70), s.App.BankKeeper.GetBalance(result, s.testAcc, "usid").Amount)
	require.Equal(t, int64(antedecorators.MaxPriority), result.Priority())
}

func TestFeeDenomGaslessAndOracleOrdering(t *testing.T) {
	for _, enabled := range []bool{false, true} {
		s := feeDenomAnteSuite(t)
		s.App.EvmKeeper.Paramstore.Set(s.Ctx, evmtypes.KeyFeeTokenEnabled, enabled)
		validator := s.SetupValidator(stakingtypes.Bonded)
		s.App.OracleKeeper.SetFeederDelegation(s.Ctx, validator, s.testAcc)
		vote := &oracletypes.MsgAggregateExchangeRateVote{Feeder: s.testAcc.String(), Validator: validator.String(), ExchangeRates: "1uhpx"}
		require.NoError(t, s.txBuilder.SetMsgs(vote))
		s.txBuilder.SetFeeAmount(sdk.NewCoins(sdk.NewInt64Coin("usid", 30)))
		s.txBuilder.SetGasLimit(0)
		transaction := feeDenomSignedTx(t, s)
		ctx, _ := s.Ctx.CacheContext()
		result, err := s.anteHandler(ctx, transaction, false)
		require.NoError(t, err)
		require.Equal(t, sdk.NewInt(100), s.App.BankKeeper.GetBalance(result, s.testAcc, "usid").Amount)
		require.Equal(t, int64(antedecorators.OraclePriority), result.Priority())
		require.Zero(t, result.GasMeter().GasConsumed())

		ctx, _ = s.Ctx.CacheContext()
		result, err = s.anteHandler(ctx.WithIsCheckTx(false), transaction, false)
		require.NoError(t, err)
		remaining := int64(100)
		if enabled {
			remaining = 70
		}
		require.Equal(t, sdk.NewInt(remaining), s.App.BankKeeper.GetBalance(result, s.testAcc, "usid").Amount)
		require.Equal(t, int64(antedecorators.OraclePriority), result.Priority())

		s.txBuilder.SetGasLimit(1_000_000)
		require.NoError(t, s.txBuilder.SetMsgs(vote, &banktypes.MsgSend{FromAddress: s.testAcc.String(), ToAddress: s.testAcc.String(), Amount: sdk.NewCoins(sdk.NewInt64Coin("uhpx", 1))}))
		ctx, _ = s.Ctx.CacheContext()
		result, err = s.anteHandler(ctx, feeDenomSignedTx(t, s), false)
		require.ErrorContains(t, err, "oracle votes cannot be in the same tx")
		require.Equal(t, sdk.NewInt(remaining), s.App.BankKeeper.GetBalance(result, s.testAcc, "usid").Amount)
	}
}

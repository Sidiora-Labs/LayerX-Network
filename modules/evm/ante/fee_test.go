package ante_test

import (
	"encoding/hex"
	"math"
	"math/big"
	"testing"
	"time"

	"github.com/ethereum/go-ethereum/common"
	ethtypes "github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/sidiora-labs/paxeer-network/modules/evm/ante"
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types/ethtx"
	node "github.com/sidiora-labs/paxeer-network/node"
	"github.com/sidiora-labs/paxeer-network/node/antedecorators"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func TestEVMFeeCheckDecorator(t *testing.T) {
	k := &testkeeper.EVMTestApp.EvmKeeper
	ctx := testkeeper.EVMTestApp.GetContextForDeliverTx([]byte{}).WithBlockTime(time.Now())
	upgradeKeeper := &testkeeper.EVMTestApp.UpgradeKeeper
	handler := ante.NewEVMFeeCheckDecorator(k, upgradeKeeper)
	privKey := testkeeper.MockPrivateKey()
	testPrivHex := hex.EncodeToString(privKey.Bytes())
	key, _ := crypto.HexToECDSA(testPrivHex)
	to := new(common.Address)
	copy(to[:], []byte("0x1234567890abcdef1234567890abcdef12345678"))
	chainID := k.ChainID(ctx)
	txData := ethtypes.DynamicFeeTx{
		Nonce:     0,
		GasFeeCap: big.NewInt(10000000000000),
		Gas:       1000,
		To:        to,
		Value:     big.NewInt(1000000000000000),
		Data:      []byte("abc"),
		ChainID:   chainID,
	}
	chainCfg := types.DefaultChainConfig()
	ethCfg := chainCfg.EthereumConfig(chainID)
	blockNum := big.NewInt(ctx.BlockHeight())
	signer := ethtypes.MakeSigner(ethCfg, blockNum, uint64(ctx.BlockTime().Unix()))
	tx, err := ethtypes.SignTx(ethtypes.NewTx(&txData), signer, key)
	require.Nil(t, err)
	typedTx, err := ethtx.NewDynamicFeeTx(tx)
	require.Nil(t, err)
	msg, err := types.NewMsgEVMTransaction(typedTx)
	require.Nil(t, err)

	preprocessor := ante.NewEVMPreprocessDecorator(k, k.AccountKeeper())
	ctx, err = preprocessor.AnteHandle(ctx, mockTx{msgs: []sdk.Msg{msg}}, false, func(ctx sdk.Context, _ sdk.Tx, _ bool) (sdk.Context, error) {
		return ctx, nil
	})
	require.Nil(t, err)

	// should return error because gas fee cap is too low
	_, err = handler.AnteHandle(ctx, mockTx{msgs: []sdk.Msg{msg}}, false, func(ctx sdk.Context, _ sdk.Tx, _ bool) (sdk.Context, error) {
		return ctx, nil
	})
	require.NotNil(t, err)

	txData.GasFeeCap = k.GetMinimumFeePerGas(ctx).TruncateInt().BigInt()
	tx, err = ethtypes.SignTx(ethtypes.NewTx(&txData), signer, key)
	require.Nil(t, err)
	typedTx, err = ethtx.NewDynamicFeeTx(tx)
	require.Nil(t, err)
	msg, err = types.NewMsgEVMTransaction(typedTx)
	require.Nil(t, err)

	// should return error because the sender does not have enough funds
	ctx, err = preprocessor.AnteHandle(ctx, mockTx{msgs: []sdk.Msg{msg}}, false, func(ctx sdk.Context, _ sdk.Tx, _ bool) (sdk.Context, error) {
		return ctx, nil
	})
	require.Nil(t, err)
	_, err = handler.AnteHandle(ctx, mockTx{msgs: []sdk.Msg{msg}}, false, func(ctx sdk.Context, _ sdk.Tx, _ bool) (sdk.Context, error) {
		return ctx, nil
	})
	require.NotNil(t, err)

	amt := typedTx.Cost()
	coinsAmt := sdk.NewCoins(sdk.NewCoin(k.GetBaseDenom(ctx), sdk.NewIntFromBigInt(amt).Quo(sdk.NewIntFromBigInt(state.UhpxToSweiMultiplier)).Add(sdk.OneInt())))
	k.BankKeeper().MintCoins(ctx, types.ModuleName, coinsAmt)
	paxAddr := sdk.AccAddress(msg.Derived.SenderPaxAddr)
	k.BankKeeper().SendCoinsFromModuleToAccount(ctx, types.ModuleName, paxAddr, coinsAmt)

	// should succeed now that the sender has enough funds
	ctx, err = preprocessor.AnteHandle(ctx, mockTx{msgs: []sdk.Msg{msg}}, false, func(ctx sdk.Context, _ sdk.Tx, _ bool) (sdk.Context, error) {
		return ctx, nil
	})
	require.Nil(t, err)
	_, err = handler.AnteHandle(ctx, mockTx{msgs: []sdk.Msg{msg}}, false, func(ctx sdk.Context, _ sdk.Tx, _ bool) (sdk.Context, error) {
		return ctx, nil
	})
	require.Nil(t, err)

	// should fail because of minimum fee
	txData.GasFeeCap = big.NewInt(0)
	tx, err = ethtypes.SignTx(ethtypes.NewTx(&txData), signer, key)
	require.Nil(t, err)
	typedTx, err = ethtx.NewDynamicFeeTx(tx)
	require.Nil(t, err)
	msg, err = types.NewMsgEVMTransaction(typedTx)
	require.Nil(t, err)
	ctx, err = preprocessor.AnteHandle(ctx, mockTx{msgs: []sdk.Msg{msg}}, false, func(ctx sdk.Context, _ sdk.Tx, _ bool) (sdk.Context, error) {
		return ctx, nil
	})
	require.Nil(t, err)
	_, err = handler.AnteHandle(ctx, mockTx{msgs: []sdk.Msg{msg}}, false, func(ctx sdk.Context, _ sdk.Tx, _ bool) (sdk.Context, error) {
		return ctx, nil
	})
	require.NotNil(t, err)

	// should fail because of negative gas tip cap
	txData.GasTipCap = big.NewInt(-1)
	txData.GasFeeCap = big.NewInt(10000000000000)
	tx, err = ethtypes.SignTx(ethtypes.NewTx(&txData), signer, key)
	require.Nil(t, err)
	typedTx = newDynamicFeeTxWithoutValidation(tx)
	msg, err = types.NewMsgEVMTransaction(typedTx)
	require.Nil(t, err)
	ctx, err = preprocessor.AnteHandle(ctx, mockTx{msgs: []sdk.Msg{msg}}, false, func(ctx sdk.Context, _ sdk.Tx, _ bool) (sdk.Context, error) {
		return ctx, nil
	})
	require.Nil(t, err)
	_, err = handler.AnteHandle(ctx, mockTx{msgs: []sdk.Msg{msg}}, false, func(ctx sdk.Context, _ sdk.Tx, _ bool) (sdk.Context, error) {
		return ctx, nil
	})
	require.NotNil(t, err)
	require.Contains(t, err.Error(), "gas fee cap cannot be negative")
}

func TestCalculatePriorityScenarios(t *testing.T) {
	k := &testkeeper.EVMTestApp.EvmKeeper
	ctx := testkeeper.EVMTestApp.GetContextForDeliverTx([]byte{}).WithBlockTime(time.Now())
	upgradeKeeper := &testkeeper.EVMTestApp.UpgradeKeeper
	decorator := ante.NewEVMFeeCheckDecorator(k, upgradeKeeper)

	_1gwei := big.NewInt(100000000000)
	_1_1gwei := big.NewInt(1100000000000)
	_2gwei := big.NewInt(200000000000)
	maxInt := big.NewInt(math.MaxInt64)
	maxPriority := big.NewInt(antedecorators.MaxPriority)

	scenarios := []struct {
		name             string
		txData           ethtypes.TxData
		expectedPriority *big.Int
	}{
		{
			name: "DynamicFeeTx with tip",
			txData: &ethtypes.DynamicFeeTx{
				GasFeeCap: _1gwei,
				GasTipCap: _1gwei,
				Value:     _1gwei,
			},
			expectedPriority: _1gwei,
		},
		{
			name: "DynamicFeeTx with higher gas fee cap and gas tip cap",
			txData: &ethtypes.DynamicFeeTx{
				GasFeeCap: _1_1gwei,
				GasTipCap: _1_1gwei,
				Value:     _1gwei,
			},
			expectedPriority: _1_1gwei,
		},
		{
			name: "DynamicFeeTx value does not change priority",
			txData: &ethtypes.DynamicFeeTx{
				GasFeeCap: _1gwei,
				GasTipCap: _1gwei,
				Value:     _2gwei,
			},
			expectedPriority: _1gwei,
		},
		{
			name: "DynamicFeeTx with no tip",
			txData: &ethtypes.DynamicFeeTx{
				GasFeeCap: _1gwei,
				GasTipCap: big.NewInt(0),
				Value:     _1gwei,
			},
			expectedPriority: big.NewInt(0), // if you don't tip, you get lowest priority
		},
		{
			name: "DynamicFeeTx with a non-multiple of 10 tip",
			txData: &ethtypes.DynamicFeeTx{
				GasFeeCap: big.NewInt(1000000000000000),
				GasTipCap: big.NewInt(9999999999999),
				Value:     big.NewInt(1000000000),
			},
			expectedPriority: big.NewInt(9999999999999),
		},
		{
			name: "DynamicFeeTx test overflow",
			txData: &ethtypes.DynamicFeeTx{
				GasFeeCap: new(big.Int).Add(maxInt, big.NewInt(1)),
				GasTipCap: new(big.Int).Add(maxInt, big.NewInt(1)),
				Value:     big.NewInt(1000000000),
			},
			expectedPriority: maxPriority,
		},
		{
			name: "LegacyTx has priority with gas price",
			txData: &ethtypes.LegacyTx{
				GasPrice: _1gwei,
				Value:    _1gwei,
			},
			expectedPriority: _1gwei,
		},
		{
			name: "LegacyTx has zero priority with zero gas price",
			txData: &ethtypes.LegacyTx{
				GasPrice: big.NewInt(0),
				Value:    _1gwei,
			},
			expectedPriority: big.NewInt(0),
		},
		{
			name: "LegacyTx with a non-multiple of 10 gas price",
			txData: &ethtypes.LegacyTx{
				GasPrice: big.NewInt(9999999999999),
				Value:    big.NewInt(1000000000000000),
			},
			expectedPriority: big.NewInt(9999999999999),
		},
	}

	// Run each scenario
	for _, s := range scenarios {
		t.Run(s.name, func(t *testing.T) {
			tx := ethtypes.NewTx(s.txData)
			txData, err := ethtx.NewTxDataFromTx(tx)
			require.NoError(t, err)
			priority := decorator.CalculatePriority(ctx, txData)

			if s.expectedPriority != nil {
				// Check the returned value
				if priority.Cmp(s.expectedPriority) != 0 {
					t.Errorf("Expected priority %v, but got %v", s.expectedPriority, priority)
				}
			}
		})
	}
}

func newDynamicFeeTxWithoutValidation(tx *ethtypes.Transaction) *ethtx.DynamicFeeTx {
	txData := &ethtx.DynamicFeeTx{
		Nonce:    tx.Nonce(),
		Data:     tx.Data(),
		GasLimit: tx.Gas(),
	}

	v, r, s := tx.RawSignatureValues()
	ethtx.SetConvertIfPresent(tx.To(), func(to *common.Address) string { return to.Hex() }, txData.SetTo)
	ethtx.SetConvertIfPresent(tx.Value(), sdk.NewIntFromBigInt, txData.SetAmount)
	ethtx.SetConvertIfPresent(tx.GasFeeCap(), sdk.NewIntFromBigInt, txData.SetGasFeeCap)
	ethtx.SetConvertIfPresent(tx.GasTipCap(), sdk.NewIntFromBigInt, txData.SetGasTipCap)
	al := tx.AccessList()
	ethtx.SetConvertIfPresent(&al, ethtx.NewAccessList, txData.SetAccesses)

	txData.SetSignatureValues(tx.ChainId(), v, r, s)
	return txData
}

func TestFeeTokenAnte(t *testing.T) {
	app := node.Setup(t, false, true, false)
	k := &app.EvmKeeper
	var priorities []int64
	for _, tc := range []struct {
		name                   string
		preference             string
		enabled                bool
		tokenBalance           int64
		nativeBalance          int64
		missing, stale, future bool
		value                  int64
		wantError              error
	}{
		{name: "Sidiora", preference: "usid", enabled: true, tokenBalance: 1_000_000},
		{name: "insufficient Sidiora", preference: "usid", enabled: true, tokenBalance: 1, wantError: sdkerrors.ErrInsufficientFunds},
		{name: "missing rate", preference: "usid", enabled: true, tokenBalance: 1_000_000, missing: true, wantError: keeper.ErrFeeTokenRateUnavailable},
		{name: "stale rate", preference: "usid", enabled: true, tokenBalance: 1_000_000, stale: true, wantError: keeper.ErrFeeTokenRateStale},
		{name: "future rate", preference: "usid", enabled: true, tokenBalance: 1_000_000, future: true, wantError: keeper.ErrFeeTokenRateInvalid},
		{name: "disallowed denom", preference: "uother", enabled: true, tokenBalance: 1_000_000, wantError: keeper.ErrFeeTokenDenomNotAllowed},
		{name: "switch off", preference: "usid", nativeBalance: 1_000_000, tokenBalance: 1_000_000},
		{name: "no preference", enabled: true, nativeBalance: 1_000_000, tokenBalance: 1_000_000},
		{name: "network preference", preference: "uhpx", enabled: true, nativeBalance: 1_000_000, tokenBalance: 1_000_000},
		{name: "value requires network coin", preference: "usid", enabled: true, tokenBalance: 1_000_000, value: 1, wantError: sdkerrors.ErrInsufficientFunds},
	} {
		t.Run(tc.name, func(t *testing.T) {
			ctx, _ := app.GetContextForDeliverTx(nil).WithBlockHeight(100).CacheContext()
			params := types.DefaultParams()
			params.FeeTokenEnabled = tc.enabled
			height := int64(100)
			if tc.stale {
				height = 0
				params.MaxFeeTokenRateAge = 10
			}
			if tc.future {
				height = 101
			}
			params.AllowedFeeDenoms = []types.AllowedFeeDenom{{Denom: "usid", Rate: sdk.NewDec(types.InitialSidioraBaseUnitsPerPax), RateUpdateHeight: height}}
			if tc.missing {
				params.AllowedFeeDenoms = nil
			}
			k.SetParams(ctx, params)
			key, err := crypto.GenerateKey()
			require.NoError(t, err)
			to := common.HexToAddress("0x4567")
			signer := ethtypes.LatestSignerForChainID(k.ChainID(ctx))
			tx, err := ethtypes.SignTx(ethtypes.NewTx(&ethtypes.LegacyTx{Gas: 100_000, GasPrice: big.NewInt(1_000_000_000_000), To: &to, Value: big.NewInt(tc.value)}), signer, key)
			require.NoError(t, err)
			data, err := ethtx.NewLegacyTx(tx)
			require.NoError(t, err)
			msg, err := types.NewMsgEVMTransaction(data)
			require.NoError(t, err)
			require.NoError(t, ante.Preprocess(ctx, msg, k.ChainID(ctx), false))
			payer := k.GetPaxAddressOrDefault(ctx, msg.Derived.SenderEVMAddr)
			if tc.preference != "" {
				ctx.KVStore(app.GetKey(types.StoreKey)).Set(types.AccountFeeDenomKey(msg.Derived.SenderEVMAddr), []byte(tc.preference))
			}
			coins := sdk.NewCoins(sdk.NewInt64Coin("usid", tc.tokenBalance), sdk.NewInt64Coin("uhpx", tc.nativeBalance))
			require.NoError(t, k.BankKeeper().MintCoins(ctx, types.ModuleName, coins))
			require.NoError(t, k.BankKeeper().SendCoinsFromModuleToAccount(ctx, types.ModuleName, payer, coins))
			nativeBefore := k.GetBalance(ctx, payer)
			builder := app.GetTxConfig().NewTxBuilder()
			require.NoError(t, builder.SetMsgs(msg))
			result, err := ante.NewEVMFeeCheckDecorator(k, &app.UpgradeKeeper).AnteHandle(ctx, builder.GetTx(), false, func(ctx sdk.Context, _ sdk.Tx, _ bool) (sdk.Context, error) { return ctx, nil })
			if tc.wantError != nil {
				require.ErrorIs(t, err, tc.wantError)
				if tc.name == "insufficient Sidiora" || tc.name == "disallowed denom" {
					require.Contains(t, err.Error(), tc.preference)
				}
				require.Equal(t, sdk.NewInt(tc.tokenBalance), k.BankKeeper().GetBalance(ctx, payer, "usid").Amount)
				return
			}
			require.NoError(t, err)
			priorities = append(priorities, result.Priority())
			charge, err := k.GetAnteFeeTokenCharge(ctx, tx.Hash())
			require.NoError(t, err)
			if tc.preference == "usid" && tc.enabled {
				require.Equal(t, sdk.NewInt(688_600), k.BankKeeper().GetBalance(ctx, payer, "usid").Amount)
				require.Equal(t, nativeBefore, k.GetBalance(ctx, payer))
				require.NotNil(t, charge)
				require.Equal(t, "usid", charge.Denom)
				surplus, err := k.GetAnteSurplusSum(ctx)
				require.NoError(t, err)
				require.True(t, surplus.IsZero())
				k.Paramstore.Set(ctx, types.KeyAllowedFeeDenoms, []types.AllowedFeeDenom{{Denom: "usid", Rate: sdk.NewDec(6_228_000), RateUpdateHeight: 100}})
				response, err := keeper.NewMsgServerImpl(k).EVMTransaction(sdk.WrapSDKContext(ctx), msg)
				require.NoError(t, err)
				require.Empty(t, response.VmError)
				require.Equal(t, uint64(21_000), response.GasUsed)
				require.Equal(t, sdk.NewInt(934_606), k.BankKeeper().GetBalance(ctx, payer, "usid").Amount)
				require.Equal(t, nativeBefore, k.GetBalance(ctx, payer))
			} else {
				require.Nil(t, charge)
				require.Equal(t, sdk.NewInt(tc.tokenBalance), k.BankKeeper().GetBalance(ctx, payer, "usid").Amount)
				require.Equal(t, new(big.Int).Sub(nativeBefore, big.NewInt(100_000_000_000_000_000)), k.GetBalance(ctx, payer))
			}
		})
	}
	require.Len(t, priorities, 4)
	for _, priority := range priorities {
		require.Equal(t, priorities[0], priority)
	}
}

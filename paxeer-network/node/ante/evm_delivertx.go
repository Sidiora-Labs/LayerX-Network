package ante

import (
	"math/big"

	"github.com/ethereum/go-ethereum/common"
	ethtypes "github.com/ethereum/go-ethereum/core/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/derived"
	evmkeeper "github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	evmtypes "github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types/ethtx"
	"github.com/sidiora-labs/paxeer-network/sdk/client"
	"github.com/sidiora-labs/paxeer-network/sdk/crypto/keys/secp256k1"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
	upgradekeeper "github.com/sidiora-labs/paxeer-network/sdk/x/upgrade/keeper"
)

func EvmDeliverTxAnte(
	ctx sdk.Context,
	txConfig client.TxConfig,
	tx sdk.Tx,
	upgradeKeeper *upgradekeeper.Keeper,
	ek *evmkeeper.Keeper,
) (returnCtx sdk.Context, returnErr error) {
	ctx = ctx.WithDeliverTxCallback(func(sdk.Context) {})
	chainID := ek.ChainID(ctx)
	if err := EvmStatelessChecks(ctx, tx, chainID); err != nil {
		return ctx, err
	}
	msg := tx.GetMsgs()[0].(*evmtypes.MsgEVMTransaction)
	txData, _ := evmtypes.UnpackTxData(msg.Data) // cached and validated
	ctx = ctx.WithGasMeter(sdk.NewInfiniteGasMeterWithMultiplier(ctx))
	if atx, ok := txData.(*ethtx.AssociateTx); ok {
		return HandleAssociateTx(ctx, ek, atx, false)
	}
	ethereumData, ok := txData.(ethtx.EthereumTxData)
	if !ok {
		return ctx, sdkerrors.Wrap(sdkerrors.ErrInvalidRequest, "unsupported EVM transaction envelope")
	}
	etx := ethtypes.NewTx(ethereumData.AsEthereumData())
	evmAddr, paxAddr, version, err := EvmDeliverHandleSignatures(ctx, ek, ethereumData, chainID, msg)
	if err != nil {
		return ctx, err
	}
	ctx = DecorateNonceCallback(ctx, ek, evmAddr, etx.Nonce())
	if err := EvmDeliverChargeFees(ctx, ek, upgradeKeeper, ethereumData, etx, msg, version, evmAddr); err != nil {
		return ctx, err
	}
	return DecorateContext(ctx, ek, tx, ethereumData, etx, evmAddr, paxAddr), nil
}

func EvmDeliverHandleSignatures(ctx sdk.Context, ek *evmkeeper.Keeper, txData ethtx.EthereumTxData, chainID *big.Int, msg *evmtypes.MsgEVMTransaction) (common.Address, sdk.AccAddress, derived.SignerVersion, error) {
	if msg.Derived != nil {
		if msg.Derived.PubKey == nil {
			return common.Address{}, nil, 0, sdkerrors.ErrInvalidPubKey
		}
		evmAddr := msg.Derived.SenderEVMAddr
		paxAddr := msg.Derived.SenderPaxAddr
		version := msg.Derived.Version
		if err := AssociateAddress(ctx, ek, evmAddr, paxAddr, msg.Derived.PubKey); err != nil {
			return evmAddr, paxAddr, version, err
		}
		if ek.EthReplayConfig.Enabled {
			ek.PrepareReplayedAddr(ctx, evmAddr)
		}
		return evmAddr, paxAddr, version, nil
	}

	evmAddr, paxAddr, paxPubkey, version, err := CheckAndDecodeSignature(ctx, txData, chainID, ek.EthBlockTestConfig.Enabled)
	if err != nil {
		return evmAddr, paxAddr, version, err
	}
	if err := AssociateAddress(ctx, ek, evmAddr, paxAddr, paxPubkey); err != nil {
		return evmAddr, paxAddr, version, err
	}
	if ek.EthReplayConfig.Enabled {
		ek.PrepareReplayedAddr(ctx, evmAddr)
	}
	msg.Derived = &derived.Derived{
		SenderEVMAddr: evmAddr,
		SenderPaxAddr: paxAddr,
		PubKey:        &secp256k1.PubKey{Key: paxPubkey.Bytes()},
		Version:       version,
		IsAssociate:   false,
	}
	return evmAddr, paxAddr, version, nil
}

func EvmDeliverChargeFees(ctx sdk.Context, ek *evmkeeper.Keeper, upgradeKeeper *upgradekeeper.Keeper, txData ethtx.EthereumTxData, etx *ethtypes.Transaction, msg *evmtypes.MsgEVMTransaction, version derived.SignerVersion, evmAddr common.Address) error {
	stateDB, err := EvmCheckAndChargeFees(ctx, evmAddr, ek, upgradeKeeper, txData, etx, msg, version, true)
	if err != nil {
		return err
	}
	surplus, err := stateDB.Finalize()
	if err != nil {
		return err
	}
	return ek.AddAnteSurplus(ctx, etx.Hash(), surplus)
}

func DecorateNonceCallback(ctx sdk.Context, ek *evmkeeper.Keeper, evmAddr common.Address, txNonce uint64) sdk.Context {
	if ek.EthReplayConfig.Enabled || ek.EthBlockTestConfig.Enabled {
		return ctx
	}
	startingNonce := ek.GetNonce(ctx, evmAddr)
	if startingNonce != txNonce {
		return ctx
	}
	return ctx.WithDeliverTxCallback(func(callCtx sdk.Context) {
		// bump nonce if it is for some reason not incremented (e.g. ante failure)
		if ek.GetNonce(callCtx, evmAddr) == startingNonce {
			ek.SetNonce(callCtx, evmAddr, startingNonce+1)
			ek.SetNonceBumped(callCtx)
		}
	})
}

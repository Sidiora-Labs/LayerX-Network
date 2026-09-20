package keeper

import (
	"encoding/binary"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
)

func (k *Keeper) GetDepositNonce(ctx sdk.Context, depositor common.Address, assetID [32]byte) uint64 {
	bz := k.store(ctx).Get(types.DepositNonceKey(depositor, assetID))
	if bz == nil {
		return 0
	}
	return binary.BigEndian.Uint64(bz)
}

func (k *Keeper) setDepositNonce(ctx sdk.Context, depositor common.Address, assetID [32]byte, nonce uint64) {
	k.store(ctx).Set(types.DepositNonceKey(depositor, assetID), binary.BigEndian.AppendUint64(nil, nonce))
}

func (k *Keeper) GetDeposit(ctx sdk.Context, depositID [32]byte) (types.Deposit, bool) {
	var deposit types.Deposit
	bz := k.store(ctx).Get(types.DepositKey(depositID))
	if bz == nil {
		return deposit, false
	}
	k.cdc.MustUnmarshal(bz, &deposit)
	return deposit, true
}

// GetDepositByIndex resolves the 1-based global deposit counter.
func (k *Keeper) GetDepositByIndex(ctx sdk.Context, index uint64) (types.Deposit, bool) {
	bz := k.store(ctx).Get(types.DepositIndexKey(index))
	if len(bz) != 32 {
		return types.Deposit{}, false
	}
	var depositID [32]byte
	copy(depositID[:], bz)
	return k.GetDeposit(ctx, depositID)
}

func (k *Keeper) setDeposit(ctx sdk.Context, deposit types.Deposit) {
	depositID, _ := types.ParseHash32(deposit.DepositId)
	k.store(ctx).Set(types.DepositKey(depositID), k.cdc.MustMarshal(&deposit))
	k.store(ctx).Set(types.DepositIndexKey(deposit.Index), depositID[:])
}

func (k *Keeper) IterateDeposits(ctx sdk.Context, visit func(types.Deposit) bool) {
	k.iterate(ctx, types.DepositIndexPrefix, func(value []byte) bool {
		var depositID [32]byte
		copy(depositID[:], value)
		deposit, _ := k.GetDeposit(ctx, depositID)
		return visit(deposit)
	})
}

// Deposit moves amount of the asset's denom from the payer's bank account
// into the custody module account and records the deposit under LayerXVault's
// exact depositId formula. beneficiary is the 32-byte LayerX account
// identifier, opaque to Paxeer, exactly as the vault carried it.
func (k *Keeper) Deposit(ctx sdk.Context, payer common.Address, payerAccount sdk.AccAddress,
	assetID, beneficiary [32]byte, amount sdk.Int) (types.Deposit, error) {
	asset, found := k.GetAsset(ctx, assetID)
	if !found {
		return types.Deposit{}, types.ErrUnknownAsset
	}
	if !asset.Enabled || asset.Paused {
		return types.Deposit{}, types.ErrAssetDisabled
	}
	minimum, _ := asset.Minimum()
	if beneficiary == ([32]byte{}) || !amount.IsPositive() || amount.LT(minimum) || amount.BigInt().BitLen() > 128 {
		return types.Deposit{}, sdkerrors.Wrap(types.ErrInvalidDeposit, "beneficiary or amount")
	}
	custodied, released, pending := k.totals(ctx, assetID)
	custodied = custodied.Add(amount)
	if limit, capped, _ := asset.Cap(); capped && custodied.GT(limit) {
		return types.Deposit{}, sdkerrors.Wrap(types.ErrInvalidDeposit, "custody cap exceeded")
	}
	cached, write := ctx.CacheContext()
	if err := k.bankKeeper.SendCoinsFromAccountToModule(cached, payerAccount, types.ModuleName,
		sdk.NewCoins(sdk.NewCoin(asset.Denom, amount))); err != nil {
		return types.Deposit{}, err
	}
	nonce := k.GetDepositNonce(cached, payer, assetID) + 1
	depositID := types.DepositID(k.evmKeeper.ChainID(cached), payer, assetID, beneficiary, amount.BigInt(), nonce)
	if _, exists := k.GetDeposit(cached, depositID); exists {
		return types.Deposit{}, sdkerrors.Wrap(types.ErrInvalidDeposit, "deposit already recorded")
	}
	index := k.GetDepositCount(cached) + 1
	deposit := types.Deposit{
		DepositId:   types.Hash32(depositID),
		Index:       index,
		Depositor:   types.Address(payer),
		Beneficiary: types.Hash32(beneficiary),
		AssetId:     asset.AssetId,
		Denom:       asset.Denom,
		Amount:      amount.String(),
		Nonce:       nonce,
		Height:      ctx.BlockHeight(),
	}
	k.setDeposit(cached, deposit)
	k.setDepositNonce(cached, payer, assetID, nonce)
	k.setDepositCount(cached, index)
	k.setTotals(cached, assetID, custodied, released, pending)
	if err := cached.EventManager().EmitTypedEvent(&types.EventCustodyDeposit{
		DepositId: deposit.DepositId, AssetId: deposit.AssetId, Payer: deposit.Depositor,
		Beneficiary: deposit.Beneficiary, Amount: deposit.Amount, Nonce: nonce, Index: index, Denom: asset.Denom,
	}); err != nil {
		return types.Deposit{}, err
	}
	write()
	ctx.EventManager().EmitEvents(cached.EventManager().Events())
	return deposit, nil
}

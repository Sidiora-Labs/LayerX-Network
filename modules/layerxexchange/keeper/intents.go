package keeper

import (
	"github.com/ethereum/go-ethereum/common"
	custodytypes "github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	"github.com/sidiora-labs/paxeer-network/modules/layerxexchange/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
)

// atomically runs a state transition on a branch that is committed, with its
// events, only when the whole transition succeeds.
func atomically[T any](ctx sdk.Context, run func(sdk.Context) (T, error)) (T, error) {
	cached, write := ctx.CacheContext()
	out, err := run(cached)
	if err != nil {
		var zero T
		return zero, err
	}
	write()
	ctx.EventManager().EmitEvents(cached.EventManager().Events())
	return out, nil
}

// layerxAmount accepts a positive amount that fits the LayerX u128.
func layerxAmount(amount sdk.Int) bool {
	return !amount.IsNil() && amount.IsPositive() && amount.BigInt().BitLen() <= 128
}

// record assigns the owner's next nonce and its intent identifier and stores
// the pending intent.
func (k *Keeper) record(ctx sdk.Context, owner common.Address, intent types.Intent) (types.Intent, error) {
	if owner == (common.Address{}) {
		return types.Intent{}, sdkerrors.Wrap(types.ErrInvalidIntent, "owner")
	}
	nonce := k.GetOwnerNonce(ctx, owner) + 1
	intentID := types.IntentID(k.evmKeeper.ChainID(ctx), owner, intent.Kind, nonce)
	if _, exists := k.GetIntent(ctx, intentID); exists {
		return types.Intent{}, types.ErrIntentExists
	}
	intent.IntentId = custodytypes.Hash32(intentID)
	intent.Status = types.IntentStatus_INTENT_STATUS_PENDING
	intent.Owner = custodytypes.Address(owner)
	intent.Nonce = nonce
	intent.Height = ctx.BlockHeight()
	k.setIntent(ctx, intent)
	k.setOwnerNonce(ctx, owner, nonce)
	k.setIntentCount(ctx, k.GetIntentCount(ctx)+1)
	return intent, nil
}

// DepositMargin moves amount of the asset's denom from the owner's bank
// account into the layerxcustody module account as a custody deposit for the
// LayerX account, and records the deposit intent that tells the router the
// credit is margin. The asset must margin an enabled market.
func (k *Keeper) DepositMargin(ctx sdk.Context, owner common.Address, ownerAccount sdk.AccAddress,
	assetID, account [32]byte, amount sdk.Int) (types.Intent, error) {
	if !k.GetParams(ctx).MarginAsset(custodytypes.Hash32(assetID)) {
		return types.Intent{}, types.ErrNotMarginAsset
	}
	return atomically(ctx, func(ctx sdk.Context) (types.Intent, error) {
		deposit, err := k.custody.Deposit(ctx, owner, ownerAccount, assetID, account, amount)
		if err != nil {
			return types.Intent{}, err
		}
		intent, err := k.record(ctx, owner, types.Intent{Kind: types.IntentKind_INTENT_KIND_DEPOSIT,
			Account: deposit.Beneficiary, AssetId: deposit.AssetId, Denom: deposit.Denom, Amount: deposit.Amount,
			DepositId: deposit.DepositId})
		if err != nil {
			return types.Intent{}, err
		}
		return intent, ctx.EventManager().EmitTypedEvent(&types.EventMarginDeposited{IntentId: intent.IntentId,
			Owner: intent.Owner, Account: intent.Account, AssetId: intent.AssetId, Denom: intent.Denom,
			Amount: intent.Amount, DepositId: intent.DepositId, Nonce: intent.Nonce})
	})
}

// WithdrawMargin records a request that LayerX move margin of the account
// back to its withdrawable balance. No coin moves on Paxeer: the release is a
// proof-carrying layerxcustody withdrawal of the resulting LayerX receipt.
func (k *Keeper) WithdrawMargin(ctx sdk.Context, owner common.Address, account, assetID [32]byte,
	amount sdk.Int) (types.Intent, error) {
	asset, found := k.custody.GetAsset(ctx, assetID)
	if !found || !k.GetParams(ctx).MarginAsset(asset.AssetId) {
		return types.Intent{}, types.ErrNotMarginAsset
	}
	if account == ([32]byte{}) || !layerxAmount(amount) {
		return types.Intent{}, sdkerrors.Wrap(types.ErrInvalidIntent, "account or amount")
	}
	return atomically(ctx, func(ctx sdk.Context) (types.Intent, error) {
		intent, err := k.record(ctx, owner, types.Intent{Kind: types.IntentKind_INTENT_KIND_WITHDRAW,
			Account: custodytypes.Hash32(account), AssetId: asset.AssetId, Denom: asset.Denom, Amount: amount.String()})
		if err != nil {
			return types.Intent{}, err
		}
		return intent, ctx.EventManager().EmitTypedEvent(&types.EventMarginWithdrawalRequested{
			IntentId: intent.IntentId, Owner: intent.Owner, Account: intent.Account, AssetId: intent.AssetId,
			Amount: intent.Amount, Nonce: intent.Nonce})
	})
}

// PlaceOrder records an order intent for an enabled market. Paxeer checks
// only the shape of the order; LayerX matches it.
func (k *Keeper) PlaceOrder(ctx sdk.Context, owner common.Address, marketID [32]byte, side uint8,
	price, quantity sdk.Int, timeInForce uint8) (types.Intent, error) {
	market, found := k.GetParams(ctx).Market(custodytypes.Hash32(marketID))
	if !found || !market.Enabled {
		return types.Intent{}, types.ErrUnknownMarket
	}
	if side != types.SideBuy && side != types.SideSell {
		return types.Intent{}, sdkerrors.Wrap(types.ErrInvalidIntent, "side")
	}
	if timeInForce > types.TimeInForcePostOnly {
		return types.Intent{}, sdkerrors.Wrap(types.ErrInvalidIntent, "time in force")
	}
	if !layerxAmount(price) || !layerxAmount(quantity) {
		return types.Intent{}, sdkerrors.Wrap(types.ErrInvalidIntent, "price or quantity")
	}
	return atomically(ctx, func(ctx sdk.Context) (types.Intent, error) {
		intent, err := k.record(ctx, owner, types.Intent{Kind: types.IntentKind_INTENT_KIND_PLACE,
			MarketId: market.MarketId, AssetId: market.MarginAssetId, Side: uint32(side), Price: price.String(),
			Quantity: quantity.String(), TimeInForce: uint32(timeInForce)})
		if err != nil {
			return types.Intent{}, err
		}
		return intent, ctx.EventManager().EmitTypedEvent(&types.EventOrderPlaced{IntentId: intent.IntentId,
			Owner: intent.Owner, MarketId: intent.MarketId, Side: intent.Side, Price: intent.Price,
			Quantity: intent.Quantity, TimeInForce: intent.TimeInForce, Nonce: intent.Nonce})
	})
}

// CancelOrder records a request to cancel a LayerX order.
func (k *Keeper) CancelOrder(ctx sdk.Context, owner common.Address, orderID [32]byte) (types.Intent, error) {
	if orderID == ([32]byte{}) {
		return types.Intent{}, sdkerrors.Wrap(types.ErrInvalidIntent, "order id")
	}
	return atomically(ctx, func(ctx sdk.Context) (types.Intent, error) {
		intent, err := k.record(ctx, owner, types.Intent{Kind: types.IntentKind_INTENT_KIND_CANCEL,
			OrderId: custodytypes.Hash32(orderID)})
		if err != nil {
			return types.Intent{}, err
		}
		return intent, ctx.EventManager().EmitTypedEvent(&types.EventOrderCancelRequested{IntentId: intent.IntentId,
			Owner: intent.Owner, OrderId: intent.OrderId, Nonce: intent.Nonce})
	})
}

// RequestSettlement records a request to settle a LayerX position.
func (k *Keeper) RequestSettlement(ctx sdk.Context, owner common.Address, positionID [32]byte) (types.Intent, error) {
	if positionID == ([32]byte{}) {
		return types.Intent{}, sdkerrors.Wrap(types.ErrInvalidIntent, "position id")
	}
	return atomically(ctx, func(ctx sdk.Context) (types.Intent, error) {
		intent, err := k.record(ctx, owner, types.Intent{Kind: types.IntentKind_INTENT_KIND_SETTLE,
			PositionId: custodytypes.Hash32(positionID)})
		if err != nil {
			return types.Intent{}, err
		}
		return intent, ctx.EventManager().EmitTypedEvent(&types.EventSettlementRequested{IntentId: intent.IntentId,
			Owner: intent.Owner, PositionId: intent.PositionId, Nonce: intent.Nonce})
	})
}

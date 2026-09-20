package keeper

import (
	"encoding/binary"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	"github.com/sidiora-labs/paxeer-network/sdk/codec"
	"github.com/sidiora-labs/paxeer-network/sdk/store/prefix"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
)

// Keeper owns LayerX custody: the module account holds every custodied coin
// and only proof-carrying claims move coins out of it.
type Keeper struct {
	storeKey sdk.StoreKey
	cdc      codec.BinaryCodec

	accountKeeper types.AccountKeeper
	bankKeeper    types.BankKeeper
	evmKeeper     types.EVMKeeper

	anchor types.AnchorReader
}

func NewKeeper(cdc codec.BinaryCodec, storeKey sdk.StoreKey, accountKeeper types.AccountKeeper,
	bankKeeper types.BankKeeper, evmKeeper types.EVMKeeper) *Keeper {
	return &Keeper{storeKey: storeKey, cdc: cdc, accountKeeper: accountKeeper, bankKeeper: bankKeeper, evmKeeper: evmKeeper}
}

// SetAnchorReader replaces the authority-set anchor material with the anchor
// module's keeper. It is called once during app wiring.
func (k *Keeper) SetAnchorReader(anchor types.AnchorReader) { k.anchor = anchor }

// Anchor returns the LayerX trust material custody verifies against.
func (k *Keeper) Anchor() types.AnchorReader {
	if k.anchor != nil {
		return k.anchor
	}
	return authorityAnchor{k}
}

func (k *Keeper) store(ctx sdk.Context) sdk.KVStore { return ctx.KVStore(k.storeKey) }

// ModuleAddress is the bank account that holds custody.
func (k *Keeper) ModuleAddress() sdk.AccAddress {
	return k.accountKeeper.GetModuleAddress(types.ModuleName)
}

func (k *Keeper) GetParams(ctx sdk.Context) types.Params {
	var params types.Params
	bz := k.store(ctx).Get(types.ParamsKey)
	if bz == nil {
		return types.DefaultParams()
	}
	k.cdc.MustUnmarshal(bz, &params)
	return params
}

func (k *Keeper) SetParams(ctx sdk.Context, params types.Params) error {
	if err := params.Validate(); err != nil {
		return err
	}
	k.store(ctx).Set(types.ParamsKey, k.cdc.MustMarshal(&params))
	return nil
}

// Authority is the bech32 account allowed to administer custody.
func (k *Keeper) Authority(ctx sdk.Context) string {
	if authority := k.GetParams(ctx).Authority; authority != "" {
		return authority
	}
	return authtypes.NewModuleAddress(govtypes.ModuleName).String()
}

func (k *Keeper) requireAuthority(ctx sdk.Context, signer string) error {
	if signer != k.Authority(ctx) {
		return sdkerrors.Wrapf(sdkerrors.ErrUnauthorized, "%s is not the custody authority", signer)
	}
	return nil
}

func (k *Keeper) GetEmergency(ctx sdk.Context) bool {
	return k.store(ctx).Has(types.EmergencyKey)
}

func (k *Keeper) SetEmergency(ctx sdk.Context, enabled bool) error {
	if enabled {
		k.store(ctx).Set(types.EmergencyKey, []byte{1})
	} else {
		k.store(ctx).Delete(types.EmergencyKey)
	}
	return ctx.EventManager().EmitTypedEvent(&types.EventEmergencySet{Enabled: enabled})
}

func (k *Keeper) GetDepositCount(ctx sdk.Context) uint64 {
	bz := k.store(ctx).Get(types.DepositCountKey)
	if bz == nil {
		return 0
	}
	return binary.BigEndian.Uint64(bz)
}

func (k *Keeper) setDepositCount(ctx sdk.Context, count uint64) {
	k.store(ctx).Set(types.DepositCountKey, binary.BigEndian.AppendUint64(nil, count))
}

// SetAsset creates or replaces one asset mapping and its pointer index. A
// denom or pointer already bound to another asset is refused, and the denom
// of an asset that holds custody or pending claims cannot change.
func (k *Keeper) SetAsset(ctx sdk.Context, asset types.AssetMapping) error {
	if err := asset.Validate(); err != nil {
		return err
	}
	assetID, _ := types.ParseHash32(asset.AssetId)
	var conflict error
	k.IterateAssets(ctx, func(other types.AssetMapping) bool {
		if other.AssetId != asset.AssetId && (other.Denom == asset.Denom || (asset.Pointer != "" && other.Pointer == asset.Pointer)) {
			conflict = sdkerrors.Wrapf(types.ErrInvalidAsset, "denom or pointer already maps asset %s", other.AssetId)
		}
		return conflict != nil
	})
	if conflict != nil {
		return conflict
	}
	if previous, found := k.GetAsset(ctx, assetID); found {
		totals := k.GetTotals(ctx, assetID)
		if previous.Denom != asset.Denom && (totals.Custodied != "0" || totals.Pending != "0") {
			return sdkerrors.Wrap(types.ErrInvalidAsset, "denom of an asset with custody cannot change")
		}
		if previous.Pointer != "" {
			pointer, _ := types.ParseAddress(previous.Pointer)
			k.store(ctx).Delete(types.AssetPointerKey(pointer))
		}
	}
	if asset.Pointer != "" {
		pointer, _ := types.ParseAddress(asset.Pointer)
		asset.Pointer = types.Address(pointer)
		k.store(ctx).Set(types.AssetPointerKey(pointer), assetID[:])
	}
	k.store(ctx).Set(types.AssetKey(assetID), k.cdc.MustMarshal(&asset))
	return nil
}

func (k *Keeper) GetAsset(ctx sdk.Context, assetID [32]byte) (types.AssetMapping, bool) {
	var asset types.AssetMapping
	bz := k.store(ctx).Get(types.AssetKey(assetID))
	if bz == nil {
		return asset, false
	}
	k.cdc.MustUnmarshal(bz, &asset)
	return asset, true
}

func (k *Keeper) GetAssetByPointer(ctx sdk.Context, pointer common.Address) (types.AssetMapping, bool) {
	bz := k.store(ctx).Get(types.AssetPointerKey(pointer))
	if len(bz) != 32 {
		return types.AssetMapping{}, false
	}
	var assetID [32]byte
	copy(assetID[:], bz)
	return k.GetAsset(ctx, assetID)
}

// GetAssetByDenom scans the (small, authority-set) asset map.
func (k *Keeper) GetAssetByDenom(ctx sdk.Context, denom string) (types.AssetMapping, bool) {
	var out types.AssetMapping
	found := false
	k.IterateAssets(ctx, func(asset types.AssetMapping) bool {
		if asset.Denom == denom {
			out, found = asset, true
		}
		return found
	})
	return out, found
}

func (k *Keeper) iterate(ctx sdk.Context, keyPrefix []byte, visit func(value []byte) bool) {
	iterator := prefix.NewStore(k.store(ctx), keyPrefix).Iterator(nil, nil)
	defer func() { _ = iterator.Close() }()
	for ; iterator.Valid(); iterator.Next() {
		if visit(iterator.Value()) {
			return
		}
	}
}

func (k *Keeper) IterateAssets(ctx sdk.Context, visit func(types.AssetMapping) bool) {
	k.iterate(ctx, types.AssetPrefix, func(value []byte) bool {
		var asset types.AssetMapping
		k.cdc.MustUnmarshal(value, &asset)
		return visit(asset)
	})
}

// GetTotals returns the custody accounting of an asset, zero when untouched.
func (k *Keeper) GetTotals(ctx sdk.Context, assetID [32]byte) types.AssetTotals {
	totals := types.AssetTotals{AssetId: types.Hash32(assetID), Custodied: "0", Released: "0", Pending: "0"}
	if bz := k.store(ctx).Get(types.AssetTotalsKey(assetID)); bz != nil {
		k.cdc.MustUnmarshal(bz, &totals)
	}
	return totals
}

func (k *Keeper) setTotals(ctx sdk.Context, assetID [32]byte, custodied, released, pending sdk.Int) {
	totals := types.AssetTotals{AssetId: types.Hash32(assetID), Custodied: custodied.String(),
		Released: released.String(), Pending: pending.String()}
	k.store(ctx).Set(types.AssetTotalsKey(assetID), k.cdc.MustMarshal(&totals))
}

func (k *Keeper) totals(ctx sdk.Context, assetID [32]byte) (custodied, released, pending sdk.Int) {
	totals := k.GetTotals(ctx, assetID)
	custodied, _ = sdk.NewIntFromString(totals.Custodied)
	released, _ = sdk.NewIntFromString(totals.Released)
	pending, _ = sdk.NewIntFromString(totals.Pending)
	return custodied, released, pending
}

func (k *Keeper) IterateTotals(ctx sdk.Context, visit func(types.AssetTotals) bool) {
	k.iterate(ctx, types.AssetTotalsPrefix, func(value []byte) bool {
		var totals types.AssetTotals
		k.cdc.MustUnmarshal(value, &totals)
		return visit(totals)
	})
}

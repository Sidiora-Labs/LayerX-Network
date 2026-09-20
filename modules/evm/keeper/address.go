package keeper

import (
	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/sdk/store/prefix"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func (k *Keeper) SetAddressMapping(ctx sdk.Context, paxAddress sdk.AccAddress, evmAddress common.Address) {
	store := ctx.KVStore(k.storeKey)
	store.Set(types.EVMAddressToPaxAddressKey(evmAddress), paxAddress)
	store.Set(types.PaxAddressToEVMAddressKey(paxAddress), evmAddress[:])
	if !k.accountKeeper.HasAccount(ctx, paxAddress) {
		k.accountKeeper.SetAccount(ctx, k.accountKeeper.NewAccountWithAddress(ctx, paxAddress))
	}
	ctx.EventManager().EmitEvent(sdk.NewEvent(
		types.EventTypeAddressAssociated,
		sdk.NewAttribute(types.AttributeKeyPaxAddress, paxAddress.String()),
		sdk.NewAttribute(types.AttributeKeyEvmAddress, evmAddress.Hex()),
	))
}

func (k *Keeper) DeleteAddressMapping(ctx sdk.Context, paxAddress sdk.AccAddress, evmAddress common.Address) {
	store := ctx.KVStore(k.storeKey)
	store.Delete(types.EVMAddressToPaxAddressKey(evmAddress))
	store.Delete(types.PaxAddressToEVMAddressKey(paxAddress))
}

func (k *Keeper) GetEVMAddress(ctx sdk.Context, paxAddress sdk.AccAddress) (common.Address, bool) {
	store := ctx.KVStore(k.storeKey)
	bz := store.Get(types.PaxAddressToEVMAddressKey(paxAddress))
	addr := common.Address{}
	if bz == nil {
		return addr, false
	}
	copy(addr[:], bz)
	return addr, true
}

func (k *Keeper) GetEVMAddressOrDefault(ctx sdk.Context, paxAddress sdk.AccAddress) common.Address {
	addr, ok := k.GetEVMAddress(ctx, paxAddress)
	if ok {
		return addr
	}
	return common.BytesToAddress(paxAddress)
}

func (k *Keeper) GetPaxAddress(ctx sdk.Context, evmAddress common.Address) (sdk.AccAddress, bool) {
	store := ctx.KVStore(k.storeKey)
	bz := store.Get(types.EVMAddressToPaxAddressKey(evmAddress))
	if bz == nil {
		return []byte{}, false
	}
	return bz, true
}

func (k *Keeper) GetPaxAddressOrDefault(ctx sdk.Context, evmAddress common.Address) sdk.AccAddress {
	addr, ok := k.GetPaxAddress(ctx, evmAddress)
	if ok {
		return addr
	}
	return sdk.AccAddress(evmAddress[:])
}

func (k *Keeper) IteratePaxAddressMapping(ctx sdk.Context, cb func(evmAddr common.Address, paxAddr sdk.AccAddress) bool) {
	iter := prefix.NewStore(ctx.KVStore(k.storeKey), types.EVMAddressToPaxAddressKeyPrefix).Iterator(nil, nil)
	defer func() { _ = iter.Close() }()
	for ; iter.Valid(); iter.Next() {
		evmAddr := common.BytesToAddress(iter.Key())
		paxAddr := sdk.AccAddress(iter.Value())
		if cb(evmAddr, paxAddr) {
			break
		}
	}
}

// A sdk.AccAddress may not receive funds from bank if it's the result of direct-casting
// from an EVM address AND the originating EVM address has already been associated with
// a true (i.e. derived from the same pubkey) sdk.AccAddress.
func (k *Keeper) CanAddressReceive(ctx sdk.Context, addr sdk.AccAddress) bool {
	directCast := common.BytesToAddress(addr) // casting goes both directions since both address formats have 20 bytes
	associatedAddr, isAssociated := k.GetPaxAddress(ctx, directCast)
	// if the associated address is the cast address itself, allow the address to receive (e.g. EVM contract addresses)
	return associatedAddr.Equals(addr) || !isAssociated // this means it's either a cast address that's not associated yet, or not a cast address at all.
}

type EvmAddressHandler struct {
	evmKeeper *Keeper
}

func NewEvmAddressHandler(evmKeeper *Keeper) EvmAddressHandler {
	return EvmAddressHandler{evmKeeper: evmKeeper}
}

func (h EvmAddressHandler) GetPaxAddressFromString(ctx sdk.Context, address string) (sdk.AccAddress, error) {
	if common.IsHexAddress(address) {
		parsedAddress := common.HexToAddress(address)
		return h.evmKeeper.GetPaxAddressOrDefault(ctx, parsedAddress), nil
	}
	parsedAddress, err := sdk.AccAddressFromBech32(address)
	if err != nil {
		return nil, err
	}
	return parsedAddress, nil
}

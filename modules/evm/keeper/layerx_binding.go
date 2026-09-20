package keeper

import (
	"bytes"
	"encoding/binary"
	"math"
	"strconv"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/layerxproof/verify"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/sdk/store/prefix"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// GetLayerXDid returns the Ed25519 public key of the did:layerx identity bound
// to an EVM address.
func (k *Keeper) GetLayerXDid(ctx sdk.Context, evmAddress common.Address) ([32]byte, bool) {
	var didPublicKey [32]byte
	bz := ctx.KVStore(k.storeKey).Get(types.EVMAddressToLayerXDidKey(evmAddress))
	if len(bz) != len(didPublicKey) {
		return didPublicKey, false
	}
	copy(didPublicKey[:], bz)
	return didPublicKey, true
}

// GetEVMAddressByLayerXDid returns the EVM address bound to a did:layerx
// public key.
func (k *Keeper) GetEVMAddressByLayerXDid(ctx sdk.Context, didPublicKey [32]byte) (common.Address, bool) {
	bz := ctx.KVStore(k.storeKey).Get(types.LayerXDidToEVMAddressKey(didPublicKey))
	if len(bz) != common.AddressLength {
		return common.Address{}, false
	}
	return common.BytesToAddress(bz), true
}

// GetLayerXBindNonce returns the binding counter of an EVM address: the nonce
// the next bind signature must cover.
func (k *Keeper) GetLayerXBindNonce(ctx sdk.Context, evmAddress common.Address) uint64 {
	bz := ctx.KVStore(k.storeKey).Get(types.LayerXBindNonceKey(evmAddress))
	if len(bz) != 8 {
		return 0
	}
	return binary.BigEndian.Uint64(bz)
}

func (k *Keeper) setLayerXBindNonce(ctx sdk.Context, evmAddress common.Address, nonce uint64) {
	ctx.KVStore(k.storeKey).Set(types.LayerXBindNonceKey(evmAddress), binary.BigEndian.AppendUint64(nil, nonce))
}

// BindLayerX binds evmAddress, whose consent is the transaction itself, to the
// did:layerx identity of didPublicKey, whose consent is a strict Ed25519
// signature over LayerXBindMessage(chain id, evmAddress, current nonce). It
// returns the nonce the signature consumed.
func (k *Keeper) BindLayerX(ctx sdk.Context, evmAddress common.Address, didPublicKey [32]byte, signature [64]byte) (uint64, error) {
	if !verify.PublicKeyIsCanonical(didPublicKey) {
		return 0, types.ErrLayerXNonCanonicalKey
	}
	if _, bound := k.GetLayerXDid(ctx, evmAddress); bound {
		return 0, types.ErrLayerXAddressBound
	}
	if _, bound := k.GetEVMAddressByLayerXDid(ctx, didPublicKey); bound {
		return 0, types.ErrLayerXDidBound
	}
	nonce := k.GetLayerXBindNonce(ctx, evmAddress)
	if nonce == math.MaxUint64 {
		return 0, types.ErrLayerXNonceExhausted
	}
	message, err := types.LayerXBindMessage(k.ChainID(ctx), evmAddress, nonce)
	if err != nil {
		return 0, err
	}
	if err := verify.Ed25519(didPublicKey, signature, message); err != nil {
		return 0, types.ErrLayerXSignature
	}
	store := ctx.KVStore(k.storeKey)
	store.Set(types.EVMAddressToLayerXDidKey(evmAddress), didPublicKey[:])
	store.Set(types.LayerXDidToEVMAddressKey(didPublicKey), evmAddress[:])
	k.setLayerXBindNonce(ctx, evmAddress, nonce+1)
	ctx.EventManager().EmitEvent(sdk.NewEvent(
		types.EventTypeLayerXBound,
		sdk.NewAttribute(types.AttributeKeyEvmAddress, evmAddress.Hex()),
		sdk.NewAttribute(types.AttributeKeyLayerXDid, types.LayerXDid(didPublicKey)),
		sdk.NewAttribute(types.AttributeKeyLayerXNonce, strconv.FormatUint(nonce, 10)),
	))
	return nonce, nil
}

// UnbindLayerX removes the binding of evmAddress on that address's authority
// alone and consumes a nonce, so no earlier bind signature can be replayed.
func (k *Keeper) UnbindLayerX(ctx sdk.Context, evmAddress common.Address) ([32]byte, uint64, error) {
	didPublicKey, bound := k.GetLayerXDid(ctx, evmAddress)
	if !bound {
		return didPublicKey, 0, types.ErrLayerXNotBound
	}
	nonce := k.GetLayerXBindNonce(ctx, evmAddress)
	if nonce == math.MaxUint64 {
		return didPublicKey, 0, types.ErrLayerXNonceExhausted
	}
	store := ctx.KVStore(k.storeKey)
	store.Delete(types.EVMAddressToLayerXDidKey(evmAddress))
	store.Delete(types.LayerXDidToEVMAddressKey(didPublicKey))
	k.setLayerXBindNonce(ctx, evmAddress, nonce+1)
	ctx.EventManager().EmitEvent(sdk.NewEvent(
		types.EventTypeLayerXUnbound,
		sdk.NewAttribute(types.AttributeKeyEvmAddress, evmAddress.Hex()),
		sdk.NewAttribute(types.AttributeKeyLayerXDid, types.LayerXDid(didPublicKey)),
		sdk.NewAttribute(types.AttributeKeyLayerXNonce, strconv.FormatUint(nonce, 10)),
	))
	return didPublicKey, nonce, nil
}

// IterateLayerXBindings visits every binding in EVM address order.
func (k *Keeper) IterateLayerXBindings(ctx sdk.Context, cb func(evmAddress common.Address, didPublicKey [32]byte) bool) {
	iter := prefix.NewStore(ctx.KVStore(k.storeKey), types.EVMAddressToLayerXDidKeyPrefix).Iterator(nil, nil)
	defer func() { _ = iter.Close() }()
	for ; iter.Valid(); iter.Next() {
		var didPublicKey [32]byte
		copy(didPublicKey[:], iter.Value())
		if cb(common.BytesToAddress(iter.Key()), didPublicKey) {
			break
		}
	}
}

// ValidateLayerXBindings checks that the binding table is a bijection of
// well-formed entries: both directions agree, every key is canonical and every
// nonce is eight bytes. It guards a table loaded from genesis.
func (k *Keeper) ValidateLayerXBindings(ctx sdk.Context) error {
	store := ctx.KVStore(k.storeKey)
	forward := prefix.NewStore(store, types.EVMAddressToLayerXDidKeyPrefix).Iterator(nil, nil)
	defer func() { _ = forward.Close() }()
	for ; forward.Valid(); forward.Next() {
		if len(forward.Key()) != common.AddressLength || len(forward.Value()) != 32 {
			return types.ErrLayerXGenesisEntrySize
		}
		var didPublicKey [32]byte
		copy(didPublicKey[:], forward.Value())
		if !verify.PublicKeyIsCanonical(didPublicKey) {
			return types.ErrLayerXNonCanonicalKey
		}
		if !bytes.Equal(store.Get(types.LayerXDidToEVMAddressKey(didPublicKey)), forward.Key()) {
			return types.ErrLayerXGenesisBinding
		}
	}
	reverse := prefix.NewStore(store, types.LayerXDidToEVMAddressKeyPrefix).Iterator(nil, nil)
	defer func() { _ = reverse.Close() }()
	for ; reverse.Valid(); reverse.Next() {
		if len(reverse.Key()) != 32 || len(reverse.Value()) != common.AddressLength {
			return types.ErrLayerXGenesisEntrySize
		}
		if !bytes.Equal(store.Get(types.EVMAddressToLayerXDidKey(common.BytesToAddress(reverse.Value()))), reverse.Key()) {
			return types.ErrLayerXGenesisBinding
		}
	}
	nonces := prefix.NewStore(store, types.LayerXBindNonceKeyPrefix).Iterator(nil, nil)
	defer func() { _ = nonces.Close() }()
	for ; nonces.Valid(); nonces.Next() {
		if len(nonces.Key()) != common.AddressLength || len(nonces.Value()) != 8 {
			return types.ErrLayerXGenesisEntrySize
		}
	}
	return nil
}

package keeper_test

import (
	"crypto/ed25519"
	"crypto/rand"
	"math/big"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/layerxproof/testvectors"
	"github.com/sidiora-labs/paxeer-network/modules/evm"
	evmkeeper "github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

const layerXTestChainID = "layerx-binding-test"

func layerXBindingContext() (*evmkeeper.Keeper, sdk.Context) {
	ctx := keeper.EVMTestApp.GetContextForDeliverTx([]byte{}).WithChainID(layerXTestChainID)
	return &keeper.EVMTestApp.EvmKeeper, ctx.WithMultiStore(ctx.MultiStore().CacheMultiStore())
}

func newLayerXDidKey(t *testing.T) ([32]byte, ed25519.PrivateKey) {
	t.Helper()
	public, private, err := ed25519.GenerateKey(rand.Reader)
	require.NoError(t, err)
	var didPublicKey [32]byte
	copy(didPublicKey[:], public)
	return didPublicKey, private
}

func signLayerXBind(t *testing.T, private ed25519.PrivateKey, chainID *big.Int, evmAddress common.Address, nonce uint64) [64]byte {
	t.Helper()
	message, err := types.LayerXBindMessage(chainID, evmAddress, nonce)
	require.NoError(t, err)
	var signature [64]byte
	copy(signature[:], ed25519.Sign(private, message))
	return signature
}

func TestLayerXBind(t *testing.T) {
	k, ctx := layerXBindingContext()
	_, evmAddress := keeper.MockAddressPair()
	didPublicKey, private := newLayerXDidKey(t)

	_, bound := k.GetLayerXDid(ctx, evmAddress)
	require.False(t, bound)
	_, bound = k.GetEVMAddressByLayerXDid(ctx, didPublicKey)
	require.False(t, bound)
	require.Equal(t, uint64(0), k.GetLayerXBindNonce(ctx, evmAddress))

	ctx = ctx.WithEventManager(sdk.NewEventManager())
	nonce, err := k.BindLayerX(ctx, evmAddress, didPublicKey, signLayerXBind(t, private, k.ChainID(ctx), evmAddress, 0))
	require.NoError(t, err)
	require.Equal(t, uint64(0), nonce)

	foundKey, bound := k.GetLayerXDid(ctx, evmAddress)
	require.True(t, bound)
	require.Equal(t, didPublicKey, foundKey)
	foundAddress, bound := k.GetEVMAddressByLayerXDid(ctx, didPublicKey)
	require.True(t, bound)
	require.Equal(t, evmAddress, foundAddress)
	require.Equal(t, uint64(1), k.GetLayerXBindNonce(ctx, evmAddress))
	require.NoError(t, k.ValidateLayerXBindings(ctx))

	// The binding does not need and does not create a Paxeer association.
	_, associated := k.GetPaxAddress(ctx, evmAddress)
	require.False(t, associated)

	events := ctx.EventManager().Events()
	require.Len(t, events, 1)
	require.Equal(t, types.EventTypeLayerXBound, events[0].Type)
	attributes := map[string]string{}
	for _, attribute := range events[0].Attributes {
		attributes[string(attribute.Key)] = string(attribute.Value)
	}
	require.Equal(t, evmAddress.Hex(), attributes[types.AttributeKeyEvmAddress])
	require.Equal(t, types.LayerXDid(didPublicKey), attributes[types.AttributeKeyLayerXDid])
	require.Equal(t, "0", attributes[types.AttributeKeyLayerXNonce])
}

func TestLayerXBindReplayRefused(t *testing.T) {
	k, ctx := layerXBindingContext()
	_, evmAddress := keeper.MockAddressPair()
	didPublicKey, private := newLayerXDidKey(t)
	signature := signLayerXBind(t, private, k.ChainID(ctx), evmAddress, 0)

	_, err := k.BindLayerX(ctx, evmAddress, didPublicKey, signature)
	require.NoError(t, err)
	_, _, err = k.UnbindLayerX(ctx, evmAddress)
	require.NoError(t, err)

	// The address is free again and the DID is free again, yet the consumed
	// signature no longer covers the current nonce.
	_, err = k.BindLayerX(ctx, evmAddress, didPublicKey, signature)
	require.ErrorIs(t, err, types.ErrLayerXSignature)
	_, bound := k.GetLayerXDid(ctx, evmAddress)
	require.False(t, bound)
	require.Equal(t, uint64(2), k.GetLayerXBindNonce(ctx, evmAddress))
}

func TestLayerXBindWrongChainIDRefused(t *testing.T) {
	k, ctx := layerXBindingContext()
	_, evmAddress := keeper.MockAddressPair()
	didPublicKey, private := newLayerXDidKey(t)

	otherChain := new(big.Int).Add(k.ChainID(ctx), big.NewInt(1))
	_, err := k.BindLayerX(ctx, evmAddress, didPublicKey, signLayerXBind(t, private, otherChain, evmAddress, 0))
	require.ErrorIs(t, err, types.ErrLayerXSignature)

	// A signature for Paxeer mainnet is refused here and accepted there.
	mainnet := ctx.WithChainID("hyperpax_125-1")
	require.Equal(t, int64(125), k.ChainID(mainnet).Int64())
	signature := signLayerXBind(t, private, k.ChainID(mainnet), evmAddress, 0)
	_, err = k.BindLayerX(ctx, evmAddress, didPublicKey, signature)
	require.ErrorIs(t, err, types.ErrLayerXSignature)
	require.Equal(t, uint64(0), k.GetLayerXBindNonce(ctx, evmAddress))
	_, err = k.BindLayerX(mainnet, evmAddress, didPublicKey, signature)
	require.NoError(t, err)
}

func TestLayerXBindWrongAddressRefused(t *testing.T) {
	k, ctx := layerXBindingContext()
	_, evmAddress := keeper.MockAddressPair()
	_, thief := keeper.MockAddressPair()
	didPublicKey, private := newLayerXDidKey(t)

	signature := signLayerXBind(t, private, k.ChainID(ctx), evmAddress, 0)
	_, err := k.BindLayerX(ctx, thief, didPublicKey, signature)
	require.ErrorIs(t, err, types.ErrLayerXSignature)
	_, bound := k.GetEVMAddressByLayerXDid(ctx, didPublicKey)
	require.False(t, bound)

	// A signature by a different key over the right message is refused too.
	_, otherPrivate := newLayerXDidKey(t)
	_, err = k.BindLayerX(ctx, evmAddress, didPublicKey, signLayerXBind(t, otherPrivate, k.ChainID(ctx), evmAddress, 0))
	require.ErrorIs(t, err, types.ErrLayerXSignature)
}

func TestLayerXBindNonCanonicalKeyRefused(t *testing.T) {
	k, ctx := layerXBindingContext()
	_, evmAddress := keeper.MockAddressPair()
	_, private := newLayerXDidKey(t)

	// y = p, y = p + 1 (aliases of 0 and 1), y = 0, y = 1 and y = 2^255 - 1.
	fieldPrime := [32]byte{
		0xed, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
		0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f,
	}
	aliasOfOne := fieldPrime
	aliasOfOne[0] = 0xee
	allOnes := fieldPrime
	allOnes[0] = 0xff
	for _, didPublicKey := range [][32]byte{fieldPrime, aliasOfOne, {}, {0x01}, allOnes} {
		_, err := k.BindLayerX(ctx, evmAddress, didPublicKey, signLayerXBind(t, private, k.ChainID(ctx), evmAddress, 0))
		require.ErrorIs(t, err, types.ErrLayerXNonCanonicalKey)
		_, bound := k.GetEVMAddressByLayerXDid(ctx, didPublicKey)
		require.False(t, bound)
	}
	require.Equal(t, uint64(0), k.GetLayerXBindNonce(ctx, evmAddress))
}

func TestLayerXBindHighSRefused(t *testing.T) {
	k, ctx := layerXBindingContext()
	_, evmAddress := keeper.MockAddressPair()
	didPublicKey, private := newLayerXDidKey(t)
	signature := signLayerXBind(t, private, k.ChainID(ctx), evmAddress, 0)

	// S + L satisfies the verification equation and is not reduced.
	groupOrder := [32]byte{
		0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
		0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10,
	}
	malleable := signature
	carry := uint16(0)
	for i := 0; i < 32; i++ {
		sum := uint16(signature[32+i]) + uint16(groupOrder[i]) + carry
		malleable[32+i] = byte(sum)
		carry = sum >> 8
	}
	require.Zero(t, carry)
	_, err := k.BindLayerX(ctx, evmAddress, didPublicKey, malleable)
	require.ErrorIs(t, err, types.ErrLayerXSignature)
	_, err = k.BindLayerX(ctx, evmAddress, didPublicKey, signature)
	require.NoError(t, err)
}

func TestLayerXDoubleBindRefused(t *testing.T) {
	k, ctx := layerXBindingContext()
	_, firstAddress := keeper.MockAddressPair()
	_, secondAddress := keeper.MockAddressPair()
	firstKey, firstPrivate := newLayerXDidKey(t)
	secondKey, secondPrivate := newLayerXDidKey(t)

	_, err := k.BindLayerX(ctx, firstAddress, firstKey, signLayerXBind(t, firstPrivate, k.ChainID(ctx), firstAddress, 0))
	require.NoError(t, err)

	// The bound address cannot take a second DID, even with that DID's consent.
	_, err = k.BindLayerX(ctx, firstAddress, secondKey, signLayerXBind(t, secondPrivate, k.ChainID(ctx), firstAddress, 1))
	require.ErrorIs(t, err, types.ErrLayerXAddressBound)
	// The bound DID cannot take a second address, even with its own consent.
	_, err = k.BindLayerX(ctx, secondAddress, firstKey, signLayerXBind(t, firstPrivate, k.ChainID(ctx), secondAddress, 0))
	require.ErrorIs(t, err, types.ErrLayerXDidBound)

	foundKey, _ := k.GetLayerXDid(ctx, firstAddress)
	require.Equal(t, firstKey, foundKey)
	foundAddress, _ := k.GetEVMAddressByLayerXDid(ctx, firstKey)
	require.Equal(t, firstAddress, foundAddress)
	_, bound := k.GetLayerXDid(ctx, secondAddress)
	require.False(t, bound)
	_, bound = k.GetEVMAddressByLayerXDid(ctx, secondKey)
	require.False(t, bound)
	require.Equal(t, uint64(1), k.GetLayerXBindNonce(ctx, firstAddress))
	require.Equal(t, uint64(0), k.GetLayerXBindNonce(ctx, secondAddress))
}

func TestLayerXUnbindThenRebind(t *testing.T) {
	k, ctx := layerXBindingContext()
	_, evmAddress := keeper.MockAddressPair()
	_, otherAddress := keeper.MockAddressPair()
	firstKey, firstPrivate := newLayerXDidKey(t)
	secondKey, secondPrivate := newLayerXDidKey(t)

	_, _, err := k.UnbindLayerX(ctx, evmAddress)
	require.ErrorIs(t, err, types.ErrLayerXNotBound)
	require.Equal(t, uint64(0), k.GetLayerXBindNonce(ctx, evmAddress))

	_, err = k.BindLayerX(ctx, evmAddress, firstKey, signLayerXBind(t, firstPrivate, k.ChainID(ctx), evmAddress, 0))
	require.NoError(t, err)

	ctx = ctx.WithEventManager(sdk.NewEventManager())
	removedKey, nonce, err := k.UnbindLayerX(ctx, evmAddress)
	require.NoError(t, err)
	require.Equal(t, firstKey, removedKey)
	require.Equal(t, uint64(1), nonce)
	require.Equal(t, types.EventTypeLayerXUnbound, ctx.EventManager().Events()[0].Type)
	_, bound := k.GetLayerXDid(ctx, evmAddress)
	require.False(t, bound)
	_, bound = k.GetEVMAddressByLayerXDid(ctx, firstKey)
	require.False(t, bound)
	require.Equal(t, uint64(2), k.GetLayerXBindNonce(ctx, evmAddress))

	// The address takes a new DID at nonce 2; the released DID takes a new address.
	nonce, err = k.BindLayerX(ctx, evmAddress, secondKey, signLayerXBind(t, secondPrivate, k.ChainID(ctx), evmAddress, 2))
	require.NoError(t, err)
	require.Equal(t, uint64(2), nonce)
	_, err = k.BindLayerX(ctx, otherAddress, firstKey, signLayerXBind(t, firstPrivate, k.ChainID(ctx), otherAddress, 0))
	require.NoError(t, err)
	require.Equal(t, uint64(3), k.GetLayerXBindNonce(ctx, evmAddress))
	require.NoError(t, k.ValidateLayerXBindings(ctx))
}

func TestLayerXBindRustSignerVectors(t *testing.T) {
	fixture, err := testvectors.LoadPaxeerBind()
	require.NoError(t, err)

	mainAccountID, err := types.LayerXMainAccountID(fixture.PublicKey)
	require.NoError(t, err)
	require.Equal(t, fixture.Did, types.LayerXDid(fixture.PublicKey))
	require.Equal(t, fixture.MainAccountName, types.LayerXMainAccountName(fixture.PublicKey))
	require.Equal(t, fixture.MainAccountID, mainAccountID)

	accepted := 0
	for _, bind := range fixture.Binds {
		k, ctx := layerXBindingContext()
		if bind.ChainID == 125 {
			ctx = ctx.WithChainID("hyperpax_125-1")
		}
		require.Equal(t, bind.ChainID, k.ChainID(ctx).Uint64(), bind.Name)
		evmAddress := common.Address(bind.EVMAddress)

		message, err := types.LayerXBindMessage(k.ChainID(ctx), evmAddress, bind.Nonce)
		require.NoError(t, err)
		require.Equal(t, bind.Message, message, "%s: message bytes differ from the Rust signer's", bind.Name)

		// Reach the vector's nonce with bind and unbind pairs of another DID.
		otherKey, otherPrivate := newLayerXDidKey(t)
		for k.GetLayerXBindNonce(ctx, evmAddress) < bind.Nonce {
			nonce := k.GetLayerXBindNonce(ctx, evmAddress)
			_, err := k.BindLayerX(ctx, evmAddress, otherKey, signLayerXBind(t, otherPrivate, k.ChainID(ctx), evmAddress, nonce))
			require.NoError(t, err)
			_, _, err = k.UnbindLayerX(ctx, evmAddress)
			require.NoError(t, err)
		}
		require.Equal(t, bind.Nonce, k.GetLayerXBindNonce(ctx, evmAddress), bind.Name)

		_, err = k.BindLayerX(ctx, evmAddress, fixture.PublicKey, bind.Signature)
		if bind.Valid {
			require.NoError(t, err, bind.Name)
			foundAddress, bound := k.GetEVMAddressByLayerXDid(ctx, fixture.PublicKey)
			require.True(t, bound)
			require.Equal(t, evmAddress, foundAddress)
			accepted++
		} else {
			require.ErrorIs(t, err, types.ErrLayerXSignature, bind.Name)
		}
	}
	require.GreaterOrEqual(t, accepted, 1)
}

func TestLayerXBindingGenesisRoundTrip(t *testing.T) {
	k, origin := layerXBindingContext()
	source := origin.WithMultiStore(origin.MultiStore().CacheMultiStore())
	paxAddress, boundAddress := keeper.MockAddressPair()
	_, releasedAddress := keeper.MockAddressPair()
	boundKey, boundPrivate := newLayerXDidKey(t)
	releasedKey, releasedPrivate := newLayerXDidKey(t)

	k.SetAddressMapping(source, paxAddress, boundAddress)
	_, err := k.BindLayerX(source, boundAddress, boundKey, signLayerXBind(t, boundPrivate, k.ChainID(source), boundAddress, 0))
	require.NoError(t, err)
	staleSignature := signLayerXBind(t, releasedPrivate, k.ChainID(source), releasedAddress, 0)
	_, err = k.BindLayerX(source, releasedAddress, releasedKey, staleSignature)
	require.NoError(t, err)
	_, _, err = k.UnbindLayerX(source, releasedAddress)
	require.NoError(t, err)

	genesis := evm.ExportGenesis(source, k)
	require.NoError(t, genesis.Validate())

	// The destination never saw the bindings until the exported genesis is loaded.
	_, bound := k.GetLayerXDid(origin, boundAddress)
	require.False(t, bound)
	evm.InitGenesis(origin, k, *genesis)

	foundKey, bound := k.GetLayerXDid(origin, boundAddress)
	require.True(t, bound)
	require.Equal(t, boundKey, foundKey)
	foundAddress, bound := k.GetEVMAddressByLayerXDid(origin, boundKey)
	require.True(t, bound)
	require.Equal(t, boundAddress, foundAddress)
	require.Equal(t, uint64(1), k.GetLayerXBindNonce(origin, boundAddress))
	_, bound = k.GetLayerXDid(origin, releasedAddress)
	require.False(t, bound)
	_, bound = k.GetEVMAddressByLayerXDid(origin, releasedKey)
	require.False(t, bound)
	require.NoError(t, k.ValidateLayerXBindings(origin))

	// The nonce travelled with the export, so the consumed signature stays dead.
	require.Equal(t, uint64(2), k.GetLayerXBindNonce(origin, releasedAddress))
	_, err = k.BindLayerX(origin, releasedAddress, releasedKey, staleSignature)
	require.ErrorIs(t, err, types.ErrLayerXSignature)

	// A malformed binding entry in a genesis file is refused.
	malformed := types.DefaultGenesis()
	malformed.Serialized = append(malformed.Serialized, &types.Serialized{
		Prefix: types.EVMAddressToLayerXDidKeyPrefix,
		Key:    boundAddress[:],
		Value:  make([]byte, 32),
	})
	require.PanicsWithError(t, types.ErrLayerXNonCanonicalKey.Error(), func() {
		evm.InitGenesis(origin.WithMultiStore(origin.MultiStore().CacheMultiStore()), k, *malformed)
	})
}

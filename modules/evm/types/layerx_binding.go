package types

import (
	"bytes"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"math/big"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
	"github.com/sidiora-labs/paxeer-network/layerxproof/verify"
)

const (
	// LayerXBindDomain opens every message a LayerX DID key signs to consent to
	// a binding with an EVM address.
	LayerXBindDomain = "LX:PAXEER-BIND:v1"
	// LayerXDidPrefix precedes the 64 lowercase hex characters of the Ed25519
	// public key in a LayerX DID.
	LayerXDidPrefix = "did:layerx:"
	// LayerXBindMessageLength is the domain, the 32-byte chain id, the 20-byte
	// EVM address and the 8-byte nonce.
	LayerXBindMessageLength = len(LayerXBindDomain) + 32 + common.AddressLength + 8
)

var (
	ErrLayerXChainID          = errors.New("layerx binding: chain id does not fit 32 bytes")
	ErrLayerXSignature        = errors.New("layerx binding: signature refused")
	ErrLayerXAddressBound     = errors.New("layerx binding: evm address is already bound to a did")
	ErrLayerXDidBound         = errors.New("layerx binding: did is already bound to an evm address")
	ErrLayerXNotBound         = errors.New("layerx binding: evm address is not bound to a did")
	ErrLayerXNonceExhausted   = errors.New("layerx binding: nonce exhausted")
	ErrLayerXNonCanonicalKey  = errors.New("layerx binding: did public key is not canonical")
	ErrLayerXSignatureLength  = errors.New("layerx binding: signature must be 64 bytes")
	ErrLayerXGenesisBinding   = errors.New("layerx binding: genesis binding table is inconsistent")
	ErrLayerXGenesisEntrySize = errors.New("layerx binding: genesis entry has the wrong size")
)

// LayerXBindMessage is the exact byte string the DID key signs:
// "LX:PAXEER-BIND:v1" || chain id (uint256 big-endian) || EVM address ||
// nonce (uint64 big-endian).
func LayerXBindMessage(chainID *big.Int, evmAddress common.Address, nonce uint64) ([]byte, error) {
	if chainID == nil || chainID.Sign() < 0 || chainID.BitLen() > 256 {
		return nil, ErrLayerXChainID
	}
	message := make([]byte, 0, LayerXBindMessageLength)
	message = append(message, LayerXBindDomain...)
	var chain [32]byte
	chainID.FillBytes(chain[:])
	message = append(message, chain[:]...)
	message = append(message, evmAddress[:]...)
	message = binary.BigEndian.AppendUint64(message, nonce)
	return message, nil
}

// LayerXDid renders did:layerx:<64 lowercase hex characters>.
func LayerXDid(didPublicKey [32]byte) string {
	return LayerXDidPrefix + hex.EncodeToString(didPublicKey[:])
}

// LayerXMainAccountName is the canonical LayerX name of the DID's main account.
func LayerXMainAccountName(didPublicKey [32]byte) string {
	return "agent:" + LayerXDid(didPublicKey) + ":main"
}

// LayerXMainAccountID derives the native LayerX identifier of the DID's main
// account.
func LayerXMainAccountID(didPublicKey [32]byte) ([32]byte, error) {
	return codec.DeriveAccountID([]byte(LayerXMainAccountName(didPublicKey)))
}

// ValidateLayerXGenesisEntry refuses a serialized genesis entry under one of
// the binding prefixes that is not well formed. Entries under any other prefix
// pass. It is safe on a streamed genesis, where each direction of the table
// arrives in its own chunk.
func ValidateLayerXGenesisEntry(prefix, key, value []byte) error {
	var didPublicKey [32]byte
	if len(key) == 0 && len(prefix) > 1 {
		prefix, key = prefix[:1], prefix[1:]
	}
	switch {
	case bytes.Equal(prefix, EVMAddressToLayerXDidKeyPrefix):
		if len(key) != common.AddressLength || len(value) != len(didPublicKey) {
			return ErrLayerXGenesisEntrySize
		}
		copy(didPublicKey[:], value)
	case bytes.Equal(prefix, LayerXDidToEVMAddressKeyPrefix):
		if len(key) != len(didPublicKey) || len(value) != common.AddressLength {
			return ErrLayerXGenesisEntrySize
		}
		copy(didPublicKey[:], key)
	case bytes.Equal(prefix, LayerXBindNonceKeyPrefix):
		if len(key) != common.AddressLength || len(value) != 8 {
			return ErrLayerXGenesisEntrySize
		}
		return nil
	default:
		return nil
	}
	if !verify.PublicKeyIsCanonical(didPublicKey) {
		return ErrLayerXNonCanonicalKey
	}
	return nil
}

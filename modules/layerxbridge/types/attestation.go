package types

import (
	"bytes"
	"encoding/hex"
	"math/big"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/crypto"
	tokenfactorytypes "github.com/sidiora-labs/paxeer-network/modules/tokenfactory/types"
)

// The attestation layouts are fixed by the Ethereum PaxeerXVault
// (bridge/evm/src/BridgeAttestation.sol) and
// documented byte for byte in ATTESTATION.md.
const (
	DomainIn  = "PAXEERX_BRIDGE_IN_V1"
	DomainOut = "PAXEERX_BRIDGE_OUT_V1"

	InboundPreimageLength  = 20 + 32 + 20 + 32 + 8 + 32 + 20 + 32
	OutboundPreimageLength = 21 + 32 + 20 + 32 + 8 + 20 + 20 + 32

	// SignatureLength is a secp256k1 signature r || s || v.
	SignatureLength = 65

	// SubdenomPrefix starts every bridged subdenom.
	SubdenomPrefix = "lxb"
)

// BridgeIn is one attested BridgeDeposit log of a remote vault.
type BridgeIn struct {
	ChainID   uint64
	Vault     Address20
	TxHash    Hash32
	LogIndex  uint64
	Recipient Hash32
	Asset     Address20
	Amount    *big.Int
}

func (b BridgeIn) Nullifier() Nullifier {
	return Nullifier{ChainID: b.ChainID, TxHash: b.TxHash, LogIndex: b.LogIndex}
}

// BridgeOut is one Paxeer burn that authorises a release on the remote vault.
type BridgeOut struct {
	ChainID      uint64
	Vault        Address20
	PaxeerTxHash Hash32
	PaxeerNonce  uint64
	Recipient    Address20
	Asset        Address20
	Amount       *big.Int
}

func uint256(value *big.Int) []byte { return common.LeftPadBytes(value.Bytes(), 32) }

// InboundPreimage is "PAXEERX_BRIDGE_IN_V1" || uint256 chainId || address
// vault || bytes32 txHash || uint64 logIndex || bytes32 recipient || address
// asset || uint256 amount, integers big-endian: 196 bytes.
func InboundPreimage(in BridgeIn) []byte {
	out := make([]byte, 0, InboundPreimageLength)
	out = append(out, DomainIn...)
	out = append(out, uint256(new(big.Int).SetUint64(in.ChainID))...)
	out = append(out, in.Vault[:]...)
	out = append(out, in.TxHash[:]...)
	out = append(out, u64(in.LogIndex)...)
	out = append(out, in.Recipient[:]...)
	out = append(out, in.Asset[:]...)
	out = append(out, uint256(in.Amount)...)
	return out
}

// InboundDigest is keccak256(InboundPreimage): what attestors sign for a
// bridgeIn and PaxeerXVault.depositDigest returns.
func InboundDigest(in BridgeIn) Hash32 { return Hash32(crypto.Keccak256Hash(InboundPreimage(in))) }

// OutboundPreimage is "PAXEERX_BRIDGE_OUT_V1" || uint256 chainId || address
// vault || bytes32 paxeerTxHash || uint64 paxeerNonce || address recipient ||
// address asset || uint256 amount, integers big-endian: 185 bytes.
func OutboundPreimage(out BridgeOut) []byte {
	buf := make([]byte, 0, OutboundPreimageLength)
	buf = append(buf, DomainOut...)
	buf = append(buf, uint256(new(big.Int).SetUint64(out.ChainID))...)
	buf = append(buf, out.Vault[:]...)
	buf = append(buf, out.PaxeerTxHash[:]...)
	buf = append(buf, u64(out.PaxeerNonce)...)
	buf = append(buf, out.Recipient[:]...)
	buf = append(buf, out.Asset[:]...)
	buf = append(buf, uint256(out.Amount)...)
	return buf
}

// OutboundDigest is keccak256(OutboundPreimage): what attestors sign for a
// PaxeerXVault.release.
func OutboundDigest(out BridgeOut) Hash32 { return Hash32(crypto.Keccak256Hash(OutboundPreimage(out))) }

// RecipientAddress is the Paxeer EVM address a bytes32 paxeerRecipient
// names: the low 20 bytes, with the high 12 bytes zero
// (bytes32(uint256(uint160(address)))).
func RecipientAddress(recipient Hash32) (Address20, bool) {
	if !bytes.Equal(recipient[:12], make([]byte, 12)) {
		return Address20{}, false
	}
	var address Address20
	copy(address[:], recipient[12:])
	return address, address != (Address20{})
}

var secp256k1HalfOrder = new(big.Int).Rsh(crypto.S256().Params().N, 1)

// RecoverSigner returns the EVM address that signed digest, with the vault's
// rules: r || s || v, v in {27, 28}, s at most half the curve order, the raw
// digest signed without an EIP-191 prefix.
func RecoverSigner(digest Hash32, signature []byte) (Address20, error) {
	if len(signature) != SignatureLength {
		return Address20{}, ErrBadSignature.Wrapf("signature is %d bytes, want %d", len(signature), SignatureLength)
	}
	v := signature[64]
	if v != 27 && v != 28 {
		return Address20{}, ErrBadSignature.Wrap("v must be 27 or 28")
	}
	r := new(big.Int).SetBytes(signature[:32])
	s := new(big.Int).SetBytes(signature[32:64])
	if s.Cmp(secp256k1HalfOrder) > 0 || !crypto.ValidateSignatureValues(v-27, r, s, true) {
		return Address20{}, ErrBadSignature.Wrap("signature values out of range or high s")
	}
	normalized := append(append([]byte(nil), signature[:64]...), v-27)
	publicKey, err := crypto.SigToPub(digest[:], normalized)
	if err != nil {
		return Address20{}, ErrBadSignature.Wrap(err.Error())
	}
	return Address20(crypto.PubkeyToAddress(*publicKey)), nil
}

// Subdenom is "lxb" followed by the hex of the first 20 bytes of
// keccak256(uint64 chainId || address asset): 43 characters, inside the
// tokenfactory subdenom bound.
func Subdenom(chainID uint64, asset Address20) string {
	hash := crypto.Keccak256(u64(chainID), asset[:])
	return SubdenomPrefix + hex.EncodeToString(hash[:20])
}

// Denom is the module-owned tokenfactory denom of (chain, asset):
// factory/{bridge module address}/{Subdenom}.
func Denom(chainID uint64, asset Address20) string {
	denom, err := tokenfactorytypes.GetTokenDenom(ModuleAddress().String(), Subdenom(chainID, asset))
	if err != nil {
		panic(err)
	}
	return denom
}

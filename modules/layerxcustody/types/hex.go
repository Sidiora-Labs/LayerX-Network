package types

import (
	"encoding/hex"
	"strings"

	"github.com/ethereum/go-ethereum/common"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
)

// Hash32 renders a 32-byte identifier as 64 lowercase hex characters, the
// only spelling the module stores and accepts.
func Hash32(value [32]byte) string { return hex.EncodeToString(value[:]) }

// ParseHash32 accepts exactly 64 lowercase hex characters.
func ParseHash32(text string) ([32]byte, error) {
	var out [32]byte
	if len(text) != 64 || strings.ToLower(text) != text {
		return out, sdkerrors.Wrapf(ErrInvalidHex, "%q is not 64 lowercase hex characters", text)
	}
	if _, err := hex.Decode(out[:], []byte(text)); err != nil {
		return out, sdkerrors.Wrapf(ErrInvalidHex, "%q: %s", text, err)
	}
	return out, nil
}

// ParseNonZeroHash32 additionally refuses the zero identifier.
func ParseNonZeroHash32(text string) ([32]byte, error) {
	out, err := ParseHash32(text)
	if err == nil && out == ([32]byte{}) {
		return out, sdkerrors.Wrap(ErrInvalidHex, "zero identifier")
	}
	return out, err
}

// Address renders an EVM address in its EIP-55 0x form.
func Address(value common.Address) string { return value.Hex() }

// ParseAddress accepts a 0x-prefixed 20-byte EVM address.
func ParseAddress(text string) (common.Address, error) {
	if len(text) != 42 || !strings.HasPrefix(text, "0x") || !common.IsHexAddress(text) {
		return common.Address{}, sdkerrors.Wrapf(ErrInvalidHex, "%q is not an EVM address", text)
	}
	return common.HexToAddress(text), nil
}

// ParseAmount accepts a non-negative decimal integer.
func ParseAmount(text string) (sdk.Int, error) {
	value, ok := sdk.NewIntFromString(text)
	if !ok || value.IsNegative() {
		return sdk.Int{}, sdkerrors.Wrapf(ErrInvalidHex, "%q is not a non-negative amount", text)
	}
	return value, nil
}

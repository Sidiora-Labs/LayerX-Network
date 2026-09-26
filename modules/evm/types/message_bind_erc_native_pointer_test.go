package types_test

import (
	"math"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/stretchr/testify/require"

	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
)

const (
	bindTestDenom   = "factory/pax1dzfx9mk4fl9kl2mysjmtvk2xp75ljumk6nynhf/usid"
	bindTestPointer = "0x21f7b20a555199fa73A238B1a91FD0f549068fEe"
)

func validBindMsg() *types.MsgBindERCNativePointer {
	return types.NewMsgBindERCNativePointer(types.GovernanceAuthority(), bindTestDenom, common.HexToAddress(bindTestPointer), 1)
}

func TestMsgBindERCNativePointerAcceptsAWellFormedBinding(t *testing.T) {
	msg := validBindMsg()
	require.NoError(t, msg.ValidateBasic())
	require.Equal(t, types.RouterKey, msg.Route())
	require.Equal(t, types.TypeMsgBindERCNativePointer, msg.Type())
	governance, err := sdk.AccAddressFromBech32(types.GovernanceAuthority())
	require.NoError(t, err)
	require.Equal(t, []sdk.AccAddress{governance}, msg.GetSigners())
	require.NotEmpty(t, msg.GetSignBytes())
	require.Equal(t, common.HexToAddress(bindTestPointer).Hex(), msg.Pointer)
}

func TestMsgBindERCNativePointerRefusesAMalformedAuthority(t *testing.T) {
	msg := validBindMsg()
	msg.Authority = "not-an-address"
	require.ErrorContains(t, msg.ValidateBasic(), "invalid authority address")
}

func TestMsgBindERCNativePointerRefusesAnEmptyDenom(t *testing.T) {
	msg := validBindMsg()
	msg.Token = ""
	require.ErrorContains(t, msg.ValidateBasic(), "the denom is empty")
}

func TestMsgBindERCNativePointerRefusesAMalformedDenom(t *testing.T) {
	msg := validBindMsg()
	msg.Token = "1usid with spaces"
	require.ErrorContains(t, msg.ValidateBasic(), "invalid denom")
}

func TestMsgBindERCNativePointerRefusesAMalformedAddress(t *testing.T) {
	msg := validBindMsg()
	msg.Pointer = "0x21f7b20a555199fa73A238B1a91FD0f549068f"
	require.ErrorContains(t, msg.ValidateBasic(), "is not a hex-encoded address")
}

func TestMsgBindERCNativePointerRefusesTheZeroAddress(t *testing.T) {
	msg := validBindMsg()
	msg.Pointer = common.Address{}.Hex()
	require.ErrorContains(t, msg.ValidateBasic(), "the pointer is the zero address")
}

func TestMsgBindERCNativePointerRefusesAZeroVersion(t *testing.T) {
	msg := validBindMsg()
	msg.Version = 0
	require.ErrorContains(t, msg.ValidateBasic(), "the pointer version is zero")
}

func TestMsgBindERCNativePointerRefusesAVersionBeyondSixteenBits(t *testing.T) {
	msg := validBindMsg()
	msg.Version = math.MaxUint16 + 1
	require.ErrorContains(t, msg.ValidateBasic(), "exceeds 65535")
}

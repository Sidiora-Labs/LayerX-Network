package types

import (
	"math"

	"github.com/ethereum/go-ethereum/common"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
)

const TypeMsgBindERCNativePointer = "evm_bind_erc_native_pointer"

// BindERCNativePointerUpgrade is the upgrade from whose height on a native
// pointer binding executes: the upgrade that brings the Sidiora fee token.
const BindERCNativePointerUpgrade = "v6.7"

var _ sdk.Msg = &MsgBindERCNativePointer{}

// GovernanceAuthority is the governance module account, the only authority a
// native pointer binding executes for.
func GovernanceAuthority() string {
	return authtypes.NewModuleAddress(govtypes.ModuleName).String()
}

// NewMsgBindERCNativePointer binds the deployed ERC20 contract at pointer as
// the pointer of the native denom token at the given pointer version.
func NewMsgBindERCNativePointer(authority string, token string, pointer common.Address, version uint16) *MsgBindERCNativePointer {
	return &MsgBindERCNativePointer{Authority: authority, Token: token, Pointer: pointer.Hex(), Version: uint32(version)}
}

func (msg *MsgBindERCNativePointer) Route() string {
	return RouterKey
}

func (msg *MsgBindERCNativePointer) Type() string {
	return TypeMsgBindERCNativePointer
}

func (msg *MsgBindERCNativePointer) GetSigners() []sdk.AccAddress {
	authority, err := sdk.AccAddressFromBech32(msg.Authority)
	if err != nil {
		panic(err)
	}
	return []sdk.AccAddress{authority}
}

func (msg *MsgBindERCNativePointer) GetSignBytes() []byte {
	return sdk.MustSortJSON(ModuleCdc.MustMarshalJSON(msg))
}

// ValidateBasic refuses a malformed authority, an empty or malformed denom, a
// malformed or zero pointer address, and a pointer version that is zero or
// beyond the pointer registry's sixteen bits.
func (msg *MsgBindERCNativePointer) ValidateBasic() error {
	if _, err := sdk.AccAddressFromBech32(msg.Authority); err != nil {
		return sdkerrors.Wrapf(sdkerrors.ErrInvalidAddress, "invalid authority address (%s)", err)
	}
	if msg.Token == "" {
		return sdkerrors.Wrap(sdkerrors.ErrInvalidRequest, "the denom is empty")
	}
	if err := sdk.ValidateDenom(msg.Token); err != nil {
		return sdkerrors.Wrapf(sdkerrors.ErrInvalidRequest, "invalid denom %q: %s", msg.Token, err)
	}
	if !common.IsHexAddress(msg.Pointer) {
		return sdkerrors.Wrapf(sdkerrors.ErrInvalidAddress, "the pointer %q is not a hex-encoded address", msg.Pointer)
	}
	if common.HexToAddress(msg.Pointer) == (common.Address{}) {
		return sdkerrors.Wrap(sdkerrors.ErrInvalidAddress, "the pointer is the zero address")
	}
	if msg.Version == 0 {
		return sdkerrors.Wrap(sdkerrors.ErrInvalidRequest, "the pointer version is zero")
	}
	if msg.Version > math.MaxUint16 {
		return sdkerrors.Wrapf(sdkerrors.ErrInvalidRequest, "the pointer version %d exceeds %d", msg.Version, math.MaxUint16)
	}
	return nil
}

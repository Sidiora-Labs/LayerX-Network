package types

import (
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
)

const (
	TypeMsgUpdateParams = "update_params"
	TypeMsgSetMarket    = "set_market"
)

var (
	_ sdk.Msg = &MsgUpdateParams{}
	_ sdk.Msg = &MsgSetMarket{}
)

func signer(address string) []sdk.AccAddress {
	decoded, _ := sdk.AccAddressFromBech32(address)
	return []sdk.AccAddress{decoded}
}

func validSigner(address string) error {
	if _, err := sdk.AccAddressFromBech32(address); err != nil {
		return sdkerrors.Wrapf(sdkerrors.ErrInvalidAddress, "invalid signer address (%s)", err)
	}
	return nil
}

func (m MsgUpdateParams) Route() string                { return RouterKey }
func (m MsgUpdateParams) Type() string                 { return TypeMsgUpdateParams }
func (m MsgUpdateParams) GetSigners() []sdk.AccAddress { return signer(m.Authority) }
func (m MsgUpdateParams) GetSignBytes() []byte {
	return sdk.MustSortJSON(ModuleCdc.MustMarshalJSON(&m))
}
func (m MsgUpdateParams) ValidateBasic() error {
	if err := validSigner(m.Authority); err != nil {
		return err
	}
	return m.Params.Validate()
}

func (m MsgSetMarket) Route() string                { return RouterKey }
func (m MsgSetMarket) Type() string                 { return TypeMsgSetMarket }
func (m MsgSetMarket) GetSigners() []sdk.AccAddress { return signer(m.Authority) }
func (m MsgSetMarket) GetSignBytes() []byte {
	return sdk.MustSortJSON(ModuleCdc.MustMarshalJSON(&m))
}
func (m MsgSetMarket) ValidateBasic() error {
	if err := validSigner(m.Authority); err != nil {
		return err
	}
	return m.Market.Validate()
}

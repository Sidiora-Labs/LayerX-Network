package types

import (
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// The governance messages are generated from api/layerxbridge/tx.proto. Each
// is executed by the keeper only when Authority is the module authority (the
// gov module account by default), which is also its only signer.

const (
	TypeMsgRegisterChain = "register_chain"
	TypeMsgSetAttestors  = "set_attestors"
	TypeMsgSetCap        = "set_cap"
	TypeMsgPause         = "pause"
	TypeMsgUnpause       = "unpause"
)

var (
	_ sdk.Msg = &MsgRegisterChain{}
	_ sdk.Msg = &MsgSetAttestors{}
	_ sdk.Msg = &MsgSetCap{}
	_ sdk.Msg = &MsgPause{}
	_ sdk.Msg = &MsgUnpause{}
)

// authoritySigner is the one signer of every governance message. An authority
// that is not a bech32 account yields no signer, and ValidateBasic refuses it
// before any signature is checked.
func authoritySigner(authority string) []sdk.AccAddress {
	account, err := sdk.AccAddressFromBech32(authority)
	if err != nil {
		return []sdk.AccAddress{}
	}
	return []sdk.AccAddress{account}
}

func signBytes(msg sdk.Msg) []byte { return sdk.MustSortJSON(ModuleCdc.MustMarshalJSON(msg)) }

func (m MsgRegisterChain) Route() string                { return RouterKey }
func (m MsgRegisterChain) Type() string                 { return TypeMsgRegisterChain }
func (m MsgRegisterChain) GetSigners() []sdk.AccAddress { return authoritySigner(m.Authority) }
func (m MsgRegisterChain) GetSignBytes() []byte         { return signBytes(&m) }
func (m MsgRegisterChain) String() string               { return jsonString(m) }

func (m MsgSetAttestors) Route() string                { return RouterKey }
func (m MsgSetAttestors) Type() string                 { return TypeMsgSetAttestors }
func (m MsgSetAttestors) GetSigners() []sdk.AccAddress { return authoritySigner(m.Authority) }
func (m MsgSetAttestors) GetSignBytes() []byte         { return signBytes(&m) }
func (m MsgSetAttestors) String() string               { return jsonString(m) }

func (m MsgSetCap) Route() string                { return RouterKey }
func (m MsgSetCap) Type() string                 { return TypeMsgSetCap }
func (m MsgSetCap) GetSigners() []sdk.AccAddress { return authoritySigner(m.Authority) }
func (m MsgSetCap) GetSignBytes() []byte         { return signBytes(&m) }
func (m MsgSetCap) String() string               { return jsonString(m) }

func (m MsgPause) Route() string                { return RouterKey }
func (m MsgPause) Type() string                 { return TypeMsgPause }
func (m MsgPause) GetSigners() []sdk.AccAddress { return authoritySigner(m.Authority) }
func (m MsgPause) GetSignBytes() []byte         { return signBytes(&m) }
func (m MsgPause) String() string               { return jsonString(m) }

func (m MsgUnpause) Route() string                { return RouterKey }
func (m MsgUnpause) Type() string                 { return TypeMsgUnpause }
func (m MsgUnpause) GetSigners() []sdk.AccAddress { return authoritySigner(m.Authority) }
func (m MsgUnpause) GetSignBytes() []byte         { return signBytes(&m) }
func (m MsgUnpause) String() string               { return jsonString(m) }

func validateAuthority(authority string) error {
	if _, err := sdk.AccAddressFromBech32(authority); err != nil {
		return ErrUnauthorized.Wrapf("authority: %v", err)
	}
	return nil
}

func (m MsgRegisterChain) ValidateBasic() error {
	if err := validateAuthority(m.Authority); err != nil {
		return err
	}
	return m.Chain.Validate()
}

func (m MsgSetAttestors) ValidateBasic() error {
	if err := validateAuthority(m.Authority); err != nil {
		return err
	}
	return m.Set.Validate()
}

func (m MsgSetCap) ValidateBasic() error {
	if err := validateAuthority(m.Authority); err != nil {
		return err
	}
	if m.ChainID == 0 {
		return ErrInvalidCap.Wrap("chain id is zero")
	}
	return Cap{Denom: Denom(m.ChainID, m.Asset), MaxInFlight: m.MaxInFlight, MaxPerTx: m.MaxPerTx}.Validate()
}

func (m MsgPause) ValidateBasic() error { return validateAuthority(m.Authority) }

func (m MsgUnpause) ValidateBasic() error { return validateAuthority(m.Authority) }

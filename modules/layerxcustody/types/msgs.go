package types

import (
	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
)

const (
	TypeMsgUpdateParams       = "update_params"
	TypeMsgSetAsset           = "set_asset"
	TypeMsgRegisterCheckpoint = "register_checkpoint"
	TypeMsgSetEmergency       = "set_emergency"
	TypeMsgCancelClaim        = "cancel_claim"
	TypeMsgRequestWithdrawal  = "request_withdrawal"
	TypeMsgFinaliseWithdrawal = "finalise_withdrawal"
	TypeMsgRequestForcedExit  = "request_forced_exit"
	TypeMsgExecuteForcedExit  = "execute_forced_exit"

	// MaxEvidenceBytes bounds every evidence field of a message.
	MaxEvidenceBytes = 64 * 1024
)

var (
	_ sdk.Msg = &MsgUpdateParams{}
	_ sdk.Msg = &MsgSetAsset{}
	_ sdk.Msg = &MsgRegisterCheckpoint{}
	_ sdk.Msg = &MsgSetEmergency{}
	_ sdk.Msg = &MsgCancelClaim{}
	_ sdk.Msg = &MsgRequestWithdrawal{}
	_ sdk.Msg = &MsgFinaliseWithdrawal{}
	_ sdk.Msg = &MsgRequestForcedExit{}
	_ sdk.Msg = &MsgExecuteForcedExit{}
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

func validWithdrawalEvidence(receipt, proof, header, headerSignature []byte) error {
	if len(receipt) == 0 || len(receipt) > MaxEvidenceBytes || len(proof) == 0 || len(proof) > MaxEvidenceBytes ||
		len(header) != codec.BatchHeaderBytes || len(headerSignature) != 64 {
		return sdkerrors.Wrap(ErrInvalidClaim, "withdrawal evidence is malformed")
	}
	return nil
}

func validExitEvidence(witness []byte, account, assetID, recipient string, signature []byte) error {
	if len(witness) == 0 || len(witness) > MaxEvidenceBytes || len(signature) != 64 {
		return sdkerrors.Wrap(ErrInvalidExit, "forced exit evidence is malformed")
	}
	if _, err := ParseNonZeroHash32(account); err != nil {
		return err
	}
	if _, err := ParseNonZeroHash32(assetID); err != nil {
		return err
	}
	_, err := ParseAddress(recipient)
	return err
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

func (m MsgSetAsset) Route() string                { return RouterKey }
func (m MsgSetAsset) Type() string                 { return TypeMsgSetAsset }
func (m MsgSetAsset) GetSigners() []sdk.AccAddress { return signer(m.Authority) }
func (m MsgSetAsset) GetSignBytes() []byte {
	return sdk.MustSortJSON(ModuleCdc.MustMarshalJSON(&m))
}
func (m MsgSetAsset) ValidateBasic() error {
	if err := validSigner(m.Authority); err != nil {
		return err
	}
	return m.Asset.Validate()
}

func (m MsgRegisterCheckpoint) Route() string                { return RouterKey }
func (m MsgRegisterCheckpoint) Type() string                 { return TypeMsgRegisterCheckpoint }
func (m MsgRegisterCheckpoint) GetSigners() []sdk.AccAddress { return signer(m.Authority) }
func (m MsgRegisterCheckpoint) GetSignBytes() []byte {
	return sdk.MustSortJSON(ModuleCdc.MustMarshalJSON(&m))
}
func (m MsgRegisterCheckpoint) ValidateBasic() error {
	if err := validSigner(m.Authority); err != nil {
		return err
	}
	return Checkpoint{BatchNumber: m.BatchNumber, StateRoot: m.StateRoot, ReceiptRoot: m.ReceiptRoot}.Validate()
}

func (m MsgSetEmergency) Route() string                { return RouterKey }
func (m MsgSetEmergency) Type() string                 { return TypeMsgSetEmergency }
func (m MsgSetEmergency) GetSigners() []sdk.AccAddress { return signer(m.Authority) }
func (m MsgSetEmergency) GetSignBytes() []byte {
	return sdk.MustSortJSON(ModuleCdc.MustMarshalJSON(&m))
}
func (m MsgSetEmergency) ValidateBasic() error { return validSigner(m.Authority) }

func (m MsgCancelClaim) Route() string                { return RouterKey }
func (m MsgCancelClaim) Type() string                 { return TypeMsgCancelClaim }
func (m MsgCancelClaim) GetSigners() []sdk.AccAddress { return signer(m.Authority) }
func (m MsgCancelClaim) GetSignBytes() []byte {
	return sdk.MustSortJSON(ModuleCdc.MustMarshalJSON(&m))
}
func (m MsgCancelClaim) ValidateBasic() error {
	if err := validSigner(m.Authority); err != nil {
		return err
	}
	_, err := ParseNonZeroHash32(m.ClaimId)
	return err
}

func (m MsgRequestWithdrawal) Route() string                { return RouterKey }
func (m MsgRequestWithdrawal) Type() string                 { return TypeMsgRequestWithdrawal }
func (m MsgRequestWithdrawal) GetSigners() []sdk.AccAddress { return signer(m.Sender) }
func (m MsgRequestWithdrawal) GetSignBytes() []byte {
	return sdk.MustSortJSON(ModuleCdc.MustMarshalJSON(&m))
}
func (m MsgRequestWithdrawal) ValidateBasic() error {
	if err := validSigner(m.Sender); err != nil {
		return err
	}
	return validWithdrawalEvidence(m.Receipt, m.Proof, m.Header, m.HeaderSignature)
}

func (m MsgFinaliseWithdrawal) Route() string                { return RouterKey }
func (m MsgFinaliseWithdrawal) Type() string                 { return TypeMsgFinaliseWithdrawal }
func (m MsgFinaliseWithdrawal) GetSigners() []sdk.AccAddress { return signer(m.Sender) }
func (m MsgFinaliseWithdrawal) GetSignBytes() []byte {
	return sdk.MustSortJSON(ModuleCdc.MustMarshalJSON(&m))
}
func (m MsgFinaliseWithdrawal) ValidateBasic() error {
	if err := validSigner(m.Sender); err != nil {
		return err
	}
	return validWithdrawalEvidence(m.Receipt, m.Proof, m.Header, m.HeaderSignature)
}

func (m MsgRequestForcedExit) Route() string                { return RouterKey }
func (m MsgRequestForcedExit) Type() string                 { return TypeMsgRequestForcedExit }
func (m MsgRequestForcedExit) GetSigners() []sdk.AccAddress { return signer(m.Sender) }
func (m MsgRequestForcedExit) GetSignBytes() []byte {
	return sdk.MustSortJSON(ModuleCdc.MustMarshalJSON(&m))
}
func (m MsgRequestForcedExit) ValidateBasic() error {
	if err := validSigner(m.Sender); err != nil {
		return err
	}
	return validExitEvidence(m.Witness, m.Account, m.AssetId, m.Recipient, m.RecipientSignature)
}

func (m MsgExecuteForcedExit) Route() string                { return RouterKey }
func (m MsgExecuteForcedExit) Type() string                 { return TypeMsgExecuteForcedExit }
func (m MsgExecuteForcedExit) GetSigners() []sdk.AccAddress { return signer(m.Sender) }
func (m MsgExecuteForcedExit) GetSignBytes() []byte {
	return sdk.MustSortJSON(ModuleCdc.MustMarshalJSON(&m))
}
func (m MsgExecuteForcedExit) ValidateBasic() error {
	if err := validSigner(m.Sender); err != nil {
		return err
	}
	return validExitEvidence(m.Witness, m.Account, m.AssetId, m.Recipient, m.RecipientSignature)
}

package types

import (
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// The governance messages. Each is executed by the keeper only when Authority
// is the module authority (the gov module account by default).

type MsgRegisterAttestor struct {
	Authority string   `json:"authority"`
	Attestor  Attestor `json:"attestor"`
}

type MsgRemoveAttestor struct {
	Authority string    `json:"authority"`
	Signer    Address20 `json:"signer"`
}

type MsgSetThreshold struct {
	Authority string `json:"authority"`
	Threshold uint32 `json:"threshold"`
}

type MsgSetParams struct {
	Authority       string  `json:"authority"`
	Fee             sdk.Int `json:"fee"`
	MaxPayloadBytes uint32  `json:"max_payload_bytes"`
	MaxCallbackGas  uint64  `json:"max_callback_gas"`
	TimeoutBlocks   uint64  `json:"timeout_blocks"`
}

type MsgPause struct {
	Authority string `json:"authority"`
}

type MsgUnpause struct {
	Authority string `json:"authority"`
}

func validateAuthority(authority string) error {
	if _, err := sdk.AccAddressFromBech32(authority); err != nil {
		return ErrUnauthorized.Wrapf("authority: %v", err)
	}
	return nil
}

func (m MsgRegisterAttestor) ValidateBasic() error {
	if err := validateAuthority(m.Authority); err != nil {
		return err
	}
	return m.Attestor.Validate()
}

func (m MsgRemoveAttestor) ValidateBasic() error {
	if err := validateAuthority(m.Authority); err != nil {
		return err
	}
	if m.Signer == (Address20{}) {
		return ErrInvalidAttestors.Wrap("zero signer")
	}
	return nil
}

func (m MsgSetThreshold) ValidateBasic() error {
	if err := validateAuthority(m.Authority); err != nil {
		return err
	}
	if m.Threshold == 0 {
		return ErrInvalidThreshold.Wrap("threshold is zero")
	}
	return nil
}

func (m MsgSetParams) ValidateBasic() error {
	if err := validateAuthority(m.Authority); err != nil {
		return err
	}
	return ValidateSettings(m.Fee, m.MaxPayloadBytes, m.MaxCallbackGas, m.TimeoutBlocks)
}

func (m MsgPause) ValidateBasic() error { return validateAuthority(m.Authority) }

func (m MsgUnpause) ValidateBasic() error { return validateAuthority(m.Authority) }

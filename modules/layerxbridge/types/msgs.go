package types

import (
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// The governance messages. Each is executed by the keeper only when Authority
// is the module authority (the gov module account by default).

type MsgRegisterChain struct {
	Authority string `json:"authority"`
	Chain     Chain  `json:"chain"`
}

type MsgSetAttestors struct {
	Authority string      `json:"authority"`
	Set       AttestorSet `json:"set"`
}

type MsgSetCap struct {
	Authority   string    `json:"authority"`
	ChainID     uint64    `json:"chain_id"`
	Asset       Address20 `json:"asset"`
	MaxInFlight sdk.Int   `json:"max_in_flight"`
	MaxPerTx    sdk.Int   `json:"max_per_tx"`
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

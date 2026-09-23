package types

import (
	"encoding/hex"
	"encoding/json"
	"fmt"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// Hash32 is a 32-byte value rendered as hex in genesis and state JSON.
type Hash32 [32]byte

func (h Hash32) MarshalJSON() ([]byte, error) { return json.Marshal(hex.EncodeToString(h[:])) }

func (h *Hash32) UnmarshalJSON(raw []byte) error { return unmarshalFixedHex(raw, h[:]) }

// Address20 is a 20-byte EVM address rendered as hex.
type Address20 [20]byte

func (a Address20) MarshalJSON() ([]byte, error) { return json.Marshal(hex.EncodeToString(a[:])) }

func (a *Address20) UnmarshalJSON(raw []byte) error { return unmarshalFixedHex(raw, a[:]) }

func (a Address20) Hex() string { return "0x" + hex.EncodeToString(a[:]) }

func unmarshalFixedHex(raw []byte, out []byte) error {
	var text string
	if err := json.Unmarshal(raw, &text); err != nil {
		return err
	}
	decoded, err := hex.DecodeString(text)
	if err != nil {
		return err
	}
	if len(decoded) != len(out) {
		return fmt.Errorf("expected %d bytes, got %d", len(out), len(decoded))
	}
	copy(out, decoded)
	return nil
}

// Params holds the module authority: the only account that may register
// chains, set attestors and caps, and pause. It defaults to the gov module
// account.
type Params struct {
	Authority string `json:"authority"`
}

func DefaultParams(authority string) Params { return Params{Authority: authority} }

func (p Params) Validate() error {
	if _, err := sdk.AccAddressFromBech32(p.Authority); err != nil {
		return ErrInvalidParams.Wrapf("authority: %v", err)
	}
	return nil
}

// Chain is one remote chain the bridge accepts events from. Vault is the
// address of the remote PaxeerXVault whose events attestors sign for and
// which releases bridgeOut burns; FinalityDepth is the
// number of confirmations attestors wait for before signing. A disabled
// chain keeps its assets and nullifiers but accepts no bridgeIn or bridgeOut.
type Chain struct {
	ChainID       uint64    `json:"chain_id"`
	Vault         Address20 `json:"vault"`
	FinalityDepth uint64    `json:"finality_depth"`
	Enabled       bool      `json:"enabled"`
}

func (c Chain) Validate() error {
	if c.ChainID == 0 {
		return ErrInvalidChain.Wrap("chain id is zero")
	}
	if c.Vault == (Address20{}) {
		return ErrInvalidChain.Wrap("zero vault")
	}
	if c.FinalityDepth == 0 {
		return ErrInvalidChain.Wrap("finality depth is zero")
	}
	return nil
}

// Attestor is one signer of bridgeIn attestations: the EVM address its
// secp256k1 signatures recover to and the bond it has declared.
type Attestor struct {
	Signer Address20 `json:"signer"`
	Bond   sdk.Int   `json:"bond"`
}

// AttestorSet is the signer set and the number of distinct members whose
// signatures a bridgeIn needs. The empty set with threshold zero accepts
// nothing.
type AttestorSet struct {
	Attestors []Attestor `json:"attestors"`
	Threshold uint32     `json:"threshold"`
}

func (s AttestorSet) Validate() error {
	if len(s.Attestors) == 0 {
		if s.Threshold != 0 {
			return ErrInvalidAttestors.Wrap("threshold without attestors")
		}
		return nil
	}
	if len(s.Attestors) > MaxAttestors {
		return ErrInvalidAttestors.Wrapf("more than %d attestors", MaxAttestors)
	}
	if s.Threshold == 0 || int(s.Threshold) > len(s.Attestors) {
		return ErrInvalidAttestors.Wrapf("threshold %d outside 1..%d", s.Threshold, len(s.Attestors))
	}
	seen := map[Address20]bool{}
	for _, attestor := range s.Attestors {
		if attestor.Signer == (Address20{}) {
			return ErrInvalidAttestors.Wrap("zero signer")
		}
		if seen[attestor.Signer] {
			return ErrInvalidAttestors.Wrapf("duplicate signer %s", attestor.Signer.Hex())
		}
		seen[attestor.Signer] = true
		if attestor.Bond.IsNil() || attestor.Bond.IsNegative() {
			return ErrInvalidAttestors.Wrapf("bond of %s", attestor.Signer.Hex())
		}
	}
	return nil
}

// Has reports whether signer is a member of the set.
func (s AttestorSet) Has(signer Address20) bool {
	for _, attestor := range s.Attestors {
		if attestor.Signer == signer {
			return true
		}
	}
	return false
}

// BridgedAsset maps one remote asset of one chain to its module-owned
// tokenfactory denom.
type BridgedAsset struct {
	ChainID uint64    `json:"chain_id"`
	Asset   Address20 `json:"asset"`
	Denom   string    `json:"denom"`
}

// Cap bounds a bridged denom. MaxPerTx bounds one bridgeIn; MaxInFlight
// bounds the bridged supply outstanding on this chain (minted by bridgeIn and
// not yet burned by bridgeOut). A zero cap refuses every bridgeIn.
type Cap struct {
	Denom       string  `json:"denom"`
	MaxInFlight sdk.Int `json:"max_in_flight"`
	MaxPerTx    sdk.Int `json:"max_per_tx"`
}

func (c Cap) Validate() error {
	if c.Denom == "" {
		return ErrInvalidCap.Wrap("empty denom")
	}
	if c.MaxInFlight.IsNil() || c.MaxPerTx.IsNil() || c.MaxInFlight.IsNegative() || c.MaxPerTx.IsNegative() {
		return ErrInvalidCap.Wrap("caps must be non-negative")
	}
	if c.MaxPerTx.GT(c.MaxInFlight) {
		return ErrInvalidCap.Wrap("max per tx exceeds max in flight")
	}
	return nil
}

// InFlight is the outstanding bridged supply of one denom.
type InFlight struct {
	Denom  string  `json:"denom"`
	Amount sdk.Int `json:"amount"`
}

// Nullifier identifies one remote vault event. A bridgeIn consumes it once.
type Nullifier struct {
	ChainID  uint64 `json:"chain_id"`
	TxHash   Hash32 `json:"tx_hash"`
	LogIndex uint64 `json:"log_index"`
}

// OutboundNonce is the last bridgeOut nonce issued for one chain.
type OutboundNonce struct {
	ChainID uint64 `json:"chain_id"`
	Nonce   uint64 `json:"nonce"`
}

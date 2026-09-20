package types

import (
	"github.com/sidiora-labs/paxeer-network/layerxproof/verify"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
)

const (
	DefaultWithdrawalDelaySeconds uint64 = 3600
	DefaultForcedExitDelaySeconds uint64 = 0
	DefaultLivenessBoundSeconds   uint64 = 86400
	// MinLivenessBoundSeconds is EmergencyExit's constructor floor.
	MinLivenessBoundSeconds uint64 = 3600
	// MaxDelaySeconds bounds both payout windows.
	MaxDelaySeconds uint64 = 90 * 86400
	// MaxSequencerAuthorizations bounds the params list scanned per lookup.
	MaxSequencerAuthorizations = 64
)

func DefaultParams() Params {
	return Params{
		WithdrawalDelaySeconds: DefaultWithdrawalDelaySeconds,
		ForcedExitDelaySeconds: DefaultForcedExitDelaySeconds,
		LivenessBoundSeconds:   DefaultLivenessBoundSeconds,
	}
}

// Decode returns the verifier form of an authorization.
func (a SequencerAuthorization) Decode() (verify.SequencerAuthorization, error) {
	id, err := ParseNonZeroHash32(a.SequencerId)
	if err != nil {
		return verify.SequencerAuthorization{}, err
	}
	key, err := ParseNonZeroHash32(a.PublicKey)
	if err != nil {
		return verify.SequencerAuthorization{}, err
	}
	if !verify.PublicKeyIsCanonical(key) {
		return verify.SequencerAuthorization{}, sdkerrors.Wrap(ErrInvalidParams, "sequencer public key is not a canonical Ed25519 key")
	}
	if a.FirstBatchNumber > a.LastBatchNumber {
		return verify.SequencerAuthorization{}, sdkerrors.Wrap(ErrInvalidParams, "sequencer batch range is reversed")
	}
	return verify.SequencerAuthorization{SequencerID: id, PublicKey: key,
		FirstBatchNumber: a.FirstBatchNumber, LastBatchNumber: a.LastBatchNumber}, nil
}

func (p Params) Validate() error {
	if p.Authority != "" {
		if _, err := sdk.AccAddressFromBech32(p.Authority); err != nil {
			return sdkerrors.Wrapf(ErrInvalidParams, "authority: %s", err)
		}
	}
	if p.WithdrawalDelaySeconds > MaxDelaySeconds || p.ForcedExitDelaySeconds > MaxDelaySeconds {
		return sdkerrors.Wrap(ErrInvalidParams, "delay exceeds the 90 day bound")
	}
	if p.LivenessBoundSeconds < MinLivenessBoundSeconds {
		return sdkerrors.Wrap(ErrInvalidParams, "liveness bound is below one hour")
	}
	if len(p.SequencerAuthorizations) > MaxSequencerAuthorizations {
		return sdkerrors.Wrap(ErrInvalidParams, "too many sequencer authorizations")
	}
	if len(p.SequencerAuthorizations) > 0 && p.NetworkId == 0 {
		return sdkerrors.Wrap(ErrInvalidParams, "network id is required once a sequencer is authorized")
	}
	decoded := make([]verify.SequencerAuthorization, 0, len(p.SequencerAuthorizations))
	for _, authorization := range p.SequencerAuthorizations {
		next, err := authorization.Decode()
		if err != nil {
			return err
		}
		for _, previous := range decoded {
			if next.FirstBatchNumber <= previous.LastBatchNumber && previous.FirstBatchNumber <= next.LastBatchNumber {
				return sdkerrors.Wrap(ErrInvalidParams, "sequencer batch ranges overlap")
			}
		}
		decoded = append(decoded, next)
	}
	return nil
}

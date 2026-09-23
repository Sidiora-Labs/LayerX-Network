package types

import (
	custodytypes "github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
)

func DefaultGenesis() *GenesisState {
	return &GenesisState{Params: DefaultParams()}
}

// Validate checks the shape of one recorded intent.
func (i Intent) Validate() error {
	if _, err := custodytypes.ParseNonZeroHash32(i.IntentId); err != nil {
		return sdkerrors.Wrapf(ErrInvalidIntent, "intent id: %s", err)
	}
	if i.Kind == IntentKind_INTENT_KIND_UNSPECIFIED || IntentKind_name[int32(i.Kind)] == "" {
		return sdkerrors.Wrap(ErrInvalidIntent, "kind")
	}
	if i.Status != IntentStatus_INTENT_STATUS_PENDING {
		return sdkerrors.Wrap(ErrInvalidIntent, "status")
	}
	if _, err := custodytypes.ParseAddress(i.Owner); err != nil {
		return sdkerrors.Wrapf(ErrInvalidIntent, "owner: %s", err)
	}
	if i.Nonce == 0 {
		return sdkerrors.Wrap(ErrInvalidIntent, "nonce")
	}
	return nil
}

func (gs GenesisState) Validate() error {
	if err := gs.Params.Validate(); err != nil {
		return err
	}
	intents := map[string]bool{}
	for _, intent := range gs.Intents {
		if err := intent.Validate(); err != nil {
			return err
		}
		if intents[intent.IntentId] {
			return sdkerrors.Wrapf(ErrInvalidGenesis, "intent %s is recorded twice", intent.IntentId)
		}
		intents[intent.IntentId] = true
	}
	if uint64(len(gs.Intents)) > gs.IntentCount {
		return sdkerrors.Wrap(ErrInvalidGenesis, "intent count is below the recorded intents")
	}
	owners := map[string]bool{}
	for _, nonce := range gs.OwnerNonces {
		if _, err := custodytypes.ParseAddress(nonce.Owner); err != nil {
			return sdkerrors.Wrapf(ErrInvalidGenesis, "owner: %s", err)
		}
		if owners[nonce.Owner] {
			return sdkerrors.Wrapf(ErrInvalidGenesis, "owner %s has two nonces", nonce.Owner)
		}
		owners[nonce.Owner] = true
	}
	return nil
}

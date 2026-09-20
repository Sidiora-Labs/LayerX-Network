package types

import sdk "github.com/sidiora-labs/paxeer-network/sdk/types"

// AddressHandler is an interface that defines the methods to handle addresses
type AddressHandler interface {

	// GetPaxAddressFromString parses an address string and returns the corresponding sdk.AccAddress.
	// Address string does not have to be a bech32 address. It could be a 0x prefixed (EVM) address, etc.
	GetPaxAddressFromString(ctx sdk.Context, address string) (sdk.AccAddress, error)
}

type PaxAddressHandler struct{}

// GetPaxAddressFromString parses a bech32 address formatted string and returns the corresponding sdk.AccAddress
func (h PaxAddressHandler) GetPaxAddressFromString(_ sdk.Context, address string) (sdk.AccAddress, error) {
	parsedAddress, err := sdk.AccAddressFromBech32(address)
	if err != nil {
		return nil, err
	}
	return parsedAddress, nil
}

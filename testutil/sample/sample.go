package sample

import (
	"github.com/sidiora-labs/paxeer-network/sdk/crypto/keys/ed25519"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// AccAddress returns a sample account address
func AccAddress() string {
	pk := ed25519.GenPrivKey().PubKey()
	addr := pk.Address()
	return sdk.AccAddress(addr).String()
}

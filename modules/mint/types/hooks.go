package types

import (
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

type MintHooks interface {
	AfterDistributeMintedCoin(ctx sdk.Context, mintedCoin sdk.Coin)
}

var _ MintHooks = MultiMintHooks{}

// combine multiple mint hooks, all hook functions are run in array sequence.
type MultiMintHooks []MintHooks

func NewMultiMintHooks(hooks ...MintHooks) MultiMintHooks {
	return hooks
}

func (h MultiMintHooks) AfterDistributeMintedCoin(ctx sdk.Context, mintedCoin sdk.Coin) {
	for i := range h {
		h[i].AfterDistributeMintedCoin(ctx, mintedCoin)
	}
}

package types

import (
	"github.com/ethereum/go-ethereum/common"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// BankKeeper moves bridged coins between the recipient or sender and the
// module account around each tokenfactory mint and burn.
type BankKeeper interface {
	GetBalance(ctx sdk.Context, addr sdk.AccAddress, denom string) sdk.Coin
	SendCoins(ctx sdk.Context, from sdk.AccAddress, to sdk.AccAddress, amt sdk.Coins) error
}

// EVMKeeper resolves EVM addresses to the bank accounts that hold their funds.
type EVMKeeper interface {
	GetPaxAddressOrDefault(ctx sdk.Context, evmAddress common.Address) sdk.AccAddress
}

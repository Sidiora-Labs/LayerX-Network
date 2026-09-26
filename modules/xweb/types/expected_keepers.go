package types

import (
	"math/big"

	"github.com/ethereum/go-ethereum/common"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// BankKeeper moves request fees into the module account and out to the
// signers' payout accounts or back to the requester.
type BankKeeper interface {
	GetBalance(ctx sdk.Context, addr sdk.AccAddress, denom string) sdk.Coin
	SendCoins(ctx sdk.Context, from sdk.AccAddress, to sdk.AccAddress, amt sdk.Coins) error
}

// EVMKeeper resolves EVM addresses to the bank accounts that hold their funds
// and names the EVM chain id the origin-1 preimage commits to.
type EVMKeeper interface {
	GetPaxAddressOrDefault(ctx sdk.Context, evmAddress common.Address) sdk.AccAddress
	ChainID(ctx sdk.Context) *big.Int
}

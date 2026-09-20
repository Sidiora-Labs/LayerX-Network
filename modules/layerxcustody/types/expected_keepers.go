package types

import (
	"math/big"

	"github.com/ethereum/go-ethereum/common"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
)

// BankKeeper is the only path custody funds move through.
type BankKeeper interface {
	GetBalance(ctx sdk.Context, addr sdk.AccAddress, denom string) sdk.Coin
	SendCoinsFromModuleToAccount(ctx sdk.Context, senderModule string, recipientAddr sdk.AccAddress, amt sdk.Coins) error
	SendCoinsFromAccountToModule(ctx sdk.Context, senderAddr sdk.AccAddress, recipientModule string, amt sdk.Coins) error
}

// AccountKeeper creates and resolves the custody module account.
type AccountKeeper interface {
	GetModuleAddress(name string) sdk.AccAddress
	GetModuleAccount(ctx sdk.Context, moduleName string) authtypes.ModuleAccountI
}

// EVMKeeper resolves EVM addresses to the bank accounts that hold their funds.
type EVMKeeper interface {
	GetPaxAddressOrDefault(ctx sdk.Context, evmAddress common.Address) sdk.AccAddress
	ChainID(ctx sdk.Context) *big.Int
}

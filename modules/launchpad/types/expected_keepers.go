package types

import (
	"context"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/vm"
	tokenfactorytypes "github.com/sidiora-labs/paxeer-network/modules/tokenfactory/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/utils"
)

// BankKeeper moves quote and launched tokens between traders, the escrow
// account and the treasury.
type BankKeeper interface {
	SendCoins(ctx sdk.Context, fromAddr sdk.AccAddress, toAddr sdk.AccAddress, amt sdk.Coins) error
	GetBalance(ctx sdk.Context, addr sdk.AccAddress, denom string) sdk.Coin
	GetSupply(ctx sdk.Context, denom string) sdk.Coin
}

// TokenFactory is the tokenfactory message service the launchpad account
// uses as the admin of every denom it launches.
type TokenFactory interface {
	CreateDenom(ctx context.Context, msg *tokenfactorytypes.MsgCreateDenom) (*tokenfactorytypes.MsgCreateDenomResponse, error)
	SetDenomMetadata(ctx context.Context, msg *tokenfactorytypes.MsgSetDenomMetadata) (*tokenfactorytypes.MsgSetDenomMetadataResponse, error)
	Mint(ctx context.Context, msg *tokenfactorytypes.MsgMint) (*tokenfactorytypes.MsgMintResponse, error)
}

// EVMKeeper registers ERC20 pointers and maps EVM addresses to bank accounts.
type EVMKeeper interface {
	GetPaxAddressOrDefault(ctx sdk.Context, evmAddress common.Address) sdk.AccAddress
	GetEVMAddressOrDefault(ctx sdk.Context, paxAddress sdk.AccAddress) common.Address
	UpsertERCNativePointer(ctx sdk.Context, evm *vm.EVM, token string, metadata utils.ERCMetadata) (common.Address, error)
	RunWithOneOffEVMInstance(ctx sdk.Context, runner func(*vm.EVM) error, logger func(string, string)) error
}

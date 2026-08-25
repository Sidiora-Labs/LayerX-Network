package state

import (
	"math/big"

	"github.com/ethereum/go-ethereum/common"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	authkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/auth/keeper"
	bankkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/bank/keeper"
	upgradekeeper "github.com/sidiora-labs/paxeer-network/sdk/x/upgrade/keeper"
)

type EVMKeeper interface {
	PrefixStore(sdk.Context, []byte) sdk.KVStore
	PurgePrefix(sdk.Context, []byte)
	GetPaxAddress(sdk.Context, common.Address) (sdk.AccAddress, bool)
	GetPaxAddressOrDefault(ctx sdk.Context, evmAddress common.Address) sdk.AccAddress
	BankKeeper() bankkeeper.Keeper
	GetBaseDenom(sdk.Context) string
	DeleteAddressMapping(sdk.Context, sdk.AccAddress, common.Address)
	GetCode(sdk.Context, common.Address) []byte
	SetCode(sdk.Context, common.Address, []byte)
	GetCodeHash(sdk.Context, common.Address) common.Hash
	GetCodeSize(sdk.Context, common.Address) int
	GetState(sdk.Context, common.Address, common.Hash) common.Hash
	SetState(sdk.Context, common.Address, common.Hash, common.Hash)
	AccountKeeper() *authkeeper.AccountKeeper
	GetFeeCollectorAddress(sdk.Context) (common.Address, error)
	GetNonce(sdk.Context, common.Address) uint64
	SetNonce(sdk.Context, common.Address, uint64)
	PrepareReplayedAddr(ctx sdk.Context, addr common.Address)
	GetBalance(ctx sdk.Context, addr sdk.AccAddress) *big.Int
	UpgradeKeeper() *upgradekeeper.Keeper
}

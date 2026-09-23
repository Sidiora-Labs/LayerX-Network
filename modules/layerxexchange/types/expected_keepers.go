package types

import (
	"math/big"

	"github.com/ethereum/go-ethereum/common"
	custodytypes "github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// CustodyKeeper is the only path margin moves through: a margin deposit is a
// layerxcustody deposit into the custody module account, and finalized LayerX
// state is read through custody's AnchorReader.
type CustodyKeeper interface {
	Deposit(ctx sdk.Context, payer common.Address, payerAccount sdk.AccAddress, assetID, beneficiary [32]byte,
		amount sdk.Int) (custodytypes.Deposit, error)
	GetAsset(ctx sdk.Context, assetID [32]byte) (custodytypes.AssetMapping, bool)
	GetAssetByPointer(ctx sdk.Context, pointer common.Address) (custodytypes.AssetMapping, bool)
	GetAssetByDenom(ctx sdk.Context, denom string) (custodytypes.AssetMapping, bool)
	Anchor() custodytypes.AnchorReader
}

// EVMKeeper supplies the chain id every intent identifier binds.
type EVMKeeper interface {
	ChainID(ctx sdk.Context) *big.Int
}

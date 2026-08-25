package keeper

import (
	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/evm/artifacts"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func (k *Keeper) QueryERCSingleOutput(ctx sdk.Context, typ string, addr common.Address, query string) (interface{}, error) {
	moduleAddr := k.AccountKeeper().GetModuleAddress(types.ModuleName)
	q, _ := artifacts.GetParsedABI(typ).Pack(query)
	r, err := k.StaticCallEVM(ctx, moduleAddr, &addr, q)
	if err != nil {
		logger.Error("Error calling address for query, skipping", "address", addr, "query", query, "err", err)
		return nil, err
	}
	o, _ := artifacts.GetParsedABI(typ).Unpack(query, r)
	if len(o) != 1 {
		logger.Error("Not getting exactly one outputs when querying address, skipping", "outputs", len(o), "address", addr, "query", query)
		return nil, err
	}
	return o[0], nil
}

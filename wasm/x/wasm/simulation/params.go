package simulation

import (
	"fmt"
	"math/rand"

	"github.com/sidiora-labs/paxeer-network/sdk/codec"
	simtypes "github.com/sidiora-labs/paxeer-network/sdk/types/simulation"
	"github.com/sidiora-labs/paxeer-network/sdk/x/simulation"

	"github.com/sidiora-labs/paxeer-network/wasm/x/wasm/types"
)

func ParamChanges(r *rand.Rand, cdc codec.Codec) []simtypes.ParamChange {
	params := RandomParams(r)
	return []simtypes.ParamChange{
		simulation.NewSimParamChange(types.ModuleName, string(types.ParamStoreKeyUploadAccess),
			func(r *rand.Rand) string {
				jsonBz, err := cdc.MarshalAsJSON(&params.CodeUploadAccess)
				if err != nil {
					panic(err)
				}
				return string(jsonBz)
			},
		),
		simulation.NewSimParamChange(types.ModuleName, string(types.ParamStoreKeyInstantiateAccess),
			func(r *rand.Rand) string {
				return fmt.Sprintf("%q", params.CodeUploadAccess.Permission.String())
			},
		),
	}
}

func RandomParams(r *rand.Rand) types.Params {
	permissionType := types.AccessType(simtypes.RandIntBetween(r, 1, 3)) // #nosec G115
	account, _ := simtypes.RandomAcc(r, simtypes.RandomAccounts(r, 10))
	accessConfig := permissionType.With(account.Address)
	return types.Params{
		CodeUploadAccess:             accessConfig,
		InstantiateDefaultPermission: accessConfig.Permission,
	}
}

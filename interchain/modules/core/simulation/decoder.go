package simulation

import (
	"fmt"

	"github.com/sidiora-labs/paxeer-network/sdk/types/kv"

	clientsim "github.com/sidiora-labs/paxeer-network/interchain/modules/core/02-client/simulation"
	connectionsim "github.com/sidiora-labs/paxeer-network/interchain/modules/core/03-connection/simulation"
	channelsim "github.com/sidiora-labs/paxeer-network/interchain/modules/core/04-channel/simulation"
	host "github.com/sidiora-labs/paxeer-network/interchain/modules/core/24-host"
	"github.com/sidiora-labs/paxeer-network/interchain/modules/core/keeper"
)

// NewDecodeStore returns a decoder function closure that unmarshals the KVPair's
// Value to the corresponding ibc type.
func NewDecodeStore(k keeper.Keeper) func(kvA, kvB kv.Pair) string {
	return func(kvA, kvB kv.Pair) string {
		if res, found := clientsim.NewDecodeStore(k.ClientKeeper, kvA, kvB); found {
			return res
		}

		if res, found := connectionsim.NewDecodeStore(k.Codec(), kvA, kvB); found {
			return res
		}

		if res, found := channelsim.NewDecodeStore(k.Codec(), kvA, kvB); found {
			return res
		}

		panic(fmt.Sprintf("invalid %s key prefix: %s", host.ModuleName, string(kvA.Key)))
	}
}

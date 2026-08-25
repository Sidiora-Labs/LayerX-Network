package hashable

import (
	"github.com/sidiora-labs/paxeer-network/consensus/libs/utils"
)

func GenHash[T Hashable](rng utils.Rng) Hash[T] {
	return Hash[T](utils.GenBytes(rng, len(Hash[T]{})))
}

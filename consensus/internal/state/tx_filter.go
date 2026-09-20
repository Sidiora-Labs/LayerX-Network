package state

import (
	"sync"
	"time"

	"github.com/sidiora-labs/paxeer-network/consensus/internal/mempool"
	"github.com/sidiora-labs/paxeer-network/consensus/types"
)

func cachingStateFetcher(store Store) func() (State, error) {
	const ttl = time.Second

	var (
		last  time.Time
		mutex = &sync.Mutex{}
		cache State
		err   error
	)

	return func() (State, error) {
		mutex.Lock()
		defer mutex.Unlock()

		if time.Since(last) < ttl && cache.ChainID != "" {
			return cache, nil
		}

		cache, err = store.Load()
		if err != nil {
			return State{}, err
		}
		last = time.Now()

		return cache, nil
	}

}

// TxConstraintsFetcherFromStore returns the precomputed consensus-derived mempool limits for the
// current persisted state.
func TxConstraintsFetcherFromStore(store Store) mempool.TxConstraintsFetcher {
	fetch := cachingStateFetcher(store)

	return func() (mempool.TxConstraints, error) {
		state, err := fetch()
		if err != nil {
			return mempool.TxConstraints{}, err
		}

		return TxConstraintsForState(state), nil
	}
}

func TxConstraintsForState(state State) mempool.TxConstraints {
	return mempool.TxConstraints{
		MaxDataBytes: types.MaxDataBytesNoEvidence(
			state.ConsensusParams.Block.MaxBytes,
			state.Validators.Size(),
		),
		MaxGas: state.ConsensusParams.Block.MaxGas,
	}
}

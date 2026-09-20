package erc721_test

import (
	"sync"
	"testing"

	"github.com/sidiora-labs/paxeer-network/modules/evm/artifacts/erc721"
	"github.com/stretchr/testify/require"
)

// run with `-race`
func TestGetBinConcurrent(t *testing.T) {
	var wg sync.WaitGroup

	for i := 0; i < 100; i++ {
		wg.Add(1)
		go func(val int) {
			defer wg.Done()
			require.NotEmpty(t, erc721.GetBin())
		}(i)
	}

	wg.Wait()
}

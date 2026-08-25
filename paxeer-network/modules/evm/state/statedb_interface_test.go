package state

import (
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/stretchr/testify/require"
)

func TestUnsupportedEthereumTrieCommitFailsClosed(t *testing.T) {
	db := &DBImpl{}
	root, err := db.Commit(1, true, false)
	require.ErrorIs(t, err, errUnsupportedStateRoot)
	require.Equal(t, common.Hash{}, root)
}

func TestUnsupportedIntermediateRootRecordsExecutionError(t *testing.T) {
	db := &DBImpl{}
	root := db.IntermediateRoot(true)
	require.Equal(t, common.Hash{}, root)
	require.ErrorIs(t, db.Error(), errUnsupportedStateRoot)
}

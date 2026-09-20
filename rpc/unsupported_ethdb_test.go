package evmrpc

import (
	"testing"

	"github.com/stretchr/testify/require"
)

func TestUnsupportedEthereumChainDatabaseReturnsErrors(t *testing.T) {
	backend := &Backend{}
	database := backend.ChainDb()
	require.NotNil(t, database)
	_, err := database.Get([]byte("bad-block"))
	require.ErrorIs(t, err, errEthereumChainDatabaseUnavailable)
	_, err = database.Has([]byte("bad-block"))
	require.ErrorIs(t, err, errEthereumChainDatabaseUnavailable)
}

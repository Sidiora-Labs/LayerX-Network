package migrations

import (
	"errors"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/stretchr/testify/require"
)

func TestDecodeERCPointerRegistryEntryRejectsMalformedShapes(t *testing.T) {
	_, _, err := decodeERCPointerRegistryEntry([]byte{1, 2}, make([]byte, common.AddressLength))
	require.Error(t, err)

	_, _, err = decodeERCPointerRegistryEntry([]byte{'p', 0, 1}, make([]byte, common.AddressLength-1))
	require.Error(t, err)
}

func TestPointerMetadataRejectsWrongABIOutputTypes(t *testing.T) {
	_, err := stringPointerMetadata("name", []byte("name"))
	require.Error(t, err)

	_, err = uint8PointerMetadata("decimals", uint64(6))
	require.Error(t, err)
}

func TestRunERCPointerUpgradePropagatesEVMFailure(t *testing.T) {
	evmErr := errors.New("evm execution failed")
	err := runERCPointerUpgrade("native", "uhpx", func() error { return evmErr })
	require.ErrorIs(t, err, evmErr)
}

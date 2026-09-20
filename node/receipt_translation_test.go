package app

import (
	"errors"
	"math/big"
	"testing"

	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	wasmtypes "github.com/sidiora-labs/paxeer-network/wasm/x/wasm/types"
	"github.com/stretchr/testify/require"
)

func TestSyntheticReceiptUint256Validation(t *testing.T) {
	require.ErrorIs(t, validateUint256("amount", nil), ErrSyntheticReceiptTranslation)
	require.ErrorIs(t, validateUint256("amount", big.NewInt(-1)), ErrSyntheticReceiptTranslation)
	require.ErrorIs(t, validateUint256("amount", new(big.Int).Lsh(big.NewInt(1), 256)), ErrSyntheticReceiptTranslation)
	require.NoError(t, validateUint256("amount", new(big.Int).Sub(new(big.Int).Lsh(big.NewInt(1), 256), big.NewInt(1))))
}

func TestCW721OwnerEventIndexRejectsMalformedShape(t *testing.T) {
	_, err := indexCW721OwnerEvents([]abci.Event{{
		Type: wasmtypes.EventTypeCW721PreTransferOwner,
		Attributes: []abci.EventAttribute{
			{Key: []byte(wasmtypes.AttributeKeyContractAddr), Value: []byte("contract")},
			{Key: []byte(wasmtypes.AttributeKeyTokenId), Value: []byte("1")},
		},
	}})
	require.True(t, errors.Is(err, ErrSyntheticReceiptTranslation))
}

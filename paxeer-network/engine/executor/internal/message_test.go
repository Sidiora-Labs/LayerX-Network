package internal

import (
	"errors"
	"math/big"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/stretchr/testify/require"
)

func TestTransactionToMessageUsesAuthenticatedSender(t *testing.T) {
	to := common.HexToAddress("0x2222222222222222222222222222222222222222")
	sender := common.HexToAddress("0x1111111111111111111111111111111111111111")
	tx := types.NewTx(&types.DynamicFeeTx{
		ChainID:   big.NewInt(1329),
		Nonce:     7,
		GasTipCap: big.NewInt(3),
		GasFeeCap: big.NewInt(20),
		Gas:       45_000,
		To:        &to,
		Value:     big.NewInt(9),
		Data:      []byte{0xaa, 0xbb},
	})

	message, err := TransactionToMessage(tx, sender, big.NewInt(10))
	require.NoError(t, err)
	require.Equal(t, sender, message.From)
	require.Equal(t, tx.Nonce(), message.Nonce)
	require.Equal(t, tx.Gas(), message.GasLimit)
	require.Equal(t, tx.To(), message.To)
	require.Equal(t, tx.Value(), message.Value)
	require.Equal(t, tx.Data(), message.Data)
	require.Equal(t, big.NewInt(13), message.GasPrice)
	require.Equal(t, tx.GasFeeCap(), message.GasFeeCap)
	require.Equal(t, tx.GasTipCap(), message.GasTipCap)
}

func TestTransactionToMessageRejectsNilTransaction(t *testing.T) {
	message, err := TransactionToMessage(nil, common.Address{}, nil)
	require.Nil(t, message)
	require.True(t, errors.Is(err, ErrNilTransaction))
}

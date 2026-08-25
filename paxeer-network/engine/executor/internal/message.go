package internal

import (
	"errors"
	"math/big"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core"
	"github.com/ethereum/go-ethereum/core/types"
)

var ErrNilTransaction = errors.New("cannot construct execution message from a nil transaction")

// TransactionToMessage converts a transaction after its sender has already
// been authenticated by the Paxeer ante path. It deliberately does not expose
// a types.Signer: sender recovery belongs at the authentication boundary, and
// pretending to implement the remaining signer operations makes later crypto
// use silently trust an unrelated address.
func TransactionToMessage(tx *types.Transaction, sender common.Address, baseFee *big.Int) (*core.Message, error) {
	if tx == nil {
		return nil, ErrNilTransaction
	}

	message := &core.Message{
		To:                    tx.To(),
		From:                  sender,
		Nonce:                 tx.Nonce(),
		Value:                 tx.Value(),
		GasLimit:              tx.Gas(),
		GasPrice:              new(big.Int).Set(tx.GasPrice()),
		GasFeeCap:             new(big.Int).Set(tx.GasFeeCap()),
		GasTipCap:             new(big.Int).Set(tx.GasTipCap()),
		Data:                  tx.Data(),
		AccessList:            tx.AccessList(),
		BlobGasFeeCap:         tx.BlobGasFeeCap(),
		BlobHashes:            tx.BlobHashes(),
		SetCodeAuthorizations: tx.SetCodeAuthorizations(),
	}
	if baseFee != nil {
		message.GasPrice.Add(message.GasTipCap, baseFee)
		if message.GasPrice.Cmp(message.GasFeeCap) > 0 {
			message.GasPrice.Set(message.GasFeeCap)
		}
	}
	return message, nil
}

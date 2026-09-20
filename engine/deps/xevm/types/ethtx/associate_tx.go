package ethtx

import (
	"math/big"

	ethtypes "github.com/ethereum/go-ethereum/core/types"
)

func NewAssociateTx(tx *ethtypes.Transaction, customMessage string) (*AssociateTx, error) {
	v, r, s := tx.RawSignatureValues()
	txData := &AssociateTx{
		V:             v.Bytes(),
		R:             r.Bytes(),
		S:             s.Bytes(),
		CustomMessage: customMessage,
	}
	return txData, nil
}

func (tx *AssociateTx) Copy() TxData {
	return &AssociateTx{
		V:             append([]byte(nil), tx.V...),
		R:             append([]byte(nil), tx.R...),
		S:             append([]byte(nil), tx.S...),
		CustomMessage: tx.CustomMessage,
	}
}

func (tx *AssociateTx) GetRawSignatureValues() (v, r, s *big.Int) {
	return rawSignatureValues(tx.V, tx.R, tx.S)
}
func (tx *AssociateTx) Validate() error {
	if err := validateSignatureValue("v", tx.V, 32); err != nil {
		return err
	}
	if err := validateSignatureValue("r", tx.R, 32); err != nil {
		return err
	}
	if err := validateSignatureValue("s", tx.S, 32); err != nil {
		return err
	}
	return nil
}

package types

import (
	"crypto/sha256"
	"errors"
	"fmt"

	"github.com/sidiora-labs/paxeer-network/consensus/crypto"
	tmbytes "github.com/sidiora-labs/paxeer-network/consensus/libs/bytes"
	tmproto "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
)

// TxHash is the fixed length array hash used as an index.
type TxHash crypto.Hash

func (txHash TxHash) Bytes() tmbytes.HexBytes {
	return crypto.Hash(txHash).Bytes()
}

// ToProto converts Data to protobuf
func (txHash *TxHash) ToProto() *tmproto.TxKey {
	tp := new(tmproto.TxKey)

	txBzs := make([]byte, len(txHash))
	if len(txHash) > 0 {
		copy(txBzs, txHash[:])
		tp.TxKey = txBzs
	}

	return tp
}

func (txHash TxHash) String() string {
	return txHash.Bytes().String()
}

func (txHash TxHash) Format(s fmt.State, verb rune) {
	txHash.Bytes().Format(s, verb)
}

// TxHashFromProto takes a protobuf representation of TxHash &
// returns the native type.
func TxHashFromProto(dp *tmproto.TxKey) (TxHash, error) {
	if dp == nil {
		return TxHash{}, errors.New("nil data")
	}
	var txBzs [sha256.Size]byte
	copy(txBzs[:], dp.TxKey)

	return txBzs, nil
}

func TxHashesListFromProto(dps []*tmproto.TxKey) ([]TxHash, error) {
	var txHashes []TxHash
	for _, txHash := range dps {
		txHash, err := TxHashFromProto(txHash)
		if err != nil {
			return nil, err
		}
		txHashes = append(txHashes, txHash)
	}
	return txHashes, nil
}

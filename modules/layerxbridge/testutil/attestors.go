package testutil

import (
	"bytes"
	"crypto/ecdsa"
	"sort"

	"github.com/ethereum/go-ethereum/crypto"
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// Attestor is a secp256k1 key and the EVM address it signs as.
type Attestor struct {
	Key    *ecdsa.PrivateKey
	Signer types.Address20
}

// Attestors derives n deterministic attestor keys, ordered by ascending
// signer address.
func Attestors(n int) []Attestor {
	out := make([]Attestor, n)
	for i := range out {
		seed := make([]byte, 32)
		seed[31] = byte(i + 1)
		seed[0] = 0x5a
		key, err := crypto.ToECDSA(seed)
		if err != nil {
			panic(err)
		}
		out[i] = Attestor{Key: key, Signer: types.Address20(crypto.PubkeyToAddress(key.PublicKey))}
	}
	sort.Slice(out, func(i, j int) bool { return bytes.Compare(out[i].Signer[:], out[j].Signer[:]) < 0 })
	return out
}

// Set is the attestor set of attestors with bond each and threshold.
func Set(attestors []Attestor, bond int64, threshold uint32) types.AttestorSet {
	set := types.AttestorSet{Threshold: threshold}
	for _, attestor := range attestors {
		set.Attestors = append(set.Attestors, types.Attestor{Signer: attestor.Signer, Bond: sdk.NewInt(bond)})
	}
	return set
}

// Sign returns each attestor's 65-byte r || s || v signature of digest with
// v in {27, 28}, in the order given.
func Sign(digest types.Hash32, attestors ...Attestor) [][]byte {
	out := make([][]byte, len(attestors))
	for i, attestor := range attestors {
		signature, err := crypto.Sign(digest[:], attestor.Key)
		if err != nil {
			panic(err)
		}
		signature[64] += 27
		out[i] = signature
	}
	return out
}

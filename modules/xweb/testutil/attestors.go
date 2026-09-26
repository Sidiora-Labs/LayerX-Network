package testutil

import (
	"bytes"
	"crypto/ecdsa"
	"sort"

	"github.com/ethereum/go-ethereum/crypto"
	"github.com/sidiora-labs/paxeer-network/modules/xweb/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// Attestor is a secp256k1 key, the EVM address it signs as and the bank
// account its fee share is paid to.
type Attestor struct {
	Key    *ecdsa.PrivateKey
	Signer types.Address20
	Payout sdk.AccAddress
}

// Attestors derives n deterministic attestor keys and payout accounts,
// ordered by ascending signer address.
func Attestors(n int) []Attestor {
	out := make([]Attestor, n)
	for i := range out {
		seed := make([]byte, 32)
		seed[31] = byte(i + 1)
		seed[0] = 0x3c
		key, err := crypto.ToECDSA(seed)
		if err != nil {
			panic(err)
		}
		payout := make([]byte, 20)
		payout[0] = 0xb0
		payout[19] = byte(i + 1)
		out[i] = Attestor{Key: key, Signer: types.Address20(crypto.PubkeyToAddress(key.PublicKey)), Payout: sdk.AccAddress(payout)}
	}
	sort.Slice(out, func(i, j int) bool { return bytes.Compare(out[i].Signer[:], out[j].Signer[:]) < 0 })
	return out
}

// Registration is the attestor record governance registers for attestor.
func (a Attestor) Registration() types.Attestor {
	return types.Attestor{Signer: a.Signer, Payout: a.Payout.String()}
}

// Set is the attestor set of attestors with threshold.
func Set(attestors []Attestor, threshold uint32) types.AttestorSet {
	set := types.AttestorSet{Threshold: threshold}
	for _, attestor := range attestors {
		set.Attestors = append(set.Attestors, attestor.Registration())
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

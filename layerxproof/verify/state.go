package verify

import (
	"errors"

	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
)

var (
	ErrStateRootBinding = errors.New("layerx verify: state root is not the header resulting root")
	ErrNotAccountProof  = errors.New("layerx verify: witness does not prove an account leaf")
	ErrAssetIdentity    = errors.New("layerx verify: account asset identity")
)

// StateProof decodes a native state witness and proves its key/value under the
// trusted composite state root of a checkpoint.
func StateProof(witnessBytes []byte, stateRoot [32]byte) (*codec.StateWitness, error) {
	witness, err := codec.DecodeStateWitness(witnessBytes)
	if err != nil {
		return nil, err
	}
	if err := witness.Verify(stateRoot); err != nil {
		return nil, err
	}
	return witness, nil
}

// StateProofAtHeader proves a state witness under the resulting state root of
// a sequencer-signed batch header.
func StateProofAtHeader(witnessBytes []byte, headerBytes []byte, headerSignature [64]byte,
	authorization SequencerAuthorization) (*codec.StateWitness, *VerifiedBatchHeader, error) {
	header, err := BatchHeader(headerBytes, headerSignature, authorization)
	if err != nil {
		return nil, nil, err
	}
	witness, err := StateProof(witnessBytes, header.Header.ResultingStateRoot)
	if err != nil {
		return nil, nil, err
	}
	return witness, header, nil
}

// AccountProof proves one canonical account under a trusted state root and
// binds the decoded value to expectedAccount and, when given, expectedAsset.
func AccountProof(witnessBytes []byte, stateRoot [32]byte, expectedAccount [32]byte, expectedAsset *[32]byte) (*codec.Account, error) {
	witness, err := StateProof(witnessBytes, stateRoot)
	if err != nil {
		return nil, err
	}
	if witness.AccountPath == nil {
		return nil, ErrNotAccountProof
	}
	var keyed [32]byte
	copy(keyed[:], witness.Key[1:])
	if keyed != expectedAccount {
		return nil, codec.ErrAccountIdentity
	}
	account, err := codec.DecodeAccountValue(expectedAccount, witness.Value)
	if err != nil {
		return nil, err
	}
	if expectedAsset != nil && (!account.HasAsset || account.AssetID != *expectedAsset) {
		return nil, ErrAssetIdentity
	}
	return account, nil
}

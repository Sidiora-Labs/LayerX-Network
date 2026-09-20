package testvectors

import (
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strconv"
)

// PaxeerBind is one account-binding case emitted by
// agent/crates/layerx-client/examples/paxeer_bind_vectors.rs. Valid is the
// verdict of the Rust strict verifier for Signature over Message.
type PaxeerBind struct {
	Name       string
	ChainID    uint64
	EVMAddress [20]byte
	Nonce      uint64
	Message    []byte
	Signature  [64]byte
	Valid      bool
}

// PaxeerBindFixture is the whole binding fixture: one DID key, the main
// account identifier the Rust wire crate derives for it, and the bind cases.
type PaxeerBindFixture struct {
	PublicKey       [32]byte
	Did             string
	MainAccountName string
	MainAccountID   [32]byte
	Binds           []PaxeerBind
}

func decodeExact(text string, out []byte) error {
	raw, err := hex.DecodeString(text)
	if err != nil {
		return err
	}
	if len(raw) != len(out) {
		return fmt.Errorf("testvectors: %d bytes where %d are required", len(raw), len(out))
	}
	copy(out, raw)
	return nil
}

// LoadPaxeerBind reads layerxproof/testdata/paxeer_bind_vectors.json.
func LoadPaxeerBind() (*PaxeerBindFixture, error) {
	root, err := RepositoryRoot()
	if err != nil {
		return nil, err
	}
	raw, err := os.ReadFile(filepath.Join(root, "layerxproof", "testdata", "paxeer_bind_vectors.json"))
	if err != nil {
		return nil, err
	}
	var document struct {
		PublicKey       string `json:"public_key"`
		Did             string `json:"did"`
		MainAccountName string `json:"main_account_name"`
		MainAccountID   string `json:"main_account_id"`
		Binds           []struct {
			Name       string `json:"name"`
			ChainID    string `json:"chain_id"`
			EVMAddress string `json:"evm_address"`
			Nonce      string `json:"nonce"`
			Message    string `json:"message"`
			Signature  string `json:"signature"`
			Valid      bool   `json:"valid"`
		} `json:"binds"`
	}
	if err := json.Unmarshal(raw, &document); err != nil {
		return nil, err
	}
	fixture := &PaxeerBindFixture{Did: document.Did, MainAccountName: document.MainAccountName}
	if err := decodeExact(document.PublicKey, fixture.PublicKey[:]); err != nil {
		return nil, err
	}
	if err := decodeExact(document.MainAccountID, fixture.MainAccountID[:]); err != nil {
		return nil, err
	}
	for _, entry := range document.Binds {
		bind := PaxeerBind{Name: entry.Name, Valid: entry.Valid}
		if bind.ChainID, err = strconv.ParseUint(entry.ChainID, 10, 64); err != nil {
			return nil, err
		}
		if bind.Nonce, err = strconv.ParseUint(entry.Nonce, 10, 64); err != nil {
			return nil, err
		}
		if err := decodeExact(entry.EVMAddress, bind.EVMAddress[:]); err != nil {
			return nil, err
		}
		if err := decodeExact(entry.Signature, bind.Signature[:]); err != nil {
			return nil, err
		}
		if bind.Message, err = hex.DecodeString(entry.Message); err != nil {
			return nil, err
		}
		fixture.Binds = append(fixture.Binds, bind)
	}
	if len(fixture.Binds) == 0 {
		return nil, fmt.Errorf("testvectors: paxeer bind fixture has no cases")
	}
	return fixture, nil
}

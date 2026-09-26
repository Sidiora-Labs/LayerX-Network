package proposals

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"

	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
)

// AttestorManifest is the single committed record of the bridge's attestor
// addresses and the number of signatures a bridgeIn needs. It carries
// addresses and a threshold and nothing else: no key material, no endpoint and
// no operator identity. Every chain configuration is checked against it, so a
// chain cannot be opened with a different set by accident.
type AttestorManifest struct {
	Attestors []string `json:"attestors"`
	Threshold uint32   `json:"threshold"`

	source  string
	signers []types.Address20
}

// LoadAttestorManifest reads the manifest and refuses an unknown field,
// anything after the document, a zero, placeholder, duplicated or descending
// attestor and a threshold of zero or above the attestor count. The committed
// manifest carries placeholder addresses, so it is refused until the real set
// is recorded in it.
func LoadAttestorManifest(path string) (AttestorManifest, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		return AttestorManifest{}, err
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var manifest AttestorManifest
	if err := decoder.Decode(&manifest); err != nil {
		return AttestorManifest{}, refuse(path, fieldDocument, "%v", err)
	}
	if decoder.More() {
		return AttestorManifest{}, refuse(path, fieldDocument, "more than one JSON document")
	}
	signers, err := parseSignerList(path, manifest.Attestors)
	if err != nil {
		return AttestorManifest{}, err
	}
	if err := checkThreshold(path, manifest.Threshold, len(signers)); err != nil {
		return AttestorManifest{}, err
	}
	manifest.source = path
	manifest.signers = signers
	return manifest, nil
}

// Signers is the manifest's attestor set in the order it records it.
func (m AttestorManifest) Signers() []types.Address20 {
	out := make([]types.Address20, len(m.signers))
	copy(out, m.signers)
	return out
}

func (m AttestorManifest) file() string {
	if m.source == "" {
		return "attestor manifest"
	}
	return m.source
}

// Check refuses a chain configuration whose attestor set or threshold differs
// from the manifest, in membership, in order or in count.
func (m AttestorManifest) Check(cfg ChainConfig) error {
	signers, err := parseSignerList(cfg.file(), cfg.Attestors)
	if err != nil {
		return err
	}
	if len(signers) != len(m.signers) {
		return refuse(cfg.file(), fieldAttestors,
			"%d attestors against the %d %s records", len(signers), len(m.signers), m.file())
	}
	for i, signer := range signers {
		if signer != m.signers[i] {
			return refuse(cfg.file(), fmt.Sprintf("%s[%d]", fieldAttestors, i),
				"attestor %s against %s in %s", signer.Hex(), m.signers[i].Hex(), m.file())
		}
	}
	if cfg.Threshold != m.Threshold {
		return refuse(cfg.file(), fieldThreshold,
			"threshold %d against the threshold %d in %s", cfg.Threshold, m.Threshold, m.file())
	}
	return nil
}

// parseSignerList parses an attestor list and refuses an empty list, a list
// above the module's bound, a zero or placeholder attestor and any attestor
// that does not strictly follow the one before it, so a set has exactly one
// canonical ordering and cannot repeat a signer.
func parseSignerList(file string, texts []string) ([]types.Address20, error) {
	if len(texts) == 0 {
		return nil, refuse(file, fieldAttestors, "no attestors")
	}
	if len(texts) > types.MaxAttestors {
		return nil, refuse(file, fieldAttestors, "%d attestors, more than the %d the module accepts",
			len(texts), types.MaxAttestors)
	}
	signers := make([]types.Address20, 0, len(texts))
	for i, text := range texts {
		field := fmt.Sprintf("%s[%d]", fieldAttestors, i)
		signer, err := liveAddress(file, field, text)
		if err != nil {
			return nil, err
		}
		if i > 0 && bytes.Compare(signer[:], signers[i-1][:]) <= 0 {
			return nil, refuse(file, field, "attestor %s does not follow %s in ascending order",
				signer.Hex(), signers[i-1].Hex())
		}
		signers = append(signers, signer)
	}
	return signers, nil
}

func checkThreshold(file string, threshold uint32, count int) error {
	if threshold == 0 {
		return refuse(file, fieldThreshold, "threshold is zero")
	}
	if int(threshold) > count {
		return refuse(file, fieldThreshold, "threshold %d is above the attestor count %d", threshold, count)
	}
	return nil
}

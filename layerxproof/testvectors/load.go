// Package testvectors loads the cross-implementation fixture emitted by
// agent/crates/layerx-client/examples/go_verifier_vectors.rs. Every verdict in
// the fixture is the answer of the Rust verifier, never of this repository's
// Go code.
package testvectors

import (
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
)

// Vector is one fixture case: hex and decimal strings keyed by field name.
type Vector struct {
	Name   string
	Valid  bool
	Fields map[string]string
}

// Fixture is the whole fixture keyed by section.
type Fixture map[string][]Vector

// RepositoryRoot returns the repository root derived from this source file.
func RepositoryRoot() (string, error) {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		return "", fmt.Errorf("testvectors: caller unavailable")
	}
	return filepath.Join(filepath.Dir(file), "..", ".."), nil
}

// Load reads layerxproof/testdata/vectors.json.
func Load() (Fixture, error) { return loadFile("vectors.json") }

// LoadAnchor reads layerxproof/testdata/anchor_vectors.json, emitted by
// layerxproof/testdata/anchor_vectors.c linked against liblayerx and the
// layerxd evidence encoder. Every byte string in it was produced by the C
// implementation.
func LoadAnchor() (Fixture, error) { return loadFile("anchor_vectors.json") }

func loadFile(name string) (Fixture, error) {
	root, err := RepositoryRoot()
	if err != nil {
		return nil, err
	}
	raw, err := os.ReadFile(filepath.Join(root, "layerxproof", "testdata", name))
	if err != nil {
		return nil, err
	}
	var document map[string]json.RawMessage
	if err := json.Unmarshal(raw, &document); err != nil {
		return nil, err
	}
	fixture := Fixture{}
	for section, body := range document {
		if section == "generator" {
			continue
		}
		var cases []map[string]any
		if err := json.Unmarshal(body, &cases); err != nil {
			return nil, fmt.Errorf("testvectors: section %s: %w", section, err)
		}
		for _, entry := range cases {
			vector := Vector{Fields: map[string]string{}}
			for key, value := range entry {
				switch typed := value.(type) {
				case bool:
					if key != "valid" {
						return nil, fmt.Errorf("testvectors: %s: unexpected boolean %s", section, key)
					}
					vector.Valid = typed
				case string:
					if key == "name" {
						vector.Name = typed
					} else {
						vector.Fields[key] = typed
					}
				default:
					return nil, fmt.Errorf("testvectors: %s: unexpected type for %s", section, key)
				}
			}
			if _, ok := entry["valid"]; !ok || vector.Name == "" {
				return nil, fmt.Errorf("testvectors: %s: case without name or verdict", section)
			}
			fixture[section] = append(fixture[section], vector)
		}
	}
	return fixture, nil
}

// Bytes decodes a hex field; a missing field is an error.
func (v Vector) Bytes(key string) ([]byte, error) {
	text, ok := v.Fields[key]
	if !ok {
		return nil, fmt.Errorf("testvectors: %s: no field %s", v.Name, key)
	}
	return hex.DecodeString(text)
}

// Array32 decodes a field that must be exactly 32 bytes.
func (v Vector) Array32(key string) ([32]byte, error) {
	var out [32]byte
	raw, err := v.Bytes(key)
	if err != nil {
		return out, err
	}
	if len(raw) != 32 {
		return out, fmt.Errorf("testvectors: %s: field %s is %d bytes", v.Name, key, len(raw))
	}
	copy(out[:], raw)
	return out, nil
}

// Uint64 decodes a decimal field.
func (v Vector) Uint64(key string) (uint64, error) {
	text, ok := v.Fields[key]
	if !ok {
		return 0, fmt.Errorf("testvectors: %s: no field %s", v.Name, key)
	}
	return strconv.ParseUint(text, 10, 64)
}

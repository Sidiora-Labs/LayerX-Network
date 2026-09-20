package staticarchive

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
)

func TestPublishedArchiveProvenance(t *testing.T) {
	for _, directory := range []string{
		"../../wasm-runtime/internal/api",
		"../x/wasm/artifacts/v152/api",
		"../x/wasm/artifacts/v155/api",
	} {
		raw, err := os.ReadFile(filepath.Join(directory, "static-archives.json"))
		if err != nil {
			t.Fatal(err)
		}
		var descriptions manifest
		if err := json.Unmarshal(raw, &descriptions); err != nil {
			t.Fatal(err)
		}
		for _, archive := range descriptions.Archives {
			t.Run(directory+"/"+archive.Name, func(t *testing.T) {
				got, err := Verify(directory, archive.Name)
				if err != nil || got != archive.SHA256 {
					t.Fatalf("published original or packaged object identity: %v", err)
				}
			})
		}
	}
}

func TestArchiveProvenanceRefusals(t *testing.T) {
	directory := "../../wasm-runtime/internal/api"
	if _, err := Verify(directory, "absent.a"); err == nil {
		t.Fatal("missing archive accepted")
	}
	raw, err := os.ReadFile(filepath.Join(directory, "static-archives.json"))
	if err != nil {
		t.Fatal(err)
	}
	oversized := append(append([]byte(nil), raw...), []byte(strings.Repeat(" ", 64*1024))...)
	manifestRoot := t.TempDir()
	if err := os.WriteFile(filepath.Join(manifestRoot, "static-archives.json"), oversized, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := Verify(manifestRoot, "libwasmvm_muslc.a"); err == nil {
		t.Fatal("oversized archive manifest accepted")
	}
	var descriptions manifest
	if err := json.Unmarshal(raw, &descriptions); err != nil {
		t.Fatal(err)
	}
	entry := descriptions.Archives[0]
	compressed, err := os.ReadFile(filepath.Join(directory, entry.Name+".gz"))
	if err != nil {
		t.Fatal(err)
	}
	for _, index := range []int{0, len(compressed) / 2, len(compressed) - 1} {
		changed := append([]byte(nil), compressed...)
		changed[index] ^= 1
		root := t.TempDir()
		if err := os.WriteFile(filepath.Join(root, "static-archives.json"), raw, 0600); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(root, entry.Name+".gz"), changed, 0600); err != nil {
			t.Fatal(err)
		}
		if _, err := Verify(root, entry.Name); err == nil || !strings.Contains(err.Error(), "digest mismatch") {
			t.Fatalf("modified compressed original accepted: %v", err)
		}
	}
	part := entry.Parts[0]
	data, err := os.ReadFile(filepath.Join(directory, part.Name))
	if err != nil {
		t.Fatal(err)
	}
	for _, size := range []int{0, 7, 8, 67, len(data) - 1} {
		if _, err := archiveMembers(data[:size]); err == nil {
			t.Fatalf("truncated archive accepted at %d", size)
		}
	}
	changed := append([]byte(nil), data...)
	changed[8+48] = 'x'
	if _, err := archiveMembers(changed); err == nil {
		t.Fatal("invalid member length accepted")
	}
}

func modifiedIndexRefused(t *testing.T, modify func([]byte)) {
	t.Helper()
	directory := "../../wasm-runtime/internal/api"
	raw, err := os.ReadFile(filepath.Join(directory, "static-archives.json"))
	if err != nil {
		t.Fatal(err)
	}
	var descriptions manifest
	if err := json.Unmarshal(raw, &descriptions); err != nil {
		t.Fatal(err)
	}
	entry := &descriptions.Archives[0]
	root := t.TempDir()
	files := []string{entry.Name, entry.Name + ".gz"}
	for _, part := range entry.Parts {
		files = append(files, part.Name)
	}
	for _, name := range files {
		data, err := os.ReadFile(filepath.Join(directory, name))
		if err != nil {
			t.Fatal(err)
		}
		if name == entry.Parts[0].Name {
			if string(data[8:24]) != "/               " {
				t.Fatal("expected genuine GNU symbol index")
			}
			modify(data)
			digest := sha256.Sum256(data)
			entry.Parts[0].SHA256 = hex.EncodeToString(digest[:])
		}
		if err := os.WriteFile(filepath.Join(root, name), data, 0600); err != nil {
			t.Fatal(err)
		}
	}
	changed, err := json.Marshal(descriptions)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "static-archives.json"), changed, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := Verify(root, entry.Name); err == nil || !strings.Contains(err.Error(), "symbol index") {
		t.Fatalf("retargeted symbol index accepted despite unchanged original object bytes: %v", err)
	}
}

func TestArchiveSymbolBindingRefusesRetargetedIndex(t *testing.T) {
	modifiedIndexRefused(t, func(data []byte) {
		count := int(binary.BigEndian.Uint32(data[68:72]))
		first := binary.BigEndian.Uint32(data[72:76])
		replaced := false
		for index := 1; index < count; index++ {
			other := binary.BigEndian.Uint32(data[72+4*index : 76+4*index])
			if other != first {
				binary.BigEndian.PutUint32(data[72:76], other)
				replaced = true
				break
			}
		}
		if !replaced {
			t.Fatal("genuine archive must index multiple objects")
		}
	})
}

func TestArchiveSymbolBindingRefusesDuplicatePermutation(t *testing.T) {
	modifiedIndexRefused(t, func(data []byte) {
		size, err := strconv.Atoi(strings.TrimSpace(string(data[56:66])))
		if err != nil {
			t.Fatal(err)
		}
		table := data[68 : 68+size]
		count := int(binary.BigEndian.Uint32(table[:4]))
		nameOffset := 4 + 4*count
		seen := make(map[string]int)
		objectDigest := func(offset uint32) [32]byte {
			position := int(offset)
			size, err := strconv.Atoi(strings.TrimSpace(string(data[position+48 : position+58])))
			if err != nil {
				t.Fatal(err)
			}
			return sha256.Sum256(data[position+60 : position+60+size])
		}
		for index := 0; index < count; index++ {
			end := bytes.IndexByte(table[nameOffset:], 0)
			if end <= 0 {
				t.Fatal("genuine archive symbol name missing")
			}
			name := string(table[nameOffset : nameOffset+end])
			nameOffset += end + 1
			if first, ok := seen[name]; ok {
				left, right := table[4+4*first:8+4*first], table[4+4*index:8+4*index]
				leftOffset, rightOffset := binary.BigEndian.Uint32(left), binary.BigEndian.Uint32(right)
				if objectDigest(leftOffset) != objectDigest(rightOffset) {
					binary.BigEndian.PutUint32(left, rightOffset)
					binary.BigEndian.PutUint32(right, leftOffset)
					return
				}
			} else {
				seen[name] = index
			}
		}
		t.Fatal("genuine archive must have duplicate symbols bound to different objects")
	})
}

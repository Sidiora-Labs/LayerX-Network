package staticarchive

import (
	"bytes"
	"compress/gzip"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"
)

const publicationLimit = 50 * 1024 * 1024
const originalLimit = 128 * 1024 * 1024

type part struct {
	Name         string `json:"name"`
	Size         int64  `json:"size"`
	SHA256       string `json:"sha256"`
	Architecture uint32 `json:"architecture"`
}

type archive struct {
	Name             string `json:"name"`
	Size             int64  `json:"size"`
	SHA256           string `json:"sha256"`
	CompressedSize   int64  `json:"compressed_size"`
	CompressedSHA256 string `json:"compressed_sha256"`
	Kind             string `json:"kind"`
	Parts            []part `json:"parts"`
}

type manifest struct {
	Version  int       `json:"version"`
	Archives []archive `json:"archives"`
}

type member struct {
	name   string
	length int
	digest [32]byte
}

func boundedFile(directory, name string, size, maximum int64, digest string) ([]byte, error) {
	if name == "" || filepath.Base(name) != name || size <= 0 || size > maximum {
		return nil, fmt.Errorf("invalid archive path or size: %q", name)
	}
	path := filepath.Join(directory, name)
	info, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || info.Size() != size {
		return nil, fmt.Errorf("archive size or file type mismatch: %s", name)
	}
	raw, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	want, err := hex.DecodeString(digest)
	got := sha256.Sum256(raw)
	if err != nil || len(want) != 32 || !bytes.Equal(got[:], want) {
		return nil, fmt.Errorf("archive digest mismatch: %s", name)
	}
	return raw, nil
}

func archiveMembers(raw []byte) ([]member, error) {
	if len(raw) < 8 || string(raw[:8]) != "!<arch>\n" {
		return nil, fmt.Errorf("archive magic mismatch")
	}
	var members []member
	var names []byte
	for offset := 8; offset < len(raw); {
		if len(raw)-offset < 60 || string(raw[offset+58:offset+60]) != "`\n" {
			return nil, fmt.Errorf("archive member header is truncated")
		}
		header := raw[offset : offset+60]
		size, err := strconv.Atoi(strings.TrimSpace(string(header[48:58])))
		if err != nil || size < 0 || size > len(raw)-offset-60 {
			return nil, fmt.Errorf("archive member size is invalid")
		}
		body := raw[offset+60 : offset+60+size]
		offset += 60 + size + size%2
		if offset > len(raw) {
			return nil, fmt.Errorf("archive member padding is truncated")
		}
		name := strings.TrimSpace(string(header[:16]))
		switch {
		case name == "//":
			names = body
			continue
		case name == "/" || name == "/SYM64/":
			continue
		case strings.HasPrefix(name, "#1/"):
			width, err := strconv.Atoi(name[3:])
			if err != nil || width < 0 || width > len(body) {
				return nil, fmt.Errorf("archive extended name is invalid")
			}
			name, body = strings.TrimRight(string(body[:width]), "\x00"), body[width:]
		case strings.HasPrefix(name, "/"):
			index, err := strconv.Atoi(name[1:])
			if err != nil || index < 0 || index >= len(names) {
				return nil, fmt.Errorf("archive name offset is invalid")
			}
			end := bytes.Index(names[index:], []byte("/\n"))
			if end < 0 {
				return nil, fmt.Errorf("archive name terminator is missing")
			}
			name = string(names[index : index+end])
		default:
			name = strings.TrimSuffix(name, "/")
		}
		if strings.HasPrefix(name, "__.SYMDEF") {
			continue
		}
		if name == "" || len(body) == 0 {
			return nil, fmt.Errorf("archive member is empty")
		}
		members = append(members, member{name, len(body), sha256.Sum256(body)})
	}
	if len(members) == 0 {
		return nil, fmt.Errorf("archive has no object members")
	}
	return members, nil
}

func archiveSymbols(raw []byte) (map[string][][32]byte, error) {
	if _, err := archiveMembers(raw); err != nil {
		return nil, err
	}
	objects := make(map[uint64][32]byte)
	var table []byte
	width, bsd := 4, false
	for offset := 8; offset < len(raw); {
		header := raw[offset : offset+60]
		size, err := strconv.Atoi(strings.TrimSpace(string(header[48:58])))
		if err != nil {
			return nil, err
		}
		body := raw[offset+60 : offset+60+size]
		name := strings.TrimSpace(string(header[:16]))
		if strings.HasPrefix(name, "#1/") {
			length, err := strconv.Atoi(name[3:])
			if err != nil {
				return nil, err
			}
			name, body = strings.TrimRight(string(body[:length]), "\x00"), body[length:]
		}
		switch {
		case name == "/" || name == "/SYM64/" || strings.HasPrefix(name, "__.SYMDEF"):
			if table != nil {
				return nil, fmt.Errorf("duplicate archive symbol index")
			}
			table = body
			bsd = strings.HasPrefix(name, "__.SYMDEF")
			if name == "/SYM64/" || strings.Contains(name, "_64") {
				width = 8
			}
		case name == "//":
		default:
			objects[uint64(offset)] = sha256.Sum256(body)
		}
		offset += 60 + size + size%2
	}
	if len(table) < width {
		return nil, fmt.Errorf("archive symbol index is missing or truncated")
	}
	read := func(data []byte) uint64 {
		if bsd {
			if width == 8 {
				return binary.LittleEndian.Uint64(data)
			}
			return uint64(binary.LittleEndian.Uint32(data))
		}
		if width == 8 {
			return binary.BigEndian.Uint64(data)
		}
		return uint64(binary.BigEndian.Uint32(data))
	}
	count := read(table[:width])
	stride := width
	if bsd {
		stride = 2 * width
		if count%uint64(stride) != 0 {
			return nil, fmt.Errorf("archive symbol index table size is invalid")
		}
		count /= uint64(stride)
	}
	if count > uint64((len(table)-width)/stride) {
		return nil, fmt.Errorf("archive symbol index count is invalid")
	}
	stringsOffset := width + int(count)*stride
	stringsLength := len(table) - stringsOffset
	if bsd {
		if stringsLength < width {
			return nil, fmt.Errorf("archive symbol index string size is missing")
		}
		length := read(table[stringsOffset : stringsOffset+width])
		stringsOffset += width
		if length > uint64(len(table)-stringsOffset) {
			return nil, fmt.Errorf("archive symbol index strings are truncated")
		}
		stringsLength = int(length)
	}
	names := table[stringsOffset : stringsOffset+stringsLength]
	bindings := make(map[string][][32]byte)
	nameOffset := uint64(0)
	for index := 0; index < int(count); index++ {
		entry := table[width+index*stride : width+(index+1)*stride]
		if bsd {
			nameOffset, entry = read(entry[:width]), entry[width:]
		}
		if nameOffset >= uint64(len(names)) {
			return nil, fmt.Errorf("archive symbol index name offset is invalid")
		}
		end := bytes.IndexByte(names[nameOffset:], 0)
		if end <= 0 {
			return nil, fmt.Errorf("archive symbol index name is empty or unterminated")
		}
		object, exists := objects[read(entry)]
		if !exists {
			return nil, fmt.Errorf("archive symbol index references a missing object")
		}
		name := string(names[nameOffset : nameOffset+uint64(end)])
		bindings[name] = append(bindings[name], object)
		if !bsd {
			nameOffset += uint64(end) + 1
		}
	}
	return bindings, nil
}

func Verify(directory, name string) (string, error) {
	path := filepath.Join(directory, "static-archives.json")
	info, err := os.Lstat(path)
	if err != nil {
		return "", err
	}
	if !info.Mode().IsRegular() || info.Size() <= 0 || info.Size() > 64*1024 {
		return "", fmt.Errorf("archive manifest size or file type mismatch")
	}
	raw, err := os.ReadFile(path)
	if err != nil {
		return "", err
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var descriptions manifest
	if err := decoder.Decode(&descriptions); err != nil {
		return "", err
	}
	if descriptions.Version != 1 || len(descriptions.Archives) != 3 {
		return "", fmt.Errorf("archive manifest version or count mismatch")
	}
	var trailing any
	if err := decoder.Decode(&trailing); err != io.EOF {
		return "", fmt.Errorf("archive manifest has trailing data")
	}
	var selected *archive
	for index := range descriptions.Archives {
		candidate := &descriptions.Archives[index]
		if candidate.Name == name {
			if selected != nil {
				return "", fmt.Errorf("duplicate archive name")
			}
			selected = candidate
		}
	}
	if selected == nil || selected.Size <= 0 || selected.Size > originalLimit ||
		len(selected.Parts) == 0 || len(selected.Parts) > 8 {
		return "", fmt.Errorf("archive manifest entry or bound mismatch")
	}
	compressed, err := boundedFile(directory, name+".gz", selected.CompressedSize, publicationLimit, selected.CompressedSHA256)
	if err != nil {
		return "", err
	}
	reader, err := gzip.NewReader(bytes.NewReader(compressed))
	if err != nil {
		return "", err
	}
	original, err := io.ReadAll(io.LimitReader(reader, selected.Size+1))
	closeErr := reader.Close()
	if err != nil || closeErr != nil || int64(len(original)) != selected.Size {
		return "", fmt.Errorf("original archive decompression or size mismatch")
	}
	digest := sha256.Sum256(original)
	if hex.EncodeToString(digest[:]) != selected.SHA256 {
		return "", fmt.Errorf("original archive digest mismatch")
	}
	var actual []member
	actualSymbols := make(map[string][][32]byte)
	seen := make(map[string]bool)
	for index, part := range selected.Parts {
		if seen[part.Name] {
			return "", fmt.Errorf("duplicate archive part")
		}
		seen[part.Name] = true
		data, err := boundedFile(directory, part.Name, part.Size, publicationLimit, part.SHA256)
		if err != nil {
			return "", err
		}
		if selected.Kind == "fat" {
			if len(original) < 8 || binary.BigEndian.Uint32(original[:4]) != 0xcafebabe ||
				int(binary.BigEndian.Uint32(original[4:8])) != len(selected.Parts) ||
				len(original) < 8+20*len(selected.Parts) {
				return "", fmt.Errorf("fat archive header mismatch")
			}
			slice := original[8+20*index : 8+20*(index+1)]
			offset, size := uint64(binary.BigEndian.Uint32(slice[8:12])), uint64(binary.BigEndian.Uint32(slice[12:16]))
			if part.Architecture != binary.BigEndian.Uint32(slice[:4]) || offset > uint64(len(original)) ||
				size > uint64(len(original))-offset || !bytes.Equal(data, original[offset:offset+size]) {
				return "", fmt.Errorf("fat archive slice identity mismatch")
			}
		} else if part.Architecture != 0 {
			return "", fmt.Errorf("archive part architecture mismatch")
		}
		objects, err := archiveMembers(data)
		if err != nil {
			return "", err
		}
		actual = append(actual, objects...)
		if selected.Kind != "fat" {
			bindings, err := archiveSymbols(data)
			if err != nil {
				return "", err
			}
			for name, objects := range bindings {
				actualSymbols[name] = append(actualSymbols[name], objects...)
			}
		}
	}
	switch selected.Kind {
	case "group", "darwin":
		expected, err := archiveMembers(original)
		if err != nil || len(expected) != len(actual) {
			return "", fmt.Errorf("archive member count mismatch")
		}
		for index := range expected {
			if expected[index] != actual[index] {
				return "", fmt.Errorf("archive member bytes or ordering changed at %d", index)
			}
		}
		expectedSymbols, err := archiveSymbols(original)
		if err != nil || len(expectedSymbols) != len(actualSymbols) {
			return "", fmt.Errorf("archive symbol index count mismatch")
		}
		for name, objects := range expectedSymbols {
			actual := actualSymbols[name]
			if len(actual) != len(objects) {
				return "", fmt.Errorf("archive symbol index binding count changed")
			}
			for index, object := range objects {
				if actual[index] != object {
					return "", fmt.Errorf("archive symbol index binding or order changed")
				}
			}
		}
		if selected.Kind == "group" {
			var libraries []string
			for _, part := range selected.Parts {
				libraries = append(libraries, "-l:"+part.Name)
			}
			want := "GROUP ( " + strings.Join(libraries, " ") + " )\n"
			script, err := os.ReadFile(filepath.Join(directory, name))
			if err != nil || string(script) != want {
				return "", fmt.Errorf("archive linker group ordering mismatch")
			}
		}
	case "fat":
	default:
		return "", fmt.Errorf("unsupported archive packaging kind")
	}
	return selected.SHA256, nil
}

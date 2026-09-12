package custodyproof

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
)

func jsonValue(decoder *json.Decoder, depth int) error {
	if depth > 64 { return errors.New("JSON depth bound") }
	token, err := decoder.Token()
	if err != nil { return err }
	delimiter, nested := token.(json.Delim)
	if !nested { return nil }
	switch delimiter {
	case '{':
		seen := make(map[string]bool)
		for decoder.More() {
			name, err := decoder.Token()
			if err != nil { return err }
			key, valid := name.(string)
			if !valid || seen[key] { return errors.New("duplicate or invalid JSON field") }
			seen[key] = true
			if err := jsonValue(decoder, depth+1); err != nil { return err }
		}
	case '[':
		for decoder.More() {
			if err := jsonValue(decoder, depth+1); err != nil { return err }
		}
	default:
		return errors.New("invalid JSON delimiter")
	}
	_, err = decoder.Token()
	return err
}

func Decode(input []byte) (*Request, error) {
	if len(input) == 0 || len(input) > MaxInputBytes { return nil, errors.New("proof request size") }
	decoder := json.NewDecoder(bytes.NewReader(input))
	decoder.UseNumber()
	if err := jsonValue(decoder, 0); err != nil { return nil, err }
	if _, err := decoder.Token(); err != io.EOF { return nil, errors.New("trailing JSON value") }
	decoder = json.NewDecoder(bytes.NewReader(input))
	decoder.DisallowUnknownFields()
	var request Request
	if err := decoder.Decode(&request); err != nil { return nil, err }
	return &request, nil
}

package types_test

import (
	"bytes"
	"encoding/json"
	"os"
	"strings"
	"testing"

	"github.com/ethereum/go-ethereum/common/hexutil"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/sidiora-labs/paxeer-network/modules/xweb/types"
	"github.com/stretchr/testify/require"
)

type apiVectorHeader struct {
	Name  string `json:"name"`
	Value string `json:"value"`
}

type apiVector struct {
	Name        string            `json:"name"`
	Method      string            `json:"method"`
	Level       uint8             `json:"level"`
	Attestor    string            `json:"attestor"`
	URL         string            `json:"url"`
	Origin      string            `json:"origin"`
	Headers     []apiVectorHeader `json:"headers"`
	Body        string            `json:"body"`
	Pointers    []string          `json:"pointers"`
	Envelopes   []string          `json:"envelopes"`
	Payload     string            `json:"payload"`
	PayloadHash string            `json:"payload_hash"`
}

type apiRefusal struct {
	Name    string `json:"name"`
	Payload string `json:"payload"`
	Refuses string `json:"refuses"`
}

type apiVectorFile struct {
	Kind              uint8             `json:"kind"`
	Version           uint8             `json:"version"`
	Methods           map[string]uint8  `json:"methods"`
	Levels            map[string]uint8  `json:"levels"`
	Layout            []string          `json:"layout"`
	Limits            map[string]uint32 `json:"limits"`
	RestrictedHeaders []string          `json:"restricted_headers"`
	Vectors           []apiVector       `json:"vectors"`
	Refusals          []apiRefusal      `json:"refusals"`
}

func loadApiVectors(t *testing.T) apiVectorFile {
	t.Helper()
	raw, err := os.ReadFile("testdata/api-vectors.json")
	require.NoError(t, err)
	var file apiVectorFile
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	require.NoError(t, decoder.Decode(&file))
	return file
}

func vectorAddress(t *testing.T, hex string) types.Address20 {
	t.Helper()
	raw, err := hexutil.Decode(hex)
	require.NoError(t, err)
	require.Len(t, raw, 20)
	var out types.Address20
	copy(out[:], raw)
	return out
}

func apiHeaders(headers []apiVectorHeader) []types.ApiHeader {
	out := []types.ApiHeader{}
	for _, header := range headers {
		out = append(out, types.ApiHeader{Name: header.Name, Value: header.Value})
	}
	return out
}

func TestApiVectorLimitsMatchTheCodec(t *testing.T) {
	file := loadApiVectors(t)
	require.Equal(t, types.KindApi, file.Kind)
	require.Equal(t, types.ApiVersion, file.Version)
	require.Equal(t, map[string]uint8{"GET": types.MethodGet, "POST": types.MethodPost}, file.Methods)
	require.Equal(t, map[string]uint8{"majority": types.LevelMajority, "single": types.LevelSingle}, file.Levels)
	require.Equal(t, map[string]uint32{
		"url_bytes":           types.MaxApiURLBytes,
		"headers":             types.MaxApiHeaders,
		"header_name_bytes":   types.MaxApiHeaderNameBytes,
		"header_value_bytes":  types.MaxApiHeaderValueBytes,
		"body_bytes":          types.MaxApiBodyBytes,
		"pointers":            types.MaxApiPointers,
		"pointer_bytes":       types.MaxApiPointerBytes,
		"envelopes":           types.MaxAttestors,
		"credential_headers":  types.MaxCredentialHeaders,
		"credential_bytes":    types.MaxCredentialBytes,
		"default_payload_cap": types.DefaultMaxPayloadBytes,
	}, file.Limits)
	require.Equal(t, uint32(8192), types.DefaultMaxPayloadBytes)
	require.NotEmpty(t, file.Layout)
	for _, name := range file.RestrictedHeaders {
		payload := types.ApiPayload{Method: types.MethodGet, URL: "https://paxeer.app/",
			Headers: []types.ApiHeader{{Name: strings.ToUpper(name), Value: "x"}}}
		require.ErrorContains(t, payload.Validate(), "sidecar sets itself", name)
	}
}

// TestApiVectors decodes every vector, checks each field and re-encodes it
// byte for byte.
func TestApiVectors(t *testing.T) {
	file := loadApiVectors(t)
	envelopes := map[string]string{}
	for _, v := range loadEnvelopeVectors(t).Vectors {
		envelopes[v.Name] = v.Envelope
	}
	require.Len(t, file.Vectors, 4)
	for _, v := range file.Vectors {
		t.Run(v.Name, func(t *testing.T) {
			raw, err := hexutil.Decode(v.Payload)
			require.NoError(t, err)
			require.Equal(t, v.PayloadHash, types.Keccak(raw).Hex())

			decoded, err := types.DecodeApiPayload(raw)
			require.NoError(t, err)
			method, err := types.MethodName(decoded.Method)
			require.NoError(t, err)
			require.Equal(t, v.Method, method)
			require.Equal(t, file.Methods[v.Method], decoded.Method)
			require.Equal(t, v.Level, decoded.Level)
			require.Equal(t, vectorAddress(t, v.Attestor), decoded.Attestor)
			require.Equal(t, v.URL, decoded.URL)
			require.Equal(t, apiHeaders(v.Headers), decoded.Headers)
			require.Equal(t, v.Body, string(decoded.Body))
			require.Equal(t, append([]string{}, v.Pointers...), append([]string{}, decoded.Pointers...))
			require.Len(t, decoded.Envelopes, len(v.Envelopes))
			for i, name := range v.Envelopes {
				require.Contains(t, envelopes, name)
				require.Equal(t, envelopes[name], hexutil.Encode(decoded.Envelopes[i].Bytes()))
			}
			origin, err := decoded.Origin()
			require.NoError(t, err)
			require.Equal(t, v.Origin, origin)

			built := types.ApiPayload{Method: file.Methods[v.Method], Level: v.Level, Attestor: vectorAddress(t, v.Attestor),
				URL: v.URL, Headers: apiHeaders(v.Headers), Body: []byte(v.Body), Pointers: v.Pointers}
			for _, name := range v.Envelopes {
				envelopeRaw, err := hexutil.Decode(envelopes[name])
				require.NoError(t, err)
				envelope, err := types.ParseEnvelope(envelopeRaw)
				require.NoError(t, err)
				built.Envelopes = append(built.Envelopes, envelope)
			}
			encoded, err := built.Encode()
			require.NoError(t, err)
			require.Equal(t, v.Payload, hexutil.Encode(encoded))
		})
	}
}

func TestApiRefusalVectors(t *testing.T) {
	file := loadApiVectors(t)
	require.Len(t, file.Refusals, 21)
	levelRefusals := map[string]bool{"unknown level": true, "majority naming an attestor": true,
		"single naming no attestor": true}
	for _, r := range file.Refusals {
		t.Run(r.Name, func(t *testing.T) {
			raw, err := hexutil.Decode(r.Payload)
			require.NoError(t, err)
			_, err = types.DecodeApiPayload(raw)
			if levelRefusals[r.Name] {
				require.ErrorIs(t, err, types.ErrInvalidLevel)
			} else {
				require.ErrorIs(t, err, types.ErrInvalidApi)
			}
			require.ErrorContains(t, err, r.Refuses)
		})
	}
}

func TestApiPayloadValidateRefusals(t *testing.T) {
	ok := types.ApiPayload{Method: types.MethodGet, URL: "https://paxeer.app/api?x=1"}
	require.NoError(t, ok.Validate())
	var tooManyHeaders []types.ApiHeader
	for i := 0; i <= types.MaxApiHeaders; i++ {
		tooManyHeaders = append(tooManyHeaders, types.ApiHeader{Name: "X-H" + strings.Repeat("a", i+1), Value: "v"})
	}
	var tooManyPointers []string
	for i := 0; i <= types.MaxApiPointers; i++ {
		tooManyPointers = append(tooManyPointers, "/p"+strings.Repeat("a", i+1))
	}
	cases := []struct {
		name    string
		mutate  func(*types.ApiPayload)
		refuses string
	}{
		{"url too long", func(p *types.ApiPayload) {
			p.URL = "https://paxeer.app/" + strings.Repeat("a", types.MaxApiURLBytes)
		}, "url"},
		{"no host", func(p *types.ApiPayload) { p.URL = "https:///path" }, "url"},
		{"space in url", func(p *types.ApiPayload) { p.URL = "https://paxeer.app/a b" }, "url"},
		{"body too long", func(p *types.ApiPayload) {
			p.Method = types.MethodPost
			p.Body = bytes.Repeat([]byte{'a'}, types.MaxApiBodyBytes+1)
		}, "body"},
		{"too many headers", func(p *types.ApiPayload) { p.Headers = tooManyHeaders }, "header"},
		{"header name too long", func(p *types.ApiPayload) {
			p.Headers = []types.ApiHeader{{Name: strings.Repeat("a", types.MaxApiHeaderNameBytes+1), Value: "v"}}
		}, "header"},
		{"header value too long", func(p *types.ApiPayload) {
			p.Headers = []types.ApiHeader{{Name: "X-A", Value: strings.Repeat("a", types.MaxApiHeaderValueBytes+1)}}
		}, "header"},
		{"too many pointers", func(p *types.ApiPayload) { p.Pointers = tooManyPointers }, "pointer"},
		{"pointer too long", func(p *types.ApiPayload) {
			p.Pointers = []string{"/" + strings.Repeat("a", types.MaxApiPointerBytes)}
		}, "pointer"},
		{"unknown method", func(p *types.ApiPayload) { p.Method = 3 }, "method 3"},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			p := ok
			c.mutate(&p)
			err := p.Validate()
			require.ErrorIs(t, err, types.ErrInvalidApi)
			require.ErrorContains(t, err, c.refuses)
			_, err = p.Encode()
			require.ErrorIs(t, err, types.ErrInvalidApi)
		})
	}
}

func TestApiCheckAttestors(t *testing.T) {
	keyOne := crypto.Keccak256([]byte("PAXEERX_WEB_API_ENVELOPE_V1 vector attestor 1"))
	keyTwo := crypto.Keccak256([]byte("PAXEERX_WEB_API_ENVELOPE_V1 vector attestor 2"))
	one, err := crypto.ToECDSA(keyOne)
	require.NoError(t, err)
	two, err := crypto.ToECDSA(keyTwo)
	require.NoError(t, err)
	oneAddress := types.Address20(crypto.PubkeyToAddress(one.PublicKey))
	twoAddress := types.Address20(crypto.PubkeyToAddress(two.PublicKey))

	file := loadApiVectors(t)
	var payload types.ApiPayload
	for _, v := range file.Vectors {
		if v.Name == "get-majority-select" {
			raw, err := hexutil.Decode(v.Payload)
			require.NoError(t, err)
			payload, err = types.DecodeApiPayload(raw)
			require.NoError(t, err)
		}
	}
	require.Len(t, payload.Envelopes, 2)

	both := types.AttestorSet{Attestors: []types.Attestor{{Signer: oneAddress, Payout: payout},
		{Signer: twoAddress, Payout: payout}}, Threshold: 2}
	require.NoError(t, payload.CheckAttestors(both))

	onlyOne := types.AttestorSet{Attestors: []types.Attestor{{Signer: oneAddress, Payout: payout}}, Threshold: 1}
	err = payload.CheckAttestors(onlyOne)
	require.ErrorIs(t, err, types.ErrInvalidApi)
	require.ErrorContains(t, err, "2 envelopes for 1 registered attestors")

	var third types.Address20
	third[19] = 3
	oneAndThird := types.AttestorSet{Attestors: []types.Attestor{{Signer: oneAddress, Payout: payout},
		{Signer: third, Payout: payout}}, Threshold: 2}
	err = payload.CheckAttestors(oneAndThird)
	require.ErrorIs(t, err, types.ErrUnknownAttestor)
	require.ErrorContains(t, err, "envelope 1 is addressed to "+twoAddress.Hex())

	single := types.ApiPayload{Method: types.MethodGet, Level: types.LevelSingle, Attestor: third,
		URL: "https://paxeer.app/"}
	require.NoError(t, single.Validate())
	err = single.CheckAttestors(both)
	require.ErrorIs(t, err, types.ErrUnknownAttestor)
	require.ErrorContains(t, err, "the single level names "+third.Hex())
	single.Attestor = twoAddress
	require.NoError(t, single.CheckAttestors(both))
}

func TestCredentialCodec(t *testing.T) {
	headers := []types.ApiHeader{{Name: "X-Api-Key", Value: "k"}, {Name: "X-Api-Account", Value: "a"}}
	plaintext, err := types.EncodeCredential(headers)
	require.NoError(t, err)
	want := []byte{2, 0, 9}
	want = append(want, "X-Api-Key"...)
	want = append(want, 0, 1, 'k', 0, 13)
	want = append(want, "X-Api-Account"...)
	want = append(want, 0, 1, 'a')
	require.Equal(t, want, plaintext)
	decoded, err := types.DecodeCredential(plaintext, []types.ApiHeader{{Name: "Accept", Value: "*/*"}})
	require.NoError(t, err)
	require.Equal(t, headers, decoded)

	_, err = types.DecodeCredential(plaintext, []types.ApiHeader{{Name: "x-api-key", Value: "public"}})
	require.ErrorIs(t, err, types.ErrInvalidApi)
	require.ErrorContains(t, err, "credential header X-Api-Key repeats a public header")

	_, err = types.EncodeCredential(nil)
	require.ErrorContains(t, err, "credential carries no header")
	_, err = types.DecodeCredential([]byte{0}, nil)
	require.ErrorContains(t, err, "credential carries no header")
	_, err = types.DecodeCredential(append(append([]byte(nil), plaintext...), 0), nil)
	require.ErrorContains(t, err, "1 bytes follow the last credential header")
	_, err = types.DecodeCredential(plaintext[:len(plaintext)-1], nil)
	require.ErrorContains(t, err, "ends inside")

	var tooMany []types.ApiHeader
	for i := 0; i <= types.MaxCredentialHeaders; i++ {
		tooMany = append(tooMany, types.ApiHeader{Name: "X-C" + strings.Repeat("a", i+1), Value: "v"})
	}
	_, err = types.EncodeCredential(tooMany)
	require.ErrorIs(t, err, types.ErrInvalidApi)

	large := []types.ApiHeader{{Name: "X-A", Value: strings.Repeat("a", 600)}, {Name: "X-B", Value: strings.Repeat("b", 600)}}
	_, err = types.EncodeCredential(large)
	require.ErrorContains(t, err, "bound 1024")
}

package types_test

import (
	"bytes"
	"crypto/ecdsa"
	"encoding/json"
	"os"
	"testing"

	"github.com/ethereum/go-ethereum/common/hexutil"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/sidiora-labs/paxeer-network/modules/xweb/types"
	"github.com/stretchr/testify/require"
)

type envelopeVector struct {
	Name               string            `json:"name"`
	AttestorKeyLabel   string            `json:"attestor_key_label"`
	AttestorPublicKey  string            `json:"attestor_public_key"`
	Attestor           string            `json:"attestor"`
	EphemeralKeyLabel  string            `json:"ephemeral_key_label"`
	EphemeralPublicKey string            `json:"ephemeral_public_key"`
	Nonce              string            `json:"nonce"`
	Origin             string            `json:"origin"`
	Credential         []apiVectorHeader `json:"credential"`
	Plaintext          string            `json:"plaintext"`
	SharedX            string            `json:"shared_x"`
	AESKey             string            `json:"aes_key"`
	AAD                string            `json:"aad"`
	Ciphertext         string            `json:"ciphertext"`
	Envelope           string            `json:"envelope"`
}

type envelopeRefusal struct {
	Name     string `json:"name"`
	Envelope string `json:"envelope"`
	OpenWith string `json:"open_with"`
	Origin   string `json:"origin"`
	Refuses  string `json:"refuses"`
}

type envelopeVectorFile struct {
	Scheme           string            `json:"scheme"`
	KeyDerivation    string            `json:"key_derivation"`
	Layout           string            `json:"layout"`
	SharedSecret     string            `json:"shared_secret"`
	HKDFHash         string            `json:"hkdf_hash"`
	HKDFIKM          string            `json:"hkdf_ikm"`
	HKDFSalt         string            `json:"hkdf_salt"`
	HKDFInfo         string            `json:"hkdf_info"`
	HKDFLength       int               `json:"hkdf_length"`
	AEAD             string            `json:"aead"`
	AAD              string            `json:"aad"`
	CredentialLayout string            `json:"credential_layout"`
	EnvelopeBytesMin int               `json:"envelope_bytes_min"`
	EnvelopeBytesMax int               `json:"envelope_bytes_max"`
	Vectors          []envelopeVector  `json:"vectors"`
	Refusals         []envelopeRefusal `json:"refusals"`
}

func loadEnvelopeVectors(t *testing.T) envelopeVectorFile {
	t.Helper()
	raw, err := os.ReadFile("testdata/envelope-vectors.json")
	require.NoError(t, err)
	var file envelopeVectorFile
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	require.NoError(t, decoder.Decode(&file))
	return file
}

// labelKey is the vector key keccak256(label).
func labelKey(t *testing.T, label string) *ecdsa.PrivateKey {
	t.Helper()
	key, err := crypto.ToECDSA(crypto.Keccak256([]byte(label)))
	require.NoError(t, err)
	return key
}

func TestEnvelopeVectors(t *testing.T) {
	file := loadEnvelopeVectors(t)
	require.Equal(t, types.EnvelopeInfo, file.HKDFInfo)
	require.Equal(t, 32, file.HKDFLength)
	require.Equal(t, types.EnvelopeOverhead+1, file.EnvelopeBytesMin)
	require.Equal(t, types.MaxEnvelopeBytes, file.EnvelopeBytesMax)
	require.Len(t, file.Vectors, 3)
	for _, v := range file.Vectors {
		t.Run(v.Name, func(t *testing.T) {
			attestor := labelKey(t, v.AttestorKeyLabel)
			ephemeral := labelKey(t, v.EphemeralKeyLabel)
			require.Equal(t, v.AttestorPublicKey, hexutil.Encode(crypto.CompressPubkey(&attestor.PublicKey)))
			require.Equal(t, v.Attestor, hexutil.Encode(crypto.PubkeyToAddress(attestor.PublicKey).Bytes()))
			ephemeralKey := crypto.CompressPubkey(&ephemeral.PublicKey)
			require.Equal(t, v.EphemeralPublicKey, hexutil.Encode(ephemeralKey))

			plaintext, err := types.EncodeCredential(apiHeaders(v.Credential))
			require.NoError(t, err)
			require.Equal(t, v.Plaintext, hexutil.Encode(plaintext))

			shared, err := types.EnvelopeSharedX(ephemeral, &attestor.PublicKey)
			require.NoError(t, err)
			require.Equal(t, v.SharedX, hexutil.Encode(shared))
			fromAttestor, err := types.EnvelopeSharedX(attestor, &ephemeral.PublicKey)
			require.NoError(t, err)
			require.Equal(t, shared, fromAttestor)

			aesKey, err := types.EnvelopeKey(shared, ephemeralKey)
			require.NoError(t, err)
			require.Equal(t, v.AESKey, hexutil.Encode(aesKey))
			address := types.Address20(crypto.PubkeyToAddress(attestor.PublicKey))
			require.Equal(t, v.AAD, hexutil.Encode(types.EnvelopeAAD(address, v.Origin)))

			nonceRaw, err := hexutil.Decode(v.Nonce)
			require.NoError(t, err)
			var nonce [types.EnvelopeNonceLength]byte
			require.Len(t, nonceRaw, len(nonce))
			copy(nonce[:], nonceRaw)
			sealed, err := types.SealEnvelopeWith(&attestor.PublicKey, ephemeral, nonce, v.Origin, plaintext)
			require.NoError(t, err)
			require.Equal(t, v.Envelope, hexutil.Encode(sealed))
			require.Len(t, sealed, types.EnvelopeOverhead+len(plaintext))

			parsed, err := types.ParseEnvelope(sealed)
			require.NoError(t, err)
			require.Equal(t, address, parsed.Attestor)
			require.Equal(t, v.Ciphertext, hexutil.Encode(parsed.Ciphertext))

			opened, err := types.OpenEnvelope(sealed, attestor, v.Origin)
			require.NoError(t, err)
			require.Equal(t, plaintext, opened)
			headers, err := types.DecodeCredential(opened, nil)
			require.NoError(t, err)
			require.Equal(t, apiHeaders(v.Credential), headers)
		})
	}
}

func TestEnvelopeRefusalVectors(t *testing.T) {
	file := loadEnvelopeVectors(t)
	require.Len(t, file.Refusals, 3)
	for _, r := range file.Refusals {
		t.Run(r.Name, func(t *testing.T) {
			raw, err := hexutil.Decode(r.Envelope)
			require.NoError(t, err)
			_, err = types.OpenEnvelope(raw, labelKey(t, r.OpenWith), r.Origin)
			require.ErrorIs(t, err, types.ErrInvalidEnvelope)
			require.ErrorContains(t, err, r.Refuses)
		})
	}
}

func TestSealEnvelopeRoundTrip(t *testing.T) {
	attestor, err := crypto.GenerateKey()
	require.NoError(t, err)
	plaintext, err := types.EncodeCredential([]types.ApiHeader{{Name: "Authorization", Value: "Bearer x"}})
	require.NoError(t, err)
	first, err := types.SealEnvelope(&attestor.PublicKey, "https://paxeer.app", plaintext)
	require.NoError(t, err)
	second, err := types.SealEnvelope(&attestor.PublicKey, "https://paxeer.app", plaintext)
	require.NoError(t, err)
	require.NotEqual(t, first, second)
	for _, sealed := range [][]byte{first, second} {
		opened, err := types.OpenEnvelope(sealed, attestor, "https://paxeer.app")
		require.NoError(t, err)
		require.Equal(t, plaintext, opened)
	}

	_, err = types.SealEnvelope(&attestor.PublicKey, "https://paxeer.app", nil)
	require.ErrorContains(t, err, "plaintext is 0 bytes, want 1 to 1024")
	_, err = types.SealEnvelope(&attestor.PublicKey, "https://paxeer.app", make([]byte, types.MaxCredentialBytes+1))
	require.ErrorContains(t, err, "plaintext is 1025 bytes")
}

func TestParseEnvelopeRefusals(t *testing.T) {
	attestor, err := crypto.GenerateKey()
	require.NoError(t, err)
	sealed, err := types.SealEnvelope(&attestor.PublicKey, "https://paxeer.app", []byte{1, 0, 1, 'a', 0, 1, 'b'})
	require.NoError(t, err)

	_, err = types.ParseEnvelope(sealed[:types.EnvelopeOverhead])
	require.ErrorIs(t, err, types.ErrInvalidEnvelope)
	require.ErrorContains(t, err, "81 bytes, want 82 to 1105")
	_, err = types.ParseEnvelope(make([]byte, types.MaxEnvelopeBytes+1))
	require.ErrorContains(t, err, "1106 bytes, want 82 to 1105")

	zero := append([]byte(nil), sealed...)
	copy(zero[:20], make([]byte, 20))
	_, err = types.ParseEnvelope(zero)
	require.ErrorContains(t, err, "addressed to the zero attestor")

	badKey := append([]byte(nil), sealed...)
	badKey[20] = 0x05
	_, err = types.ParseEnvelope(badKey)
	require.ErrorContains(t, err, "ephemeral key")
}

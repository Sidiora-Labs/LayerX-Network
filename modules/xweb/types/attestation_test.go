package types_test

import (
	"encoding/json"
	"math/big"
	"os"
	"strings"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/common/hexutil"
	bridgetypes "github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	"github.com/sidiora-labs/paxeer-network/modules/xweb/types"
	"github.com/stretchr/testify/require"
)

type vector struct {
	Name          string `json:"name"`
	Origin        uint8  `json:"origin"`
	NetworkID     uint64 `json:"network_id"`
	Requester     string `json:"requester"`
	RequestID     uint64 `json:"request_id"`
	Kind          uint8  `json:"kind"`
	Payload       string `json:"payload"`
	PayloadVector string `json:"payload_vector"`
	PayloadHex    string `json:"payload_hex"`
	PayloadHash   string `json:"payload_hash"`
	ContentDigest string `json:"content_digest"`
	Response      string `json:"response"`
	ResponseHex   string `json:"response_hex"`
	ResponseHash  string `json:"response_hash"`
	FullLength    uint32 `json:"full_length"`
	Preimage      string `json:"preimage"`
	Digest        string `json:"digest"`
	Signer        string `json:"signer"`
	Signature     string `json:"signature"`
}

type vectorFile struct {
	Domain         string   `json:"domain"`
	PreimageLength int      `json:"preimage_length"`
	Vectors        []vector `json:"vectors"`
}

func loadVectors(t *testing.T) vectorFile {
	t.Helper()
	raw, err := os.ReadFile("testdata/preimage-vectors.json")
	require.NoError(t, err)
	var file vectorFile
	decoder := json.NewDecoder(strings.NewReader(string(raw)))
	decoder.DisallowUnknownFields()
	require.NoError(t, decoder.Decode(&file))
	return file
}

// TestPreimageVectors rebuilds every shared vector byte for byte, checks its
// digest and recovers its signer, and checks ATTESTATION.md carries it. An api
// vector takes its payload from the api vector it names.
func TestPreimageVectors(t *testing.T) {
	file := loadVectors(t)
	apiPayloads := map[string]string{}
	for _, v := range loadApiVectors(t).Vectors {
		apiPayloads[v.Name] = v.Payload
	}
	doc, err := os.ReadFile("../ATTESTATION.md")
	require.NoError(t, err)
	require.Equal(t, types.Domain, file.Domain)
	require.Equal(t, types.PreimageLength, file.PreimageLength)
	require.Equal(t, 188, types.PreimageLength)
	require.Len(t, file.Vectors, 3)
	origins := map[uint8]bool{}
	kinds := map[uint8]bool{}
	for _, v := range file.Vectors {
		t.Run(v.Name, func(t *testing.T) {
			origins[v.Origin] = true
			kinds[v.Kind] = true
			if v.Kind == types.KindApi {
				require.Empty(t, v.Payload)
				require.Equal(t, apiPayloads[v.PayloadVector], v.PayloadHex)
			} else {
				require.Empty(t, v.PayloadVector)
				require.Equal(t, hexutil.Encode([]byte(v.Payload)), v.PayloadHex)
			}
			payload, err := hexutil.Decode(v.PayloadHex)
			require.NoError(t, err)
			require.Equal(t, hexutil.Encode([]byte(v.Response)), v.ResponseHex)
			payloadHash := types.Keccak(payload)
			responseHash := types.Keccak([]byte(v.Response))
			require.Equal(t, v.PayloadHash, payloadHash.Hex())
			require.Equal(t, v.ResponseHash, responseHash.Hex())

			attestation := types.Attestation{
				Origin:        v.Origin,
				NetworkID:     new(big.Int).SetUint64(v.NetworkID),
				Requester:     types.Hash32(common.HexToHash(v.Requester)),
				RequestID:     v.RequestID,
				Kind:          v.Kind,
				PayloadHash:   payloadHash,
				ContentDigest: types.Hash32(common.HexToHash(v.ContentDigest)),
				ResponseHash:  responseHash,
				FullLength:    v.FullLength,
			}
			preimage := types.Preimage(attestation)
			require.Len(t, preimage, types.PreimageLength)
			require.Equal(t, v.Preimage, hexutil.Encode(preimage))
			digest := types.Digest(attestation)
			require.Equal(t, v.Digest, digest.Hex())
			require.Contains(t, string(doc), v.Digest)
			require.Contains(t, string(doc), v.Signature)

			signature, err := hexutil.Decode(v.Signature)
			require.NoError(t, err)
			signer, err := bridgetypes.RecoverSigner(bridgetypes.Hash32(digest), signature)
			require.NoError(t, err)
			require.Equal(t, v.Signer, signer.Hex())
		})
	}
	require.True(t, origins[types.OriginEVM])
	require.True(t, origins[types.OriginProgram])
	require.True(t, kinds[types.KindFetch])
	require.True(t, kinds[types.KindSearch])
	require.True(t, kinds[types.KindApi])
}

func TestPreimageFieldOffsets(t *testing.T) {
	requester := types.Address20(common.HexToAddress("0x00000000000000000000000000000000000a11ce"))
	attestation := types.Attestation{
		Origin:        types.OriginEVM,
		NetworkID:     big.NewInt(713714),
		Requester:     types.EVMRequester(requester),
		RequestID:     0x0102030405060708,
		Kind:          types.KindSearch,
		PayloadHash:   types.Hash32{0xaa},
		ContentDigest: types.Hash32{0xbb},
		ResponseHash:  types.Hash32{0xcc},
		FullLength:    0x0a0b0c0d,
	}
	preimage := types.Preimage(attestation)
	require.Equal(t, "PAXEERX_WEB_V1", string(preimage[:14]))
	require.Equal(t, byte(1), preimage[14])
	require.Equal(t, common.LeftPadBytes(big.NewInt(713714).Bytes(), 32), preimage[15:47])
	require.Equal(t, make([]byte, 12), preimage[47:59])
	require.Equal(t, requester[:], preimage[59:79])
	require.Equal(t, []byte{1, 2, 3, 4, 5, 6, 7, 8}, preimage[79:87])
	require.Equal(t, byte(2), preimage[87])
	require.Equal(t, byte(0xaa), preimage[88])
	require.Equal(t, byte(0xbb), preimage[120])
	require.Equal(t, byte(0xcc), preimage[152])
	require.Equal(t, []byte{0x0a, 0x0b, 0x0c, 0x0d}, preimage[184:188])

	changed := attestation
	changed.Origin = types.OriginProgram
	require.NotEqual(t, types.Digest(attestation), types.Digest(changed))
	changed = attestation
	changed.FullLength++
	require.NotEqual(t, types.Digest(attestation), types.Digest(changed))
}

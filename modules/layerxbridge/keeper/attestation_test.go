package keeper_test

import (
	"math/big"
	"os"
	"strings"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	"github.com/stretchr/testify/require"
)

// TestDigestVectorsFromAttestationDoc recomputes the ATTESTATION.md vectors,
// which the PaxeerXVault asserts on the Ethereum side.
func TestDigestVectorsFromAttestationDoc(t *testing.T) {
	doc, err := os.ReadFile("../ATTESTATION.md")
	require.NoError(t, err)
	text := string(doc)

	amount, ok := new(big.Int).SetString("1000000000000000000", 10)
	require.True(t, ok)
	hash := types.Hash32(common.HexToHash("0x2222222222222222222222222222222222222222222222222222222222222222"))
	vault := types.Address20(common.HexToAddress("0x1111111111111111111111111111111111111111"))
	asset := types.Address20(common.HexToAddress("0x4444444444444444444444444444444444444444"))

	in := types.BridgeIn{ChainID: 1, Vault: vault, TxHash: hash, LogIndex: 7,
		Recipient: types.Hash32(common.HexToHash("0x5555555555555555555555555555555555555555555555555555555555555555")),
		Asset:     asset, Amount: amount}
	require.Len(t, types.InboundPreimage(in), 196)
	inbound := types.InboundDigest(in)
	require.Equal(t, "0x511964ae9566f9536604258667400b0d76335e2a6e60ab0f700bf2433bc97918", common.Hash(inbound).Hex())
	require.True(t, strings.Contains(text, common.Hash(inbound).Hex()), "inbound vector is in ATTESTATION.md")

	out := types.BridgeOut{ChainID: 1, Vault: vault, PaxeerTxHash: hash, PaxeerNonce: 7,
		Recipient: types.Address20(common.HexToAddress("0x3333333333333333333333333333333333333333")),
		Asset:     asset, Amount: amount}
	require.Len(t, types.OutboundPreimage(out), 185)
	outbound := types.OutboundDigest(out)
	require.Equal(t, "0xbd35888e4b158986238ce7abe73957702e2f6e78fe6157197878ebd13edf5b37", common.Hash(outbound).Hex())
	require.True(t, strings.Contains(text, common.Hash(outbound).Hex()), "outbound vector is in ATTESTATION.md")
}

package cli

import (
	"bytes"
	"crypto/ecdsa"
	"crypto/rand"
	"encoding/json"
	"math/big"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/ethereum/go-ethereum/crypto"
	"github.com/stretchr/testify/require"

	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	layerxbridgetypes "github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	"github.com/sidiora-labs/paxeer-network/sdk/codec"
	cdctypes "github.com/sidiora-labs/paxeer-network/sdk/codec/types"
	"github.com/sidiora-labs/paxeer-network/sdk/std"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	govcli "github.com/sidiora-labs/paxeer-network/sdk/x/gov/client/cli"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
)

func TestGetChainId(t *testing.T) {

	tests := []struct {
		name       string
		chainIdHex string
		chainId    int64
		hasURL     bool
	}{
		{"mainnet chain id", "0x531", 1329, true},
		{"testnet chain id", "0x530", 1328, true},
		{"url error chain id", "", 0, false},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {

			//Setup RPC Server with result and get URL
			rpcServer := getRPCServer(t, test.chainIdHex)
			defer rpcServer.Close()

			if test.hasURL {
				chainId, err := getChainId(rpcServer.URL)
				require.NoError(t, err)
				require.Equal(t, *big.NewInt(test.chainId), *chainId)
			} else {
				_, err := getChainId("")
				require.Error(t, err)
			}
		})
	}
}

func TestGetNonce(t *testing.T) {
	//Test nonce is zero for a new wallet
	//Generate a new privateKey from secp256k1 and get public key
	privateKey, err := ecdsa.GenerateKey(crypto.S256(), rand.Reader)
	require.NoError(t, err)

	tests := []struct {
		name      string
		publicKey ecdsa.PublicKey
		nonceHex  string
		nonce     uint64
	}{
		{"new address", privateKey.PublicKey, "0x0", uint64(0)},
		{"active address", privateKey.PublicKey, "0x5", uint64(5)},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {

			//Setup RPC Server with result and get URL
			rpcServer := getRPCServer(t, test.nonceHex)
			defer rpcServer.Close()

			nonce, err := getNonce(rpcServer.URL, test.publicKey)
			require.NoError(t, err)
			require.Equal(t, nonce, test.nonce)
		})
	}
}

func getRPCServer(t *testing.T, result string) *httptest.Server {
	return httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {

		//Standard method call response from POST request to RPC
		response := map[string]any{
			"jsonrpc": "2.0",
			"id":      "send-cli",
			"result":  result,
		}

		//Adjust to default GET response if not POST
		if r.Method != http.MethodPost {
			response = map[string]any{
				"pax": []map[string]any{
					{
						"id":    "evm:local",
						"alias": "pax",
						"state": "OK",
					},
				},
			}
		}

		w.Header().Set("Content-Type", "application/json")
		err := json.NewEncoder(w).Encode(response)
		require.NoError(t, err)
	}))
}

var sidioraPointerProposal = filepath.Join("..", "..", "..", "..", "contracts", "test", "sidiora_pointer_proposal.json")

func pointerProposalCodec() *codec.ProtoCodec {
	registry := cdctypes.NewInterfaceRegistry()
	std.RegisterInterfaces(registry)
	govtypes.RegisterInterfaces(registry)
	types.RegisterInterfaces(registry)
	return codec.NewProtoCodec(registry)
}

func TestSidioraPointerProposalBindsTheDeployedAddress(t *testing.T) {
	body, err := os.ReadFile(sidioraPointerProposal)
	require.NoError(t, err)

	proposal, err := DecodePointerBindingProposal(pointerProposalCodec(), body)
	require.NoError(t, err)
	msgs, err := proposal.GetMessages()
	require.NoError(t, err)
	require.Len(t, msgs, 1)
	bind, ok := msgs[0].(*types.MsgBindERCNativePointer)
	require.True(t, ok)
	require.Equal(t, types.GovernanceAuthority(), bind.Authority)
	require.Equal(t, layerxbridgetypes.SidioraDenom(), bind.Token)
	require.Equal(t, "0x21f7b20a555199fa73A238B1a91FD0f549068fEe", bind.Pointer)
	require.Equal(t, uint32(1), bind.Version)
}

func TestSubmitPointerBindingProposalCarriesTheProposal(t *testing.T) {
	body, err := os.ReadFile(sidioraPointerProposal)
	require.NoError(t, err)
	proposer := sdk.AccAddress(bytes.Repeat([]byte{0x0b}, 20))

	msg, err := NewSubmitPointerBindingProposalMsg(pointerProposalCodec(), body, "10000000uhpx", proposer)
	require.NoError(t, err)
	content, ok := msg.GetContent().(*types.PointerBindingProposal)
	require.True(t, ok)
	require.Len(t, content.Messages, 1)
	require.Equal(t, proposer.String(), msg.Proposer)
	require.Equal(t, sdk.NewCoins(sdk.NewCoin("uhpx", sdk.NewInt(10000000))), msg.InitialDeposit)
}

func TestSubmitPointerBindingProposalRefusesAMissingDepositOrProposer(t *testing.T) {
	body, err := os.ReadFile(sidioraPointerProposal)
	require.NoError(t, err)
	cdc := pointerProposalCodec()
	proposer := sdk.AccAddress(bytes.Repeat([]byte{0x0b}, 20))

	_, err = NewSubmitPointerBindingProposalMsg(cdc, body, "", proposer)
	require.ErrorContains(t, err, "the proposal deposit is missing")
	_, err = NewSubmitPointerBindingProposalMsg(cdc, body, "0uhpx", proposer)
	require.ErrorContains(t, err, "is zero")
	_, err = NewSubmitPointerBindingProposalMsg(cdc, body, "10000000uhpx", nil)
	require.ErrorContains(t, err, "the proposer is missing")
}

func TestDecodePointerBindingProposalRefusesAnotherContent(t *testing.T) {
	text := []byte(`{"@type":"/cosmos.gov.v1beta1.TextProposal","title":"t","description":"d","is_expedited":false}`)
	_, err := DecodePointerBindingProposal(pointerProposalCodec(), text)
	require.ErrorContains(t, err, "not a *types.PointerBindingProposal")
}

func TestDecodePointerBindingProposalRefusesAnotherAuthority(t *testing.T) {
	body, err := os.ReadFile(sidioraPointerProposal)
	require.NoError(t, err)
	other := sdk.AccAddress(bytes.Repeat([]byte{0x0c}, 20)).String()
	forged := strings.Replace(string(body), types.GovernanceAuthority(), other, 1)
	require.NotEqual(t, string(body), forged)
	_, err = DecodePointerBindingProposal(pointerProposalCodec(), []byte(forged))
	require.ErrorContains(t, err, "is not the governance module account")
}

func TestBindERCNativePointerProposalCommandIsMounted(t *testing.T) {
	var found bool
	for _, cmd := range GetTxCmd().Commands() {
		if cmd.Name() == "bind-erc-native-pointer" {
			found = true
			require.NotNil(t, cmd.Flags().Lookup(govcli.FlagDeposit))
		}
	}
	require.True(t, found)
}

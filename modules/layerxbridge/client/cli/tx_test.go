package cli

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/spf13/cobra"
	"github.com/stretchr/testify/require"

	"github.com/sidiora-labs/paxeer-network/bridge/deploy/proposals"
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	"github.com/sidiora-labs/paxeer-network/sdk/client"
	"github.com/sidiora-labs/paxeer-network/sdk/codec"
	cdctypes "github.com/sidiora-labs/paxeer-network/sdk/codec/types"
	"github.com/sidiora-labs/paxeer-network/sdk/std"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	authtx "github.com/sidiora-labs/paxeer-network/sdk/x/auth/tx"
	govcli "github.com/sidiora-labs/paxeer-network/sdk/x/gov/client/cli"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
)

var proposalTestdata = filepath.Join("..", "..", "..", "..", "bridge", "deploy", "proposals", "testdata")

const testDeposit = "10000000uhpx"

var testProposer = sdk.AccAddress(bytes.Repeat([]byte{0x0b}, 20))

func testCodec() (*codec.ProtoCodec, client.TxConfig) {
	registry := cdctypes.NewInterfaceRegistry()
	std.RegisterInterfaces(registry)
	govtypes.RegisterInterfaces(registry)
	types.RegisterInterfaces(registry)
	cdc := codec.NewProtoCodec(registry)
	return cdc, authtx.NewTxConfig(cdc, authtx.DefaultSignModes)
}

// generatedProposals runs the generator over one committed chain
// configuration and the committed attestor manifest and returns the bundle's
// proposals beside the files it writes for them.
func generatedProposals(t *testing.T, chain string) ([]proposals.Proposal, []proposals.File) {
	t.Helper()
	cfg, err := proposals.LoadChainConfig(filepath.Join(proposalTestdata, chain+".json"))
	require.NoError(t, err)
	manifest, err := proposals.LoadAttestorManifest(filepath.Join(proposalTestdata, "attestors.json"))
	require.NoError(t, err)
	bundle, err := proposals.Generate(cfg, manifest)
	require.NoError(t, err)
	built, err := bundle.Proposals()
	require.NoError(t, err)
	files, err := bundle.ProposalFiles()
	require.NoError(t, err)
	require.Len(t, files, len(built))
	return built, files
}

func writeProposal(t *testing.T, name string, body []byte) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), name)
	require.NoError(t, os.WriteFile(path, body, 0o600))
	return path
}

// runSubmit runs paxd tx gov submit-proposal with the bridge subcommand mounted
// the way the node mounts every proposal handler, generating the unsigned
// transaction offline, and returns what the command printed.
func runSubmit(t *testing.T, args ...string) (string, error) {
	t.Helper()
	cdc, txConfig := testCodec()
	var out bytes.Buffer
	clientCtx := client.Context{}.
		WithCodec(cdc).
		WithInterfaceRegistry(cdc.InterfaceRegistry()).
		WithTxConfig(txConfig).
		WithLegacyAmino(codec.NewLegacyAmino()).
		WithChainID("paxeer-x-test").
		WithOutput(&out)
	root := govcli.NewTxCmd([]*cobra.Command{BridgeProposalHandler.CLIHandler()})
	root.SetArgs(append([]string{"submit-proposal", ProposalCommandName}, append(args,
		"--from", testProposer.String(),
		"--generate-only",
		"--keyring-backend", "test",
		"--keyring-dir", t.TempDir(),
	)...))
	root.SetOut(&out)
	root.SetErr(&out)
	err := root.ExecuteContext(context.WithValue(context.Background(), client.ClientContextKey, &clientCtx))
	return out.String(), err
}

func TestSubmitBridgeProposalCommandIsMountedUnderSubmitProposal(t *testing.T) {
	cmd := BridgeProposalHandler.CLIHandler()
	require.Equal(t, "layerxbridge-proposal [proposal-file]", cmd.Use)
	require.Equal(t, ProposalCommandName, cmd.Name())
	require.NotNil(t, cmd.Flags().Lookup(govcli.FlagDeposit))

	root := govcli.NewTxCmd([]*cobra.Command{BridgeProposalHandler.CLIHandler()})
	found, rest, err := root.Find([]string{"submit-proposal", ProposalCommandName, "04-proposal-open-chain.json"})
	require.NoError(t, err)
	require.Equal(t, ProposalCommandName, found.Name())
	require.Equal(t, []string{"04-proposal-open-chain.json"}, rest)
	for _, flag := range []string{"from", "fees", "gas", "generate-only", govcli.FlagDeposit} {
		require.NotNil(t, found.Flags().Lookup(flag), "the mounted subcommand has no --%s", flag)
	}
}

func TestSubmitBridgeProposalCommandCarriesTheGeneratedProposals(t *testing.T) {
	cdc, txConfig := testCodec()
	for _, chain := range []string{"ethereum", "solana"} {
		built, files := generatedProposals(t, chain)
		if chain == "solana" {
			require.Len(t, files, 2)
			require.Equal(t, proposals.SidioraCapProposalFile, files[1].Name)
		}
		require.Equal(t, proposals.OpenChainProposalFile, files[0].Name)
		for i, file := range files {
			t.Run(chain+"/"+file.Name, func(t *testing.T) {
				printed, err := runSubmit(t, writeProposal(t, file.Name, file.Body), "--"+govcli.FlagDeposit, testDeposit)
				require.NoError(t, err, printed)

				decoded, err := txConfig.TxJSONDecoder()([]byte(strings.TrimSpace(printed)))
				require.NoError(t, err)
				msgs := decoded.GetMsgs()
				require.Len(t, msgs, 1)
				submit, ok := msgs[0].(*govtypes.MsgSubmitProposal)
				require.True(t, ok, "the transaction carries a %T", msgs[0])
				require.Equal(t, testProposer.String(), submit.Proposer)
				require.Equal(t, sdk.NewCoins(sdk.NewInt64Coin("uhpx", 10_000_000)), submit.InitialDeposit)

				content, ok := submit.GetContent().(*types.BridgeProposal)
				require.True(t, ok, "the proposal content is a %T", submit.GetContent())
				want := built[i].Content
				require.Equal(t, want.Title, content.Title)
				require.Equal(t, want.Description, content.Description)
				require.NoError(t, content.ValidateBasic())
				carried, err := content.GetMessages()
				require.NoError(t, err)
				expected, err := want.GetMessages()
				require.NoError(t, err)
				require.Len(t, carried, len(expected))
				for j := range expected {
					require.Equal(t, sdk.MsgTypeURL(expected[j]), sdk.MsgTypeURL(carried[j]))
					require.Equal(t, expected[j].String(), carried[j].String())
				}
				// The content the command submits is, byte for byte once
				// re-encoded, the proposal the generator wrote.
				encoded, err := cdc.MarshalInterfaceJSON(content)
				require.NoError(t, err)
				require.JSONEq(t, string(file.Body), string(encoded))
			})
		}
	}
}

func TestSubmitBridgeProposalRefusesAMissingDeposit(t *testing.T) {
	_, files := generatedProposals(t, "ethereum")
	path := writeProposal(t, files[0].Name, files[0].Body)

	_, err := runSubmit(t, path)
	require.ErrorContains(t, err, "the proposal deposit is missing")

	_, err = runSubmit(t, path, "--"+govcli.FlagDeposit, "0uhpx")
	require.ErrorContains(t, err, "is zero")

	_, err = runSubmit(t, path, "--"+govcli.FlagDeposit, "ten")
	require.ErrorContains(t, err, "the proposal deposit")
}

// alter decodes a generated proposal, changes it and encodes it again.
func alter(t *testing.T, body []byte, change func(map[string]any)) []byte {
	t.Helper()
	var document map[string]any
	require.NoError(t, json.Unmarshal(body, &document))
	change(document)
	altered, err := json.MarshalIndent(document, "", "  ")
	require.NoError(t, err)
	return altered
}

func firstMessage(document map[string]any) map[string]any {
	return document["messages"].([]any)[0].(map[string]any)
}

func TestSubmitBridgeProposalRefusesAFileThatIsNotTheGeneratedProposal(t *testing.T) {
	cdc, _ := testCodec()
	_, files := generatedProposals(t, "ethereum")
	body := files[0].Body

	msg, err := NewSubmitBridgeProposalMsg(cdc, body, testDeposit, testProposer)
	require.NoError(t, err)
	require.NoError(t, msg.ValidateBasic())

	text, err := cdc.MarshalInterfaceJSON(&govtypes.TextProposal{Title: "Open a chain", Description: "Not a bridge proposal"})
	require.NoError(t, err)

	cases := []struct {
		name string
		body []byte
		want string
	}{
		{"malformed", body[:len(body)/2], "decode the proposal"},
		{"not JSON", []byte("04-proposal-open-chain.json"), "decode the proposal"},
		{"another content", text, "not a *types.BridgeProposal"},
		{"unknown top-level field", alter(t, body, func(d map[string]any) { d["deposit"] = testDeposit }), "deposit"},
		{"unknown message field", alter(t, body, func(d map[string]any) { firstMessage(d)["signer"] = testProposer.String() }), "signer"},
		{"missing description", alter(t, body, func(d map[string]any) { delete(d, "description") }), "the field description is missing"},
		{"missing messages", alter(t, body, func(d map[string]any) { delete(d, "messages") }), "the field messages is missing"},
		{"missing message authority", alter(t, body, func(d map[string]any) { delete(firstMessage(d), "authority") }), "the field messages[0].authority is missing"},
		{"missing chain", alter(t, body, func(d map[string]any) { delete(firstMessage(d), "chain") }), "the field messages[0].chain is missing"},
		{"empty title", alter(t, body, func(d map[string]any) { d["title"] = "" }), "title"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			_, err := NewSubmitBridgeProposalMsg(cdc, tc.body, testDeposit, testProposer)
			require.ErrorContains(t, err, tc.want)
		})
	}

	_, err = NewSubmitBridgeProposalMsg(cdc, body, testDeposit, nil)
	require.ErrorContains(t, err, "the proposer is missing")

	_, err = runSubmit(t, writeProposal(t, files[0].Name, alter(t, body, func(d map[string]any) { d["extra"] = true })),
		"--"+govcli.FlagDeposit, testDeposit)
	require.ErrorContains(t, err, "extra")
}

func TestBridgeProposalRESTHandlerRefusesWhatTheCommandRefuses(t *testing.T) {
	cdc, txConfig := testCodec()
	clientCtx := client.Context{}.WithCodec(cdc).WithTxConfig(txConfig).WithLegacyAmino(codec.NewLegacyAmino())
	route := BridgeProposalHandler.RESTHandler(clientCtx)
	require.Equal(t, ProposalRESTSubRoute, route.SubRoute)
	_, files := generatedProposals(t, "ethereum")

	post := func(body string) *httptest.ResponseRecorder {
		recorder := httptest.NewRecorder()
		route.Handler(recorder, httptest.NewRequest(http.MethodPost, "/gov/proposals/"+ProposalRESTSubRoute, strings.NewReader(body)))
		return recorder
	}
	baseReq := `{"from":"` + testProposer.String() + `","chain_id":"paxeer-x-test"}`

	missingDeposit := post(`{"base_req":` + baseReq + `,"proposal":` + string(files[0].Body) + `}`)
	require.Equal(t, http.StatusBadRequest, missingDeposit.Code)
	require.Contains(t, missingDeposit.Body.String(), "the proposal deposit is missing")

	unknown := post(`{"base_req":` + baseReq + `,"deposit":"` + testDeposit + `","proposal":` + string(files[0].Body) + `,"expedited":true}`)
	require.Equal(t, http.StatusBadRequest, unknown.Code)
	require.Contains(t, unknown.Body.String(), "expedited")

	missingField := alter(t, files[0].Body, func(d map[string]any) { delete(d, "title") })
	partial := post(`{"base_req":` + baseReq + `,"deposit":"` + testDeposit + `","proposal":` + string(missingField) + `}`)
	require.Equal(t, http.StatusBadRequest, partial.Code)
	require.Contains(t, partial.Body.String(), "the field title is missing")
}

func TestSubmitBridgeProposalCommandCarriesTheSidioraPairAheadOfItsCap(t *testing.T) {
	cdc, txConfig := testCodec()
	_, files := generatedProposals(t, "solana")
	require.Len(t, files, 2)
	sidiora := files[1]
	require.Equal(t, proposals.SidioraCapProposalFile, sidiora.Name)

	printed, err := runSubmit(t, writeProposal(t, sidiora.Name, sidiora.Body), "--"+govcli.FlagDeposit, testDeposit)
	require.NoError(t, err, printed)
	decoded, err := txConfig.TxJSONDecoder()([]byte(strings.TrimSpace(printed)))
	require.NoError(t, err)
	submit, ok := decoded.GetMsgs()[0].(*govtypes.MsgSubmitProposal)
	require.True(t, ok)
	content, ok := submit.GetContent().(*types.BridgeProposal)
	require.True(t, ok, "the proposal content is a %T", submit.GetContent())
	carried, err := content.GetMessages()
	require.NoError(t, err)
	require.Len(t, carried, 2)
	pair, ok := carried[0].(*types.MsgRegisterSidioraPair)
	require.True(t, ok, "the Sidiora proposal carries a %T first, want the pair's registration", carried[0])
	require.Equal(t, types.SidioraHomeChainID, pair.ChainID)
	require.Equal(t, types.DefaultAuthority(), pair.Authority)
	capMsg, ok := carried[1].(*types.MsgSetCap)
	require.True(t, ok, "the Sidiora proposal carries a %T second, want Sidiora's cap", carried[1])
	require.Equal(t, proposals.SidioraAssetID(), capMsg.Asset)
	require.Equal(t, pair.ChainID, capMsg.ChainID)

	pairOnAnotherChain := alter(t, sidiora.Body, func(d map[string]any) { firstMessage(d)["chain_id"] = "1" })
	_, err = NewSubmitBridgeProposalMsg(cdc, pairOnAnotherChain, testDeposit, testProposer)
	require.ErrorContains(t, err, "not Sidiora's foreign home")
	pairWithoutChain := alter(t, sidiora.Body, func(d map[string]any) { delete(firstMessage(d), "chain_id") })
	_, err = NewSubmitBridgeProposalMsg(cdc, pairWithoutChain, testDeposit, testProposer)
	require.ErrorContains(t, err, "the field messages[0].chain_id is missing")
}

package app

import (
	"bytes"
	"path/filepath"
	"testing"
	"time"

	"github.com/sidiora-labs/paxeer-network/bridge/deploy/proposals"
	layerxbridgecli "github.com/sidiora-labs/paxeer-network/modules/layerxbridge/client/cli"
	layerxbridgetypes "github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	"github.com/sidiora-labs/paxeer-network/sdk/client"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
	"github.com/stretchr/testify/require"
)

var bridgeProposalTestdata = filepath.Join("..", "bridge", "deploy", "proposals", "testdata")

// generatedBridgeProposal is the proposal the generator writes for the
// committed ethereum proposal input, as the file an operator submits.
func generatedBridgeProposal(t *testing.T) (proposals.Bundle, []byte) {
	t.Helper()
	cfg, err := proposals.LoadChainConfig(filepath.Join(bridgeProposalTestdata, "ethereum.json"))
	require.NoError(t, err)
	manifest, err := proposals.LoadAttestorManifest(filepath.Join(bridgeProposalTestdata, "attestors.json"))
	require.NoError(t, err)
	bundle, err := proposals.Generate(cfg, manifest)
	require.NoError(t, err)
	files, err := bundle.ProposalFiles()
	require.NoError(t, err)
	require.Len(t, files, 1)
	require.Equal(t, proposals.OpenChainProposalFile, files[0].Name)
	return bundle, files[0].Body
}

// submitBridgeProposal decodes a proposal file through the application's codec
// into the content of a submit-proposal transaction, carries that transaction
// through the codec and submits the proposal to the application's governance
// keeper, returning the proposal as governance stores it.
func submitBridgeProposal(t *testing.T, testApp *App, ctx sdk.Context, body []byte) govtypes.Proposal {
	t.Helper()
	var content govtypes.Content
	require.NoError(t, testApp.AppCodec().UnmarshalInterfaceJSON(body, &content))
	submit, err := govtypes.NewMsgSubmitProposal(content, sdk.NewCoins(), sdk.AccAddress(bytes.Repeat([]byte{0x0b}, 20)))
	require.NoError(t, err)
	encoded, err := testApp.AppCodec().MarshalInterface(submit)
	require.NoError(t, err)
	var decoded sdk.Msg
	require.NoError(t, testApp.AppCodec().UnmarshalInterface(encoded, &decoded))
	carried, ok := decoded.(*govtypes.MsgSubmitProposal)
	require.True(t, ok, "decoded a %T", decoded)
	submitted, err := testApp.GovKeeper.SubmitProposal(ctx, carried.GetContent())
	require.NoError(t, err)
	stored, found := testApp.GovKeeper.GetProposal(ctx, submitted.ProposalId)
	require.True(t, found)
	return stored
}

func TestBridgeGovernanceRouteExecutesAGeneratedProposal(t *testing.T) {
	testApp := Setup(t, false, false, false)
	ctx := testApp.GetContextForDeliverTx([]byte{}).WithBlockHeight(8).WithBlockTime(time.Unix(1_800_000_000, 0))
	router := testApp.GovKeeper.Router()
	require.True(t, router.HasRoute(layerxbridgetypes.RouterKey), "the governance router has no %s route", layerxbridgetypes.RouterKey)

	bundle, body := generatedBridgeProposal(t)
	proposal := submitBridgeProposal(t, testApp, ctx, body)
	require.Equal(t, layerxbridgetypes.RouterKey, proposal.ProposalRoute())
	require.Equal(t, layerxbridgetypes.ProposalTypeBridge, proposal.ProposalType())
	content, ok := proposal.GetContent().(*layerxbridgetypes.BridgeProposal)
	require.True(t, ok, "governance stored a %T", proposal.GetContent())
	carried, err := content.GetMessages()
	require.NoError(t, err)
	require.Len(t, carried, 2+len(bundle.Caps))

	bridge := testApp.LayerXBridgeKeeper
	_, found := bridge.GetChain(ctx, bundle.Register.Chain.ChainID)
	require.False(t, found, "submitting the proposal changed the bridge before it passed")

	// Governance executes a passed proposal by handing its stored content to
	// the route the content names.
	require.NoError(t, router.GetRoute(proposal.ProposalRoute())(ctx, proposal.GetContent()))

	chain, found := bridge.GetChain(ctx, bundle.Register.Chain.ChainID)
	require.True(t, found)
	require.Equal(t, bundle.Register.Chain, chain)
	set := bridge.GetAttestorSet(ctx)
	require.Equal(t, bundle.Attestors.Set.Threshold, set.Threshold)
	require.Len(t, set.Attestors, len(bundle.Attestors.Set.Attestors))
	for i, attestor := range bundle.Attestors.Set.Attestors {
		require.Equal(t, attestor.Signer, set.Attestors[i].Signer)
	}
	for _, capMsg := range bundle.Caps {
		record, found := bridge.GetAsset(ctx, capMsg.ChainID, capMsg.Asset)
		require.True(t, found, "asset %s is not registered", capMsg.Asset.Hex())
		require.Equal(t, layerxbridgetypes.Denom(capMsg.ChainID, capMsg.Asset), record.Denom)
		limit, found := bridge.GetCap(ctx, record.Denom)
		require.True(t, found)
		require.True(t, capMsg.MaxInFlight.Equal(limit.MaxInFlight))
		require.True(t, capMsg.MaxPerTx.Equal(limit.MaxPerTx))
	}
	require.False(t, bridge.IsPaused(ctx))
}

func TestBridgeGovernanceRouteRefusesAProposalForAnotherAuthority(t *testing.T) {
	testApp := Setup(t, false, false, false)
	ctx := testApp.GetContextForDeliverTx([]byte{}).WithBlockHeight(8).WithBlockTime(time.Unix(1_800_000_000, 0))
	bundle, body := generatedBridgeProposal(t)
	governance := layerxbridgetypes.DefaultAuthority()
	require.Equal(t, governance, bundle.Register.Authority)

	// The first message's authority becomes the bridge module account; every
	// other message keeps the governance module account.
	other := layerxbridgetypes.ModuleAddress().String()
	altered := bytes.Replace(body, []byte(`"authority": "`+governance+`"`), []byte(`"authority": "`+other+`"`), 1)
	require.NotEqual(t, body, altered)

	var content govtypes.Content
	require.NoError(t, testApp.AppCodec().UnmarshalInterfaceJSON(altered, &content))
	require.ErrorIs(t, content.ValidateBasic(), layerxbridgetypes.ErrUnauthorized)
	handler := testApp.GovKeeper.Router().GetRoute(layerxbridgetypes.RouterKey)
	require.ErrorIs(t, handler(ctx, content), layerxbridgetypes.ErrUnauthorized)
	_, found := testApp.LayerXBridgeKeeper.GetChain(ctx, bundle.Register.Chain.ChainID)
	require.False(t, found, "a refused proposal registered the chain")
}

func TestBridgeProposalHandlerIsMountedOnSubmitProposal(t *testing.T) {
	var mounted []string
	for _, handler := range getGovProposalHandlers() {
		cmd := handler.CLIHandler()
		mounted = append(mounted, cmd.Name())
		if cmd.Name() != layerxbridgecli.ProposalCommandName {
			continue
		}
		require.Equal(t, "layerxbridge-proposal [proposal-file]", cmd.Use)
		require.Equal(t, layerxbridgecli.ProposalRESTSubRoute, handler.RESTHandler(client.Context{}).SubRoute)
		return
	}
	t.Fatalf("no governance proposal handler mounts %s; mounted: %v", layerxbridgecli.ProposalCommandName, mounted)
}

func TestBridgeSubmitProposalMessageExecutesThroughTheStoredRoute(t *testing.T) {
	testApp := Setup(t, false, false, false)
	ctx := testApp.GetContextForDeliverTx([]byte{}).WithBlockHeight(8).WithBlockTime(time.Unix(1_800_000_000, 0))
	bundle, body := generatedBridgeProposal(t)
	proposer := sdk.AccAddress(bytes.Repeat([]byte{0x0b}, 20))

	// The message the mounted command builds from the generated file, decoded
	// through the application's own codec.
	msg, err := layerxbridgecli.NewSubmitBridgeProposalMsg(testApp.AppCodec(), body, "10000000"+sdk.DefaultBondDenom, proposer)
	require.NoError(t, err)
	require.Equal(t, proposer.String(), msg.Proposer)
	encoded, err := testApp.AppCodec().MarshalInterface(msg)
	require.NoError(t, err)
	var decoded sdk.Msg
	require.NoError(t, testApp.AppCodec().UnmarshalInterface(encoded, &decoded))
	carried, ok := decoded.(*govtypes.MsgSubmitProposal)
	require.True(t, ok, "decoded a %T", decoded)

	submitted, err := testApp.GovKeeper.SubmitProposal(ctx, carried.GetContent())
	require.NoError(t, err)
	proposal, found := testApp.GovKeeper.GetProposal(ctx, submitted.ProposalId)
	require.True(t, found)
	require.Equal(t, layerxbridgetypes.RouterKey, proposal.ProposalRoute())
	bridge := testApp.LayerXBridgeKeeper
	_, found = bridge.GetChain(ctx, bundle.Register.Chain.ChainID)
	require.False(t, found, "submitting the proposal changed the bridge before it passed")

	require.NoError(t, testApp.GovKeeper.Router().GetRoute(proposal.ProposalRoute())(ctx, proposal.GetContent()))

	chain, found := bridge.GetChain(ctx, bundle.Register.Chain.ChainID)
	require.True(t, found)
	require.Equal(t, bundle.Register.Chain, chain)
	require.Equal(t, bundle.Attestors.Set.Threshold, bridge.GetAttestorSet(ctx).Threshold)
	for _, capMsg := range bundle.Caps {
		record, found := bridge.GetAsset(ctx, capMsg.ChainID, capMsg.Asset)
		require.True(t, found, "asset %s is not registered", capMsg.Asset.Hex())
		limit, found := bridge.GetCap(ctx, record.Denom)
		require.True(t, found)
		require.True(t, capMsg.MaxPerTx.Equal(limit.MaxPerTx))
	}
}

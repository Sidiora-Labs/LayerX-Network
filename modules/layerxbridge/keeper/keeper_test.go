package keeper_test

import (
	"fmt"
	"math/big"
	"testing"
	"time"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge"
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/keeper"
	bridgetestutil "github.com/sidiora-labs/paxeer-network/modules/layerxbridge/testutil"
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	app "github.com/sidiora-labs/paxeer-network/node"
	"github.com/sidiora-labs/paxeer-network/sdk/baseapp"
	cdctypes "github.com/sidiora-labs/paxeer-network/sdk/codec/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
	"github.com/sidiora-labs/paxeer-network/sdk/types/module"
	banktypes "github.com/sidiora-labs/paxeer-network/sdk/x/bank/types"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
	"github.com/stretchr/testify/require"
)

const chainID = uint64(1)

var (
	vault     = types.Address20{0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11}
	asset     = types.Address20{0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44}
	recipient = common.HexToAddress("0x00000000000000000000000000000000000a11ce")
	authority = types.DefaultAuthority()
)

type suite struct {
	t         *testing.T
	app       *app.App
	ctx       sdk.Context
	k         keeper.Keeper
	attestors []bridgetestutil.Attestor
	denom     string
}

func paxeerRecipient(address common.Address) types.Hash32 {
	return types.Hash32(common.BytesToHash(address.Bytes()))
}

// newSuite starts from the default (dormant) genesis. With activate it
// registers chain 1, three attestors with threshold two and caps of 1000 per
// transaction and 1500 in flight.
func newSuite(t *testing.T, activate bool) *suite {
	t.Helper()
	testApp := app.Setup(t, false, false, false)
	ctx := testApp.GetContextForDeliverTx([]byte{}).WithBlockHeight(8).WithBlockTime(time.Unix(1_800_000_000, 0))
	k, ctx := bridgetestutil.NewKeeper(testApp, ctx)
	k.InitGenesis(ctx, *types.DefaultGenesis())
	s := &suite{t: t, app: testApp, ctx: ctx, k: k, attestors: bridgetestutil.Attestors(3), denom: types.Denom(chainID, asset)}
	if activate {
		require.NoError(t, k.RegisterChain(ctx, types.MsgRegisterChain{Authority: authority,
			Chain: types.Chain{ChainID: chainID, Vault: vault, FinalityDepth: 64, Enabled: true}}))
		require.NoError(t, k.SetAttestors(ctx, types.MsgSetAttestors{Authority: authority,
			Set: bridgetestutil.Set(s.attestors, 1_000_000, 2)}))
		s.setCap(1500, 1000)
	}
	return s
}

func (s *suite) setCap(maxInFlight, maxPerTx int64) {
	s.t.Helper()
	require.NoError(s.t, s.k.SetCap(s.ctx, types.MsgSetCap{Authority: authority, ChainID: chainID, Asset: asset,
		MaxInFlight: sdk.NewInt(maxInFlight), MaxPerTx: sdk.NewInt(maxPerTx)}))
}

func deposit(logIndex uint64, amount int64) types.BridgeIn {
	return types.BridgeIn{ChainID: chainID, Vault: vault, TxHash: types.Hash32{0xde, 0xad, byte(logIndex)},
		LogIndex: logIndex, Recipient: paxeerRecipient(recipient), Asset: asset, Amount: big.NewInt(amount)}
}

func (s *suite) signed(in types.BridgeIn, attestors ...bridgetestutil.Attestor) [][]byte {
	return bridgetestutil.Sign(types.InboundDigest(in), attestors...)
}

func (s *suite) balance(address common.Address) sdk.Int {
	return s.app.BankKeeper.GetBalance(s.ctx, s.app.EvmKeeper.GetPaxAddressOrDefault(s.ctx, address), s.denom).Amount
}

func (s *suite) events(kind string) []sdk.Event {
	var out []sdk.Event
	for _, event := range s.ctx.EventManager().Events() {
		if event.Type == kind {
			out = append(out, event)
		}
	}
	return out
}

func attribute(event sdk.Event, key string) string {
	for _, attr := range event.Attributes {
		if string(attr.Key) == key {
			return string(attr.Value)
		}
	}
	return ""
}

func TestDefaultGenesisIsDormant(t *testing.T) {
	s := newSuite(t, false)
	genesis := types.DefaultGenesis()
	require.NoError(t, genesis.Validate())
	require.Empty(t, genesis.Chains)
	require.Empty(t, genesis.Attestors.Attestors)
	require.Zero(t, genesis.Attestors.Threshold)
	require.Empty(t, genesis.Caps)
	require.False(t, genesis.Paused)
	require.Equal(t, *genesis, s.k.ExportGenesis(s.ctx))

	in := deposit(1, 10)
	_, err := s.k.BridgeIn(s.ctx, in, s.signed(in, s.attestors...))
	require.ErrorIs(t, err, types.ErrUnknownChain)
	_, err = s.k.BridgeOut(s.ctx, recipient, chainID, asset, big.NewInt(1), types.Address20(recipient))
	require.ErrorIs(t, err, types.ErrUnknownChain)
}

func TestGovernanceMessagesRequireTheAuthority(t *testing.T) {
	s := newSuite(t, false)
	stranger := sdk.AccAddress(make([]byte, 20)).String()
	chain := types.Chain{ChainID: chainID, Vault: vault, FinalityDepth: 64, Enabled: true}
	require.ErrorIs(t, s.k.RegisterChain(s.ctx, types.MsgRegisterChain{Authority: stranger, Chain: chain}), types.ErrUnauthorized)
	require.ErrorIs(t, s.k.SetAttestors(s.ctx, types.MsgSetAttestors{Authority: stranger,
		Set: bridgetestutil.Set(s.attestors, 1, 2)}), types.ErrUnauthorized)
	require.ErrorIs(t, s.k.SetCap(s.ctx, types.MsgSetCap{Authority: stranger, ChainID: chainID, Asset: asset,
		MaxInFlight: sdk.NewInt(1), MaxPerTx: sdk.NewInt(1)}), types.ErrUnauthorized)
	require.ErrorIs(t, s.k.Pause(s.ctx, types.MsgPause{Authority: stranger}), types.ErrUnauthorized)
	require.ErrorIs(t, s.k.Unpause(s.ctx, types.MsgUnpause{Authority: stranger}), types.ErrUnauthorized)

	require.ErrorIs(t, s.k.SetCap(s.ctx, types.MsgSetCap{Authority: authority, ChainID: chainID, Asset: asset,
		MaxInFlight: sdk.NewInt(1), MaxPerTx: sdk.NewInt(1)}), types.ErrUnknownChain, "caps need a registered chain")
	require.ErrorIs(t, s.k.RegisterChain(s.ctx, types.MsgRegisterChain{Authority: authority,
		Chain: types.Chain{ChainID: chainID, FinalityDepth: 64}}), types.ErrInvalidChain)
	require.ErrorIs(t, s.k.SetAttestors(s.ctx, types.MsgSetAttestors{Authority: authority,
		Set: bridgetestutil.Set(s.attestors, 1, 4)}), types.ErrInvalidAttestors)

	require.NoError(t, s.k.RegisterChain(s.ctx, types.MsgRegisterChain{Authority: authority, Chain: chain}))
	got, found := s.k.GetChain(s.ctx, chainID)
	require.True(t, found)
	require.Equal(t, chain, got)
	require.ErrorIs(t, s.k.SetCap(s.ctx, types.MsgSetCap{Authority: authority, ChainID: chainID, Asset: asset,
		MaxInFlight: sdk.NewInt(1), MaxPerTx: sdk.NewInt(2)}), types.ErrInvalidCap)
	s.setCap(0, 0)
	record, found := s.k.GetAsset(s.ctx, chainID, asset)
	require.True(t, found)
	require.Equal(t, s.denom, record.Denom)
	admin, err := s.app.TokenFactoryKeeper.GetAuthorityMetadata(s.ctx, s.denom)
	require.NoError(t, err)
	require.Equal(t, s.k.ModuleAddress().String(), admin.Admin, "the module account administers the bridged denom")

	require.NoError(t, s.k.Pause(s.ctx, types.MsgPause{Authority: authority}))
	require.True(t, s.k.IsPaused(s.ctx))
	require.NoError(t, s.k.Unpause(s.ctx, types.MsgUnpause{Authority: authority}))
	require.False(t, s.k.IsPaused(s.ctx))
}

// router is a real message service router serving the module's Msg service
// over the suite's keeper, registered by the module itself against the
// application's interface registry.
func (s *suite) router() *baseapp.MsgServiceRouter {
	s.t.Helper()
	router := baseapp.NewMsgServiceRouter()
	router.SetInterfaceRegistry(s.app.InterfaceRegistry())
	layerxbridge.NewAppModule(s.k).RegisterServices(
		module.NewConfigurator(s.app.AppCodec(), router, baseapp.NewGRPCQueryRouter()))
	return router
}

// route carries msg the way a transaction does - packed into an Any under its
// type URL, encoded and decoded by the application's codec - and executes the
// decoded message through the router.
func (s *suite) route(router *baseapp.MsgServiceRouter, msg sdk.Msg) error {
	s.t.Helper()
	encoded, err := s.app.AppCodec().MarshalInterface(msg)
	require.NoError(s.t, err)
	var decoded sdk.Msg
	require.NoError(s.t, s.app.AppCodec().UnmarshalInterface(encoded, &decoded))
	require.Equal(s.t, sdk.MsgTypeURL(msg), sdk.MsgTypeURL(decoded))
	reencoded, err := s.app.AppCodec().MarshalInterface(decoded)
	require.NoError(s.t, err)
	require.Equal(s.t, encoded, reencoded, "the decoded message differs from the one sent")
	handler := router.Handler(decoded)
	require.NotNil(s.t, handler, "no route for %s", sdk.MsgTypeURL(decoded))
	_, err = handler(s.ctx, decoded)
	return err
}

func governanceMessages(from string, set types.AttestorSet) []sdk.Msg {
	return []sdk.Msg{
		&types.MsgRegisterChain{Authority: from,
			Chain: types.Chain{ChainID: chainID, Vault: vault, FinalityDepth: 64, Enabled: true}},
		&types.MsgSetAttestors{Authority: from, Set: set},
		&types.MsgSetCap{Authority: from, ChainID: chainID, Asset: asset,
			MaxInFlight: sdk.NewInt(1500), MaxPerTx: sdk.NewInt(1000)},
		&types.MsgPause{Authority: from},
		&types.MsgUnpause{Authority: from},
	}
}

func TestMsgServiceRoutesEveryGovernanceMessageToTheKeeper(t *testing.T) {
	s := newSuite(t, false)
	router := s.router()
	messages := governanceMessages(authority, bridgetestutil.Set(s.attestors, 1_000_000, 2))
	wantTypeURLs := []string{
		"/paxprotocol.paxchain.layerxbridge.MsgRegisterChain",
		"/paxprotocol.paxchain.layerxbridge.MsgSetAttestors",
		"/paxprotocol.paxchain.layerxbridge.MsgSetCap",
		"/paxprotocol.paxchain.layerxbridge.MsgPause",
		"/paxprotocol.paxchain.layerxbridge.MsgUnpause",
	}
	authorityAccount, err := sdk.AccAddressFromBech32(authority)
	require.NoError(t, err)
	for i, msg := range messages {
		require.Equal(t, wantTypeURLs[i], sdk.MsgTypeURL(msg))
		require.NotNil(t, s.app.MsgServiceRouter().Handler(msg), "the application routes no %s", wantTypeURLs[i])
		require.Equal(t, []sdk.AccAddress{authorityAccount}, msg.GetSigners())
		legacy, ok := msg.(interface {
			Route() string
			GetSignBytes() []byte
		})
		require.True(t, ok)
		require.Equal(t, types.RouterKey, legacy.Route())
		require.Contains(t, string(legacy.GetSignBytes()), "layerxbridge/"+wantTypeURLs[i][len("/paxprotocol.paxchain.layerxbridge."):])
	}

	require.NoError(t, s.route(router, messages[0]))
	chain, found := s.k.GetChain(s.ctx, chainID)
	require.True(t, found)
	require.Equal(t, messages[0].(*types.MsgRegisterChain).Chain, chain)

	require.NoError(t, s.route(router, messages[1]))
	set := s.k.GetAttestorSet(s.ctx)
	want := messages[1].(*types.MsgSetAttestors).Set
	require.Equal(t, want.Threshold, set.Threshold)
	require.Len(t, set.Attestors, len(want.Attestors))
	for i := range want.Attestors {
		require.Equal(t, want.Attestors[i].Signer, set.Attestors[i].Signer)
		require.True(t, want.Attestors[i].Bond.Equal(set.Attestors[i].Bond))
	}

	require.NoError(t, s.route(router, messages[2]))
	record, found := s.k.GetAsset(s.ctx, chainID, asset)
	require.True(t, found)
	require.Equal(t, s.denom, record.Denom)
	limit, found := s.k.GetCap(s.ctx, s.denom)
	require.True(t, found)
	require.True(t, limit.MaxInFlight.Equal(sdk.NewInt(1500)))
	require.True(t, limit.MaxPerTx.Equal(sdk.NewInt(1000)))

	require.NoError(t, s.route(router, messages[3]))
	require.True(t, s.k.IsPaused(s.ctx))
	require.NoError(t, s.route(router, messages[4]))
	require.False(t, s.k.IsPaused(s.ctx))

	in := deposit(1, 10)
	minted, err := s.k.BridgeIn(s.ctx, in, s.signed(in, s.attestors[0], s.attestors[1]))
	require.NoError(t, err)
	require.Equal(t, s.denom, minted.Denom)
	require.True(t, sdk.NewInt(10).Equal(s.balance(recipient)))
}

func TestMsgServiceRefusesAWrongAuthority(t *testing.T) {
	s := newSuite(t, false)
	router := s.router()
	stranger := sdk.AccAddress(make([]byte, 20)).String()
	for _, msg := range governanceMessages(stranger, bridgetestutil.Set(s.attestors, 1, 2)) {
		require.ErrorIs(t, s.route(router, msg), types.ErrUnauthorized, sdk.MsgTypeURL(msg))
	}
	for _, msg := range governanceMessages("not-a-bech32-account", bridgetestutil.Set(s.attestors, 1, 2)) {
		require.Empty(t, msg.GetSigners(), sdk.MsgTypeURL(msg))
		require.ErrorIs(t, s.route(router, msg), types.ErrUnauthorized, sdk.MsgTypeURL(msg))
	}
	require.Equal(t, *types.DefaultGenesis(), s.k.ExportGenesis(s.ctx), "a refused message changed the state")

	require.NoError(t, s.route(router, &types.MsgPause{Authority: authority}))
	require.ErrorIs(t, s.route(router, &types.MsgUnpause{Authority: stranger}), types.ErrUnauthorized)
	require.True(t, s.k.IsPaused(s.ctx))
}

func TestBridgeInMintsAtThreshold(t *testing.T) {
	s := newSuite(t, true)
	in := deposit(7, 600)
	result, err := s.k.BridgeIn(s.ctx, in, s.signed(in, s.attestors[0], s.attestors[2]))
	require.NoError(t, err)
	require.Equal(t, s.denom, result.Denom)
	require.Equal(t, sdk.NewInt(600), result.Amount)
	require.Equal(t, types.InboundDigest(in), result.Digest)
	require.Equal(t, []types.Address20{s.attestors[0].Signer, s.attestors[2].Signer}, result.Signers)

	require.Equal(t, sdk.NewInt(600), s.balance(recipient))
	require.Equal(t, sdk.NewInt(600), s.app.BankKeeper.GetSupply(s.ctx, s.denom).Amount)
	require.True(t, s.app.BankKeeper.GetBalance(s.ctx, s.k.ModuleAddress(), s.denom).Amount.IsZero())
	require.Equal(t, sdk.NewInt(600), s.k.InFlight(s.ctx, s.denom))
	require.True(t, s.k.IsNullified(s.ctx, in.Nullifier()))

	events := s.events(types.EventBridgeIn)
	require.Len(t, events, 1)
	require.Equal(t, "1", attribute(events[0], types.AttributeChainID))
	require.Equal(t, "7", attribute(events[0], types.AttributeLogIndex))
	require.Equal(t, "600", attribute(events[0], types.AttributeAmount))
	require.Equal(t, s.denom, attribute(events[0], types.AttributeDenom))

	all := deposit(8, 100)
	_, err = s.k.BridgeIn(s.ctx, all, s.signed(all, s.attestors...))
	require.NoError(t, err, "more than threshold signatures are accepted")
	require.Equal(t, sdk.NewInt(700), s.balance(recipient))
}

func TestBridgeInRefusals(t *testing.T) {
	s := newSuite(t, true)
	in := deposit(1, 600)
	two := func(in types.BridgeIn) [][]byte { return s.signed(in, s.attestors[0], s.attestors[1]) }

	_, err := s.k.BridgeIn(s.ctx, in, s.signed(in, s.attestors[1]))
	require.ErrorIs(t, err, types.ErrBelowThreshold)
	_, err = s.k.BridgeIn(s.ctx, in, nil)
	require.ErrorIs(t, err, types.ErrBelowThreshold)
	_, err = s.k.BridgeIn(s.ctx, in, s.signed(in, s.attestors[1], s.attestors[0]))
	require.ErrorIs(t, err, types.ErrBadSignature, "signers must ascend")
	_, err = s.k.BridgeIn(s.ctx, in, s.signed(in, s.attestors[1], s.attestors[1]))
	require.ErrorIs(t, err, types.ErrBadSignature, "a repeated signer is refused")
	var outsider bridgetestutil.Attestor
	for _, candidate := range bridgetestutil.Attestors(4) {
		if !bridgetestutil.Set(s.attestors, 1, 1).Has(candidate.Signer) {
			outsider = candidate
		}
	}
	require.NotNil(t, outsider.Key)
	_, err = s.k.BridgeIn(s.ctx, in, bridgetestutil.Sign(types.InboundDigest(in), outsider))
	require.ErrorIs(t, err, types.ErrBadSignature)
	other := deposit(1, 601)
	_, err = s.k.BridgeIn(s.ctx, in, two(other))
	require.ErrorIs(t, err, types.ErrBadSignature, "signatures over another amount recover to strangers")

	over := deposit(2, 1001)
	_, err = s.k.BridgeIn(s.ctx, over, two(over))
	require.ErrorIs(t, err, types.ErrCapExceeded, "per-transaction cap")

	wrongVault := in
	wrongVault.Vault = types.Address20{0x12}
	_, err = s.k.BridgeIn(s.ctx, wrongVault, two(wrongVault))
	require.ErrorIs(t, err, types.ErrVaultMismatch)

	badRecipient := in
	badRecipient.Recipient = types.Hash32{0x01}
	_, err = s.k.BridgeIn(s.ctx, badRecipient, two(badRecipient))
	require.ErrorIs(t, err, types.ErrInvalidRequest)

	unregistered := in
	unregistered.ChainID = 5
	_, err = s.k.BridgeIn(s.ctx, unregistered, two(unregistered))
	require.ErrorIs(t, err, types.ErrUnknownChain)

	unknownAsset := in
	unknownAsset.Asset = types.Address20{0x99}
	_, err = s.k.BridgeIn(s.ctx, unknownAsset, two(unknownAsset))
	require.ErrorIs(t, err, types.ErrUnknownAsset)

	require.NoError(t, s.k.Pause(s.ctx, types.MsgPause{Authority: authority}))
	_, err = s.k.BridgeIn(s.ctx, in, two(in))
	require.ErrorIs(t, err, types.ErrPaused)
	require.NoError(t, s.k.Unpause(s.ctx, types.MsgUnpause{Authority: authority}))

	require.NoError(t, s.k.RegisterChain(s.ctx, types.MsgRegisterChain{Authority: authority,
		Chain: types.Chain{ChainID: chainID, Vault: vault, FinalityDepth: 64, Enabled: false}}))
	_, err = s.k.BridgeIn(s.ctx, in, two(in))
	require.ErrorIs(t, err, types.ErrChainDisabled)
	require.NoError(t, s.k.RegisterChain(s.ctx, types.MsgRegisterChain{Authority: authority,
		Chain: types.Chain{ChainID: chainID, Vault: vault, FinalityDepth: 64, Enabled: true}}))

	require.True(t, s.balance(recipient).IsZero(), "no refusal minted")
	require.False(t, s.k.IsNullified(s.ctx, in.Nullifier()), "no refusal consumed the nullifier")

	_, err = s.k.BridgeIn(s.ctx, in, two(in))
	require.NoError(t, err)
	_, err = s.k.BridgeIn(s.ctx, in, two(in))
	require.ErrorIs(t, err, types.ErrNullified)

	next := deposit(3, 1000)
	_, err = s.k.BridgeIn(s.ctx, next, two(next))
	require.ErrorIs(t, err, types.ErrCapExceeded, "in-flight cap: 600 + 1000 > 1500")
	s.setCap(0, 0)
	small := deposit(4, 1)
	_, err = s.k.BridgeIn(s.ctx, small, two(small))
	require.ErrorIs(t, err, types.ErrCapExceeded, "a zero cap refuses everything")
	require.Equal(t, sdk.NewInt(600), s.balance(recipient))
}

func TestBridgeOutBurnsAndEmits(t *testing.T) {
	s := newSuite(t, true)
	in := deposit(1, 900)
	_, err := s.k.BridgeIn(s.ctx, in, s.signed(in, s.attestors[0], s.attestors[1]))
	require.NoError(t, err)
	remote := types.Address20{0x33}

	result, err := s.k.BridgeOut(s.ctx, recipient, chainID, asset, big.NewInt(400), remote)
	require.NoError(t, err)
	require.Equal(t, keeper.BridgeOutResult{Denom: s.denom, Nonce: 1}, result)
	require.Equal(t, sdk.NewInt(500), s.balance(recipient))
	require.Equal(t, sdk.NewInt(500), s.app.BankKeeper.GetSupply(s.ctx, s.denom).Amount, "burned, not parked")
	require.Equal(t, sdk.NewInt(500), s.k.InFlight(s.ctx, s.denom))
	events := s.events(types.EventBridgeOut)
	require.Len(t, events, 1)
	require.Equal(t, "1", attribute(events[0], types.AttributeNonce))
	require.Equal(t, "400", attribute(events[0], types.AttributeAmount))
	require.Equal(t, remote.Hex(), attribute(events[0], types.AttributeRecipient))

	result, err = s.k.BridgeOut(s.ctx, recipient, chainID, asset, big.NewInt(500), remote)
	require.NoError(t, err)
	require.Equal(t, uint64(2), result.Nonce)
	require.True(t, s.balance(recipient).IsZero())

	_, err = s.k.BridgeOut(s.ctx, recipient, chainID, asset, big.NewInt(1), remote)
	require.Error(t, err, "nothing left to burn")
	_, err = s.k.BridgeOut(s.ctx, recipient, chainID, asset, big.NewInt(1), types.Address20{})
	require.ErrorIs(t, err, types.ErrInvalidRequest)
	require.NoError(t, s.k.Pause(s.ctx, types.MsgPause{Authority: authority}))
	_, err = s.k.BridgeOut(s.ctx, recipient, chainID, asset, big.NewInt(1), remote)
	require.ErrorIs(t, err, types.ErrPaused)
	require.Equal(t, uint64(2), s.k.OutboundNonce(s.ctx, chainID))
}

func TestGenesisRoundTrip(t *testing.T) {
	s := newSuite(t, true)
	in := deposit(1, 300)
	_, err := s.k.BridgeIn(s.ctx, in, s.signed(in, s.attestors[0], s.attestors[1]))
	require.NoError(t, err)
	_, err = s.k.BridgeOut(s.ctx, recipient, chainID, asset, big.NewInt(100), types.Address20{0x33})
	require.NoError(t, err)
	require.NoError(t, s.k.Pause(s.ctx, types.MsgPause{Authority: authority}))
	exported := s.k.ExportGenesis(s.ctx)
	require.NoError(t, exported.Validate())
	require.Len(t, exported.Nullifiers, 1)
	require.Equal(t, []types.OutboundNonce{{ChainID: chainID, Nonce: 1}}, exported.OutboundNonces)
	require.Equal(t, []types.InFlight{{Denom: s.denom, Amount: sdk.NewInt(200)}}, exported.InFlight)

	fresh, ctx := bridgetestutil.NewKeeper(s.app, s.ctx)
	fresh.InitGenesis(ctx, exported)
	require.Equal(t, exported, fresh.ExportGenesis(ctx))
}

// submitted packs msgs into a BridgeProposal and carries it the way the
// chain's submit-proposal transaction does - as the content of a
// MsgSubmitProposal encoded and decoded by the application's codec - and
// returns the content governance hands the proposal handler.
func (s *suite) submitted(msgs ...sdk.Msg) (*govtypes.MsgSubmitProposal, govtypes.Content) {
	s.t.Helper()
	content, err := types.NewBridgeProposal("Open chain 1 on the bridge", "Registers chain 1, installs the attestor set and sets its cap.", msgs...)
	require.NoError(s.t, err)
	submit, err := govtypes.NewMsgSubmitProposal(content, sdk.NewCoins(), sdk.AccAddress(recipient.Bytes()))
	require.NoError(s.t, err)
	encoded, err := s.app.AppCodec().MarshalInterface(submit)
	require.NoError(s.t, err)
	var decoded sdk.Msg
	require.NoError(s.t, s.app.AppCodec().UnmarshalInterface(encoded, &decoded))
	carried, ok := decoded.(*govtypes.MsgSubmitProposal)
	require.True(s.t, ok, "decoded a %T", decoded)
	return carried, carried.GetContent()
}

func TestProposalHandlerExecutesEveryCarriedMessage(t *testing.T) {
	s := newSuite(t, false)
	messages := governanceMessages(authority, bridgetestutil.Set(s.attestors, 1_000_000, 2))
	submit, content := s.submitted(messages...)
	require.NoError(t, submit.ValidateBasic())
	require.Equal(t, types.RouterKey, content.ProposalRoute())
	require.Equal(t, types.ProposalTypeBridge, content.ProposalType())
	proposal, ok := content.(*types.BridgeProposal)
	require.True(t, ok, "the content decoded as %T", content)
	carried, err := proposal.GetMessages()
	require.NoError(t, err)
	require.Len(t, carried, len(messages))
	for i := range messages {
		require.Equal(t, sdk.MsgTypeURL(messages[i]), sdk.MsgTypeURL(carried[i]))
		require.Equal(t, messages[i].String(), carried[i].String(), "message %d changed on its way through governance", i)
	}
	signBytes := string(submit.GetSignBytes())
	require.Contains(t, signBytes, "layerxbridge/BridgeProposal")
	require.Contains(t, signBytes, "layerxbridge/MsgRegisterChain")
	require.Contains(t, signBytes, "layerxbridge/MsgSetCap")

	require.NoError(t, layerxbridge.NewProposalHandler(s.k)(s.ctx, content))
	chain, found := s.k.GetChain(s.ctx, chainID)
	require.True(t, found)
	require.Equal(t, messages[0].(*types.MsgRegisterChain).Chain, chain)
	set := s.k.GetAttestorSet(s.ctx)
	require.Equal(t, uint32(2), set.Threshold)
	require.Len(t, set.Attestors, len(s.attestors))
	limit, found := s.k.GetCap(s.ctx, s.denom)
	require.True(t, found)
	require.True(t, limit.MaxInFlight.Equal(sdk.NewInt(1500)))
	require.True(t, limit.MaxPerTx.Equal(sdk.NewInt(1000)))
	require.False(t, s.k.IsPaused(s.ctx), "the proposal pauses and then unpauses, in order")
	require.Len(t, s.events(types.EventChainRegistered), 1)
	require.Len(t, s.events(types.EventPaused), 1)
	require.Len(t, s.events(types.EventUnpaused), 1)

	in := deposit(1, 10)
	minted, err := s.k.BridgeIn(s.ctx, in, s.signed(in, s.attestors[0], s.attestors[1]))
	require.NoError(t, err)
	require.Equal(t, s.denom, minted.Denom)
	require.True(t, sdk.NewInt(10).Equal(s.balance(recipient)))
}

func TestProposalHandlerRefusesAWrongAuthority(t *testing.T) {
	s := newSuite(t, false)
	handler := layerxbridge.NewProposalHandler(s.k)
	set := bridgetestutil.Set(s.attestors, 1, 2)
	stranger := sdk.AccAddress(make([]byte, 20)).String()
	bridgeAccount := s.k.ModuleAddress().String()
	for _, from := range []string{stranger, bridgeAccount} {
		for i := range governanceMessages(authority, set) {
			messages := governanceMessages(authority, set)
			messages[i] = governanceMessages(from, set)[i]
			submit, content := s.submitted(messages...)
			require.ErrorIs(t, submit.ValidateBasic(), types.ErrUnauthorized, "message %d from %s", i, from)
			require.ErrorIs(t, handler(s.ctx, content), types.ErrUnauthorized, "message %d from %s", i, from)
		}
	}
	for _, msg := range governanceMessages("not-a-bech32-account", set) {
		content := &types.BridgeProposal{Title: "t", Description: "d"}
		packed, err := cdctypes.NewAnyWithValue(msg)
		require.NoError(t, err)
		content.Messages = append(content.Messages, packed)
		require.ErrorIs(t, handler(s.ctx, content), types.ErrUnauthorized, sdk.MsgTypeURL(msg))
	}

	// The keeper applies a message only for the module's authority, so even a
	// proposal for the governance module account is refused once the module's
	// authority is another account.
	params := s.k.GetParams(s.ctx)
	params.Authority = bridgeAccount
	require.NoError(t, s.k.SetParams(s.ctx, params))
	_, content := s.submitted(&types.MsgPause{Authority: authority})
	require.ErrorIs(t, handler(s.ctx, content), types.ErrUnauthorized)
	require.False(t, s.k.IsPaused(s.ctx))
	params.Authority = authority
	require.NoError(t, s.k.SetParams(s.ctx, params))
	require.Equal(t, *types.DefaultGenesis(), s.k.ExportGenesis(s.ctx), "a refused proposal changed the state")
}

func TestProposalHandlerRefusesAMalformedProposal(t *testing.T) {
	s := newSuite(t, false)
	handler := layerxbridge.NewProposalHandler(s.k)
	chain := types.Chain{ChainID: chainID, Vault: vault, FinalityDepth: 64, Enabled: true}
	register := &types.MsgRegisterChain{Authority: authority, Chain: chain}

	require.ErrorIs(t, handler(s.ctx, govtypes.NewTextProposal("text", "a text proposal", false)), sdkerrors.ErrUnknownRequest)
	require.ErrorIs(t, handler(s.ctx, &types.BridgeProposal{Title: "t", Description: "d"}), govtypes.ErrInvalidProposalContent,
		"a proposal carrying no message")
	for _, abstract := range [][2]string{{"", "d"}, {"t", ""}} {
		content, err := types.NewBridgeProposal(abstract[0], abstract[1], register)
		require.NoError(t, err)
		require.ErrorIs(t, handler(s.ctx, content), govtypes.ErrInvalidProposalContent, "title %q description %q", abstract[0], abstract[1])
	}

	send := &banktypes.MsgSend{FromAddress: authority, ToAddress: authority, Amount: sdk.NewCoins(sdk.NewInt64Coin("upax", 1))}
	_, err := types.NewBridgeProposal("t", "d", register, send)
	require.ErrorIs(t, err, govtypes.ErrInvalidProposalContent, "a message of another module")
	packedSend, err := cdctypes.NewAnyWithValue(send)
	require.NoError(t, err)
	require.ErrorIs(t, handler(s.ctx, &types.BridgeProposal{Title: "t", Description: "d", Messages: []*cdctypes.Any{packedSend}}),
		govtypes.ErrInvalidProposalContent, "a message of another module")

	encodedPause, err := (&types.MsgPause{Authority: authority}).Marshal()
	require.NoError(t, err)
	unresolved := &cdctypes.Any{TypeUrl: sdk.MsgTypeURL(&types.MsgPause{}), Value: encodedPause}
	require.ErrorIs(t, handler(s.ctx, &types.BridgeProposal{Title: "t", Description: "d", Messages: []*cdctypes.Any{unresolved}}),
		govtypes.ErrInvalidProposalContent, "a message never unpacked through the registry")
	require.ErrorIs(t, handler(s.ctx, &types.BridgeProposal{Title: "t", Description: "d", Messages: []*cdctypes.Any{nil}}),
		govtypes.ErrInvalidProposalContent, "an empty message")

	_, content := s.submitted(&types.MsgRegisterChain{Authority: authority, Chain: types.Chain{ChainID: chainID, FinalityDepth: 64}})
	require.ErrorIs(t, handler(s.ctx, content), types.ErrInvalidChain, "a message its own ValidateBasic refuses")

	// A proposal executes whole or not at all: the cap of an unregistered
	// chain fails after the registration ran, and the registration is undone.
	_, content = s.submitted(register, &types.MsgSetCap{Authority: authority, ChainID: chainID + 1, Asset: asset,
		MaxInFlight: sdk.NewInt(1500), MaxPerTx: sdk.NewInt(1000)})
	require.NoError(t, content.ValidateBasic())
	require.ErrorIs(t, handler(s.ctx, content), types.ErrUnknownChain)
	_, found := s.k.GetChain(s.ctx, chainID)
	require.False(t, found, "a failed proposal left its first message applied")
	require.Empty(t, s.events(types.EventChainRegistered))
	require.Equal(t, *types.DefaultGenesis(), s.k.ExportGenesis(s.ctx), "a refused proposal changed the state")
}

// solanaChain is Solana as governance registers it: Sidiora's foreign home.
var solanaChain = types.Chain{ChainID: types.SidioraHomeChainID, Vault: vault, FinalityDepth: 32, Enabled: true}

func sidioraCap(from string) *types.MsgSetCap {
	return &types.MsgSetCap{Authority: from, ChainID: types.SidioraHomeChainID, Asset: sidioraAsset(),
		MaxInFlight: sdk.NewInt(1500), MaxPerTx: sdk.NewInt(1000)}
}

func TestSidioraHomeChainIDIsSolana(t *testing.T) {
	label := make([]byte, 8)
	copy(label[2:], "SOLANA")
	require.Equal(t, new(big.Int).SetBytes(label).Uint64(), types.SidioraHomeChainID)
	require.Equal(t, uint64(91600046870081), types.SidioraHomeChainID)
}

func TestMsgServiceRegistersTheSidioraPair(t *testing.T) {
	s := newSuite(t, false)
	router := s.router()
	pair := &types.MsgRegisterSidioraPair{Authority: authority, ChainID: types.SidioraHomeChainID}
	const typeURL = "/paxprotocol.paxchain.layerxbridge.MsgRegisterSidioraPair"
	require.Equal(t, typeURL, sdk.MsgTypeURL(pair))
	require.NotNil(t, s.app.MsgServiceRouter().Handler(pair), "the application routes no %s", typeURL)
	authorityAccount, err := sdk.AccAddressFromBech32(authority)
	require.NoError(t, err)
	require.Equal(t, []sdk.AccAddress{authorityAccount}, pair.GetSigners())
	require.Equal(t, types.RouterKey, pair.Route())
	require.Equal(t, types.TypeMsgRegisterSidioraPair, pair.Type())
	require.Contains(t, string(pair.GetSignBytes()), "layerxbridge/MsgRegisterSidioraPair")

	require.ErrorIs(t, s.route(router, pair), types.ErrUnknownChain, "the pair needs its chain registered first")
	require.NoError(t, s.route(router, &types.MsgRegisterChain{Authority: authority, Chain: solanaChain}))

	stranger := sdk.AccAddress(make([]byte, 20)).String()
	require.ErrorIs(t, s.route(router, &types.MsgRegisterSidioraPair{Authority: stranger, ChainID: types.SidioraHomeChainID}),
		types.ErrUnauthorized)
	malformed := &types.MsgRegisterSidioraPair{Authority: "not-a-bech32-account", ChainID: types.SidioraHomeChainID}
	require.Empty(t, malformed.GetSigners())
	require.ErrorIs(t, s.route(router, malformed), types.ErrUnauthorized)
	require.NoError(t, s.route(router, &types.MsgRegisterChain{Authority: authority,
		Chain: types.Chain{ChainID: chainID, Vault: vault, FinalityDepth: 64, Enabled: true}}))
	for _, other := range []uint64{0, chainID, types.SidioraHomeChainID + 1} {
		msg := &types.MsgRegisterSidioraPair{Authority: authority, ChainID: other}
		require.ErrorIs(t, msg.ValidateBasic(), types.ErrInvalidRequest, "chain %d", other)
		require.ErrorIs(t, s.route(router, msg), types.ErrInvalidRequest, "chain %d is not Sidiora's foreign home", other)
	}
	_, found := s.k.GetAssetByDenom(s.ctx, types.SidioraDenom())
	require.False(t, found, "a refused registration recorded the pair")

	require.NoError(t, s.route(router, pair))
	record, found := s.k.GetAsset(s.ctx, types.SidioraHomeChainID, sidioraAsset())
	require.True(t, found)
	require.Equal(t, types.SidioraDenom(), record.Denom)
	metadata, found := s.app.BankKeeper.GetDenomMetaData(s.ctx, types.SidioraDenom())
	require.True(t, found)
	require.Equal(t, types.SidioraSymbol, metadata.Symbol)

	// Registering the pair again succeeds and changes nothing; the Msg service
	// answers with the denom and the keeper emits the pair's event.
	again := s.ctx.WithEventManager(sdk.NewEventManager())
	response, err := keeper.NewMsgServerImpl(s.k).RegisterSidioraPair(sdk.WrapSDKContext(again), pair)
	require.NoError(t, err, "registering the pair again")
	require.Equal(t, types.SidioraDenom(), response.Denom)
	var events []sdk.Event
	for _, event := range again.EventManager().Events() {
		if event.Type == types.EventSidioraPair {
			events = append(events, event)
		}
	}
	require.Len(t, events, 1)
	require.Equal(t, fmt.Sprint(types.SidioraHomeChainID), attribute(events[0], types.AttributeChainID))
	require.Equal(t, sidioraAsset().Hex(), attribute(events[0], types.AttributeAsset))
	require.Equal(t, types.SidioraDenom(), attribute(events[0], types.AttributeDenom))
	again2, found := s.k.GetAsset(s.ctx, types.SidioraHomeChainID, sidioraAsset())
	require.True(t, found)
	require.Equal(t, record, again2)

	require.NoError(t, s.route(router, sidioraCap(authority)))
	limit, found := s.k.GetCap(s.ctx, types.SidioraDenom())
	require.True(t, found, "the cap after the pair is set on the usid denom")
	require.True(t, limit.MaxInFlight.Equal(sdk.NewInt(1500)))
	_, found = s.k.GetCap(s.ctx, types.Denom(types.SidioraHomeChainID, sidioraAsset()))
	require.False(t, found, "the cap after the pair created the generic denom")
}

func TestSidioraPairIsRefusedOnceACapBoundItElsewhere(t *testing.T) {
	s := newSuite(t, false)
	router := s.router()
	require.NoError(t, s.route(router, &types.MsgRegisterChain{Authority: authority, Chain: solanaChain}))
	require.NoError(t, s.route(router, sidioraCap(authority)))
	before := s.k.ExportGenesis(s.ctx)
	require.ErrorIs(t, s.route(router, &types.MsgRegisterSidioraPair{Authority: authority, ChainID: types.SidioraHomeChainID}),
		types.ErrInvalidRequest)
	require.Equal(t, before, s.k.ExportGenesis(s.ctx))
	require.Empty(t, s.events(types.EventSidioraPair))
}

func TestProposalHandlerRegistersTheSidioraPairAheadOfItsCap(t *testing.T) {
	s := newSuite(t, false)
	handler := layerxbridge.NewProposalHandler(s.k)
	pair := &types.MsgRegisterSidioraPair{Authority: authority, ChainID: types.SidioraHomeChainID}

	_, content := s.submitted(pair, sidioraCap(authority))
	require.ErrorIs(t, handler(s.ctx, content), types.ErrUnknownChain, "the Sidiora proposal before the chain is open")

	_, content = s.submitted(&types.MsgRegisterChain{Authority: authority, Chain: solanaChain},
		&types.MsgSetAttestors{Authority: authority, Set: bridgetestutil.Set(s.attestors, 1_000_000, 2)})
	require.NoError(t, handler(s.ctx, content))

	// Capped first, the pair would bind to the generic denom and its
	// registration would then fail: the proposal executes whole or not at all.
	_, content = s.submitted(sidioraCap(authority), pair)
	require.ErrorIs(t, handler(s.ctx, content), types.ErrInvalidRequest)
	_, found := s.k.GetAsset(s.ctx, types.SidioraHomeChainID, sidioraAsset())
	require.False(t, found, "a failed proposal left Sidiora's cap applied")

	for _, from := range []string{sdk.AccAddress(make([]byte, 20)).String(), s.k.ModuleAddress().String()} {
		submit, refused := s.submitted(&types.MsgRegisterSidioraPair{Authority: from, ChainID: types.SidioraHomeChainID}, sidioraCap(authority))
		require.ErrorIs(t, submit.ValidateBasic(), types.ErrUnauthorized, from)
		require.ErrorIs(t, handler(s.ctx, refused), types.ErrUnauthorized, from)
	}
	submit, refused := s.submitted(&types.MsgRegisterSidioraPair{Authority: authority, ChainID: chainID}, sidioraCap(authority))
	require.ErrorIs(t, submit.ValidateBasic(), types.ErrInvalidRequest)
	require.ErrorIs(t, handler(s.ctx, refused), types.ErrInvalidRequest)

	submit, content = s.submitted(pair, sidioraCap(authority))
	require.NoError(t, submit.ValidateBasic())
	require.Contains(t, string(submit.GetSignBytes()), "layerxbridge/MsgRegisterSidioraPair")
	proposal := content.(*types.BridgeProposal)
	carried, err := proposal.GetMessages()
	require.NoError(t, err)
	require.Len(t, carried, 2)
	require.Equal(t, pair.String(), carried[0].String())
	require.NoError(t, handler(s.ctx, content))
	record, found := s.k.GetAsset(s.ctx, types.SidioraHomeChainID, sidioraAsset())
	require.True(t, found)
	require.Equal(t, types.SidioraDenom(), record.Denom)
	limit, found := s.k.GetCap(s.ctx, types.SidioraDenom())
	require.True(t, found)
	require.True(t, limit.MaxPerTx.Equal(sdk.NewInt(1000)))
	require.Len(t, s.events(types.EventSidioraPair), 1)
}

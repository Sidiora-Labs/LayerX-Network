package keeper_test

import (
	"math/big"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	tokenfactorykeeper "github.com/sidiora-labs/paxeer-network/modules/tokenfactory/keeper"
	tokenfactorytypes "github.com/sidiora-labs/paxeer-network/modules/tokenfactory/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	banktypes "github.com/sidiora-labs/paxeer-network/sdk/x/bank/types"
	"github.com/stretchr/testify/require"
)

func sidioraAsset() types.Address20 {
	return types.Address20(common.HexToAddress(types.SidioraRemoteAddress))
}

func newSidioraSuite(t *testing.T) *suite {
	t.Helper()
	s := newSuite(t, true)
	denom, err := s.k.EnsureSidioraDenom(s.ctx, chainID)
	require.NoError(t, err)
	s.denom = denom
	require.NoError(t, s.k.SetCap(s.ctx, types.MsgSetCap{
		Authority: authority, ChainID: chainID, Asset: sidioraAsset(),
		MaxInFlight: sdk.NewInt(1500), MaxPerTx: sdk.NewInt(1000),
	}))
	return s
}

func sidioraDeposit(logIndex uint64, amount int64) types.BridgeIn {
	in := deposit(logIndex, amount)
	in.Asset = sidioraAsset()
	return in
}

func TestSidioraDenomAndMetadata(t *testing.T) {
	s := newSuite(t, true)
	denom, err := tokenfactorytypes.GetTokenDenom(types.ModuleAddress().String(), types.SidioraSubdenom)
	require.NoError(t, err)
	require.Equal(t, denom, types.SidioraDenom())
	creator, subdenom, err := tokenfactorytypes.DeconstructDenom(denom)
	require.NoError(t, err)
	require.Equal(t, s.k.ModuleAddress().String(), creator)
	require.Equal(t, "usid", subdenom)
	require.Equal(t, "SID", types.SidioraSymbol)
	require.Equal(t, uint32(6), types.SidioraDecimals)
	_, found := s.app.BankKeeper.GetDenomMetaData(s.ctx, denom)
	require.False(t, found)

	got, err := s.k.EnsureSidioraDenom(s.ctx, chainID)
	require.NoError(t, err)
	require.Equal(t, denom, got)
	metadata, found := s.app.BankKeeper.GetDenomMetaData(s.ctx, denom)
	require.True(t, found)
	require.NoError(t, metadata.Validate())
	require.Equal(t, banktypes.Metadata{
		Description: "Sidiora, the second official coin of Paxeer X Network.",
		DenomUnits:  []*banktypes.DenomUnit{{Denom: denom, Exponent: 0}, {Denom: "SID", Exponent: 6}},
		Base:        denom, Display: "SID", Name: "Sidiora", Symbol: "SID",
	}, metadata)
	admin, err := s.app.TokenFactoryKeeper.GetAuthorityMetadata(s.ctx, denom)
	require.NoError(t, err)
	require.Equal(t, creator, admin.Admin)
	record, found := s.k.GetAsset(s.ctx, chainID, sidioraAsset())
	require.True(t, found)
	require.Equal(t, types.BridgedAsset{ChainID: chainID, Asset: sidioraAsset(), Denom: denom}, record)
	reverse, found := s.k.GetAssetByDenom(s.ctx, denom)
	require.True(t, found)
	require.Equal(t, record, reverse)
	_, capped := s.k.GetCap(s.ctx, denom)
	require.False(t, capped)
	in := sidioraDeposit(1, 1)
	_, err = s.k.BridgeIn(s.ctx, in, s.signed(in, s.attestors...))
	require.ErrorIs(t, err, types.ErrCapExceeded)

	before := s.k.ExportGenesis(s.ctx)
	got, err = s.k.EnsureSidioraDenom(s.ctx, chainID)
	require.NoError(t, err)
	require.Equal(t, denom, got)
	require.Equal(t, before, s.k.ExportGenesis(s.ctx))
	after, found := s.app.BankKeeper.GetDenomMetaData(s.ctx, denom)
	require.True(t, found)
	require.Equal(t, metadata, after)
	require.True(t, s.app.BankKeeper.GetSupply(s.ctx, denom).Amount.IsZero())
}

func TestSidioraBridgeMintBurnAndAdmin(t *testing.T) {
	s := newSidioraSuite(t)
	in := sidioraDeposit(1, 900)
	result, err := s.k.BridgeIn(s.ctx, in, s.signed(in, s.attestors[0], s.attestors[1]))
	require.NoError(t, err)
	require.Equal(t, s.denom, result.Denom)
	require.Equal(t, sdk.NewInt(900), s.balance(recipient))
	require.Equal(t, sdk.NewInt(900), s.app.BankKeeper.GetSupply(s.ctx, s.denom).Amount)
	require.Equal(t, sdk.NewInt(900), s.k.InFlight(s.ctx, s.denom))
	require.True(t, s.k.IsNullified(s.ctx, in.Nullifier()))

	server := tokenfactorykeeper.NewMsgServerImpl(s.app.TokenFactoryKeeper)
	for _, sender := range []string{authority, s.app.EvmKeeper.GetPaxAddressOrDefault(s.ctx, recipient).String()} {
		_, err = server.Mint(sdk.WrapSDKContext(s.ctx), &tokenfactorytypes.MsgMint{
			Sender: sender, Amount: sdk.NewInt64Coin(s.denom, 1),
		})
		require.ErrorIs(t, err, tokenfactorytypes.ErrUnauthorized)
		_, err = server.Burn(sdk.WrapSDKContext(s.ctx), &tokenfactorytypes.MsgBurn{
			Sender: sender, Amount: sdk.NewInt64Coin(s.denom, 1),
		})
		require.ErrorIs(t, err, tokenfactorytypes.ErrUnauthorized)
	}
	require.Equal(t, sdk.NewInt(900), s.balance(recipient))
	require.Equal(t, sdk.NewInt(900), s.app.BankKeeper.GetSupply(s.ctx, s.denom).Amount)

	out, err := s.k.BridgeOut(s.ctx, recipient, chainID, sidioraAsset(), big.NewInt(400), types.Address20{0x33})
	require.NoError(t, err)
	require.Equal(t, keeper.BridgeOutResult{Denom: s.denom, Nonce: 1}, out)
	require.Equal(t, sdk.NewInt(500), s.balance(recipient))
	require.Equal(t, sdk.NewInt(500), s.app.BankKeeper.GetSupply(s.ctx, s.denom).Amount)
	require.Equal(t, sdk.NewInt(500), s.k.InFlight(s.ctx, s.denom))
	require.True(t, s.app.BankKeeper.GetBalance(s.ctx, s.k.ModuleAddress(), s.denom).Amount.IsZero())

	before := s.k.ExportGenesis(s.ctx)
	_, err = s.k.EnsureSidioraDenom(s.ctx, chainID)
	require.NoError(t, err)
	require.Equal(t, before, s.k.ExportGenesis(s.ctx))
	require.Equal(t, sdk.NewInt(500), s.balance(recipient))
}

func TestSidioraBridgeRefusals(t *testing.T) {
	s := newSidioraSuite(t)
	in := sidioraDeposit(1, 600)
	_, err := s.k.BridgeIn(s.ctx, in, s.signed(in, s.attestors[0]))
	require.ErrorIs(t, err, types.ErrBelowThreshold)
	over := sidioraDeposit(2, 1001)
	_, err = s.k.BridgeIn(s.ctx, over, s.signed(over, s.attestors...))
	require.ErrorIs(t, err, types.ErrCapExceeded)
	unknown := in
	unknown.Asset = types.Address20{0x99}
	_, err = s.k.BridgeIn(s.ctx, unknown, s.signed(unknown, s.attestors...))
	require.ErrorIs(t, err, types.ErrUnknownAsset)
	require.NoError(t, s.k.Pause(s.ctx, types.MsgPause{Authority: authority}))
	_, err = s.k.BridgeIn(s.ctx, in, s.signed(in, s.attestors...))
	require.ErrorIs(t, err, types.ErrPaused)
	_, err = s.k.BridgeOut(s.ctx, recipient, chainID, sidioraAsset(), big.NewInt(1), types.Address20{0x33})
	require.ErrorIs(t, err, types.ErrPaused)
	require.True(t, s.balance(recipient).IsZero())
	require.True(t, s.k.InFlight(s.ctx, s.denom).IsZero())
	require.False(t, s.k.IsNullified(s.ctx, in.Nullifier()))
	require.False(t, s.k.IsNullified(s.ctx, over.Nullifier()))
	require.Zero(t, s.k.OutboundNonce(s.ctx, chainID))
	require.NoError(t, s.k.Unpause(s.ctx, types.MsgUnpause{Authority: authority}))

	_, err = s.k.BridgeIn(s.ctx, in, s.signed(in, s.attestors...))
	require.NoError(t, err)
	_, err = s.k.BridgeIn(s.ctx, in, s.signed(in, s.attestors...))
	require.ErrorIs(t, err, types.ErrNullified)
	next := sidioraDeposit(3, 1000)
	_, err = s.k.BridgeIn(s.ctx, next, s.signed(next, s.attestors...))
	require.ErrorIs(t, err, types.ErrCapExceeded)
	require.False(t, s.k.IsNullified(s.ctx, next.Nullifier()))
	require.Equal(t, sdk.NewInt(600), s.balance(recipient))
	require.Equal(t, sdk.NewInt(600), s.k.InFlight(s.ctx, s.denom))
	require.Equal(t, sdk.NewInt(600), s.app.BankKeeper.GetSupply(s.ctx, s.denom).Amount)
}

func TestSidioraInitializationRefusals(t *testing.T) {
	t.Run("unknown chain", func(t *testing.T) {
		s := newSuite(t, false)
		_, err := s.k.EnsureSidioraDenom(s.ctx, chainID)
		require.ErrorIs(t, err, types.ErrUnknownChain)
		_, found := s.app.BankKeeper.GetDenomMetaData(s.ctx, types.SidioraDenom())
		require.False(t, found)
	})
	t.Run("conflicting remote asset", func(t *testing.T) {
		s := newSuite(t, true)
		require.NoError(t, s.k.SetCap(s.ctx, types.MsgSetCap{
			Authority: authority, ChainID: chainID, Asset: sidioraAsset(),
			MaxInFlight: sdk.NewInt(1500), MaxPerTx: sdk.NewInt(1000),
		}))
		before := s.k.ExportGenesis(s.ctx)
		_, err := s.k.EnsureSidioraDenom(s.ctx, chainID)
		require.ErrorIs(t, err, types.ErrInvalidRequest)
		require.Equal(t, before, s.k.ExportGenesis(s.ctx))
		_, found := s.app.BankKeeper.GetDenomMetaData(s.ctx, types.SidioraDenom())
		require.False(t, found)
	})
	t.Run("conflicting chain", func(t *testing.T) {
		s := newSidioraSuite(t)
		require.NoError(t, s.k.RegisterChain(s.ctx, types.MsgRegisterChain{Authority: authority,
			Chain: types.Chain{ChainID: 2, Vault: vault, FinalityDepth: 64, Enabled: true}}))
		before := s.k.ExportGenesis(s.ctx)
		_, err := s.k.EnsureSidioraDenom(s.ctx, 2)
		require.ErrorIs(t, err, types.ErrInvalidRequest)
		require.Equal(t, before, s.k.ExportGenesis(s.ctx))
	})
	for _, admin := range []string{authority, ""} {
		t.Run("existing admin "+admin, func(t *testing.T) {
			s := newSuite(t, true)
			denom, err := s.app.TokenFactoryKeeper.CreateDenom(s.ctx, types.ModuleAddress().String(), types.SidioraSubdenom)
			require.NoError(t, err)
			server := tokenfactorykeeper.NewMsgServerImpl(s.app.TokenFactoryKeeper)
			_, err = server.ChangeAdmin(sdk.WrapSDKContext(s.ctx), &tokenfactorytypes.MsgChangeAdmin{
				Sender: types.ModuleAddress().String(), Denom: denom, NewAdmin: admin,
			})
			require.NoError(t, err)
			before, found := s.app.BankKeeper.GetDenomMetaData(s.ctx, denom)
			require.True(t, found)
			_, err = s.k.EnsureSidioraDenom(s.ctx, chainID)
			require.ErrorIs(t, err, types.ErrUnauthorized)
			after, found := s.app.BankKeeper.GetDenomMetaData(s.ctx, denom)
			require.True(t, found)
			require.Equal(t, before, after)
			_, found = s.k.GetAsset(s.ctx, chainID, sidioraAsset())
			require.False(t, found)
			metadata, err := s.app.TokenFactoryKeeper.GetAuthorityMetadata(s.ctx, denom)
			require.NoError(t, err)
			require.Equal(t, admin, metadata.Admin)
		})
	}
}

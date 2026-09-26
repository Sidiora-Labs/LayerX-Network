package tests

import (
	"math/big"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	evmtypes "github.com/sidiora-labs/paxeer-network/modules/evm/types"
	bridgetestutil "github.com/sidiora-labs/paxeer-network/modules/layerxbridge/testutil"
	bridgetypes "github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	nodeapp "github.com/sidiora-labs/paxeer-network/node"
	"github.com/sidiora-labs/paxeer-network/precompiles"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/auth/signing"
	"github.com/sidiora-labs/paxeer-network/testutil/processblock"
	"github.com/sidiora-labs/paxeer-network/testutil/processblock/msgs"
	"github.com/sidiora-labs/paxeer-network/testutil/processblock/verify"
	"github.com/stretchr/testify/require"
)

const (
	// sidioraFeeChain is the remote chain the Sidiora asset is registered under.
	sidioraFeeChain = uint64(1)
	// sidioraFeeGasPrice is 500 gwei, above the minimum and below the maximum
	// fee per gas the parameters allow.
	sidioraFeeGasPrice = int64(500_000_000_000)
	// sidioraFeeBridgedIn is one Sidiora, which covers the converted fee.
	sidioraFeeBridgedIn = int64(1_000_000)
	// sidioraFeeShortOfTheFee is less Sidiora than the converted fee.
	sidioraFeeShortOfTheFee = int64(1_000)
	// sidioraFeeConverted is 21000 gas at 500 gwei converted at 3.114 Sidiora
	// per Paxeer coin: 0.0105 Paxeer is 32697 base units of Sidiora.
	sidioraFeeConverted = int64(32_697)
	// sidioraFeeInNetworkCoin is the same gas charged in the network coin:
	// 0.0105 Paxeer is 10500 uhpx.
	sidioraFeeInNetworkCoin = int64(10_500)
	// sidioraFeeFunding is the network coin each signer starts with.
	sidioraFeeFunding = int64(1_000_000_000)
	// sidioraFeeCap is the bridge cap of the Sidiora asset.
	sidioraFeeCap = int64(1_000_000_000_000)
	// sidioraFeeInsufficientFundsCode is the result code of the registered
	// insufficient funds error the fee check refuses with.
	sidioraFeeInsufficientFundsCode = uint32(5)
	// sidioraFeeUnusableRateCode is the result code of an error carrying no
	// registered ABCI code, which is what an unusable rate returns.
	sidioraFeeUnusableRateCode = uint32(1)
	// sidioraFeeRevertedCode is the result code of a transaction whose EVM
	// execution reverted, which is what a refused preference returns.
	sidioraFeeRevertedCode = uint32(45)
)

var (
	sidioraFeeVault     = common.HexToAddress("0x00000000000000000000000000000000000000c1")
	sidioraFeeRecipient = common.HexToAddress("0x00000000000000000000000000000000000000de")
	sidioraFeeTxHash    = common.Hash{0x51, 0xd0, 0x1a}
)

// sidioraFeeChainState is a chain that has Sidiora registered, a rate governed
// for it and one account holding the Sidiora a block bridged in.
type sidioraFeeChainState struct {
	app     *processblock.App
	relayer msgs.FeeTokenSigner
	payer   msgs.FeeTokenSigner
	network msgs.FeeTokenSigner
	denom   string
}

// newSidioraFeeChain brings up a chain with the fee-token switch at enabled,
// the governed rate permitted for maxRateAge blocks, and drives the first block:
// the attested bridge in of bridgedIn base units of Sidiora to the payer.
func newSidioraFeeChain(t *testing.T, enabled bool, maxRateAge int64, bridgedIn int64) *sidioraFeeChainState {
	app := processblock.NewTestApp(t)
	processblock.CommonPreset(app)
	ctx := app.Ctx()

	// The precompiles the fee path runs through are the application's own, built
	// over this application's keepers.
	app.EvmKeeper.SetCustomPrecompiles(
		precompiles.GetCustomPrecompiles(nodeapp.LatestUpgrade, app.GetPrecompileKeepers()),
		nodeapp.LatestUpgrade,
	)

	require.NoError(t, app.LayerXBridgeKeeper.RegisterChain(ctx, bridgetypes.MsgRegisterChain{
		Authority: bridgetypes.DefaultAuthority(),
		Chain: bridgetypes.Chain{
			ChainID:       sidioraFeeChain,
			Vault:         bridgetypes.Address20(sidioraFeeVault),
			FinalityDepth: 64,
			Enabled:       true,
		},
	}))
	denom, err := app.LayerXBridgeKeeper.EnsureSidioraDenom(ctx, sidioraFeeChain)
	require.NoError(t, err)
	require.Equal(t, bridgetypes.SidioraDenom(), denom)
	asset := bridgetypes.Address20(common.HexToAddress(bridgetypes.SidioraRemoteAddress))
	require.NoError(t, app.LayerXBridgeKeeper.SetCap(ctx, bridgetypes.MsgSetCap{
		Authority:   bridgetypes.DefaultAuthority(),
		ChainID:     sidioraFeeChain,
		Asset:       asset,
		MaxInFlight: sdk.NewInt(sidioraFeeCap),
		MaxPerTx:    sdk.NewInt(sidioraFeeCap),
	}))
	attestors := bridgetestutil.Attestors(2)
	require.NoError(t, app.LayerXBridgeKeeper.SetAttestors(ctx, bridgetypes.MsgSetAttestors{
		Authority: bridgetypes.DefaultAuthority(),
		Set:       bridgetestutil.Set(attestors, 1_000, 2),
	}))

	params := app.EvmKeeper.GetParams(ctx)
	params.FeeTokenEnabled = enabled
	params.MaxFeeTokenRateAge = maxRateAge
	params.AllowedFeeDenoms = []evmtypes.AllowedFeeDenom{{
		Denom:            denom,
		Rate:             sdk.NewDec(evmtypes.InitialSidioraBaseUnitsPerPax),
		RateUpdateHeight: 1,
	}}
	app.EvmKeeper.SetParams(ctx, params)

	state := &sidioraFeeChainState{
		app:     app,
		relayer: msgs.FeeTokenSignerFromSeed(1),
		payer:   msgs.FeeTokenSignerFromSeed(2),
		network: msgs.FeeTokenSignerFromSeed(3),
		denom:   denom,
	}
	for _, signer := range []msgs.FeeTokenSigner{state.relayer, state.payer, state.network} {
		app.EvmKeeper.SetAddressMapping(ctx, signer.PaxAddr, signer.EVMAddr)
		app.FundAccount(signer.PaxAddr, sidioraFeeFunding)
	}

	deposit := bridgetypes.BridgeIn{
		ChainID:   sidioraFeeChain,
		Vault:     bridgetypes.Address20(sidioraFeeVault),
		TxHash:    bridgetypes.Hash32(sidioraFeeTxHash),
		LogIndex:  0,
		Recipient: bridgetypes.Hash32(msgs.RecipientWord(state.payer.EVMAddr)),
		Asset:     asset,
		Amount:    big.NewInt(bridgedIn),
	}
	signatures := bridgetestutil.Sign(bridgetypes.InboundDigest(deposit), attestors...)
	require.Equal(t, []uint32{0}, app.RunBlock([]signing.Tx{msgs.SidioraBridgeIn(app, state.relayer, 0,
		sidioraFeePrice(), sidioraFeeChain, sidioraFeeVault, sidioraFeeTxHash, 0, state.payer.EVMAddr,
		common.Address(asset), big.NewInt(bridgedIn), signatures)}))
	require.Equal(t, sdk.NewInt(bridgedIn).String(), state.sidiora(state.payer).String(),
		"the attested deposit mints Sidiora to the payer")

	return state
}

// setPreference drives the block in which the payer chooses its fee denom
// through the fee-token precompile.
func (s *sidioraFeeChainState) setPreference(t *testing.T) {
	s.setPreferenceWithCode(t, 0)
}

// setPreferenceWithCode drives the same block and requires its result code.
func (s *sidioraFeeChainState) setPreferenceWithCode(t *testing.T, code uint32) {
	require.Equal(t, []uint32{code}, s.app.RunBlock([]signing.Tx{
		msgs.FeeDenomPreference(s.app, s.payer, 0, sidioraFeePrice(), s.denom),
	}))
}

func (s *sidioraFeeChainState) sidiora(signer msgs.FeeTokenSigner) sdk.Int {
	return s.app.BankKeeper.GetBalance(s.app.Ctx(), signer.PaxAddr, s.denom).Amount
}

func (s *sidioraFeeChainState) preference() string {
	return s.app.EvmKeeper.GetAccountFeeDenom(s.app.Ctx(), s.payer.EVMAddr)
}

func sidioraFeePrice() *big.Int {
	return big.NewInt(sidioraFeeGasPrice)
}

// TestSidioraFeePaid drives the whole path over real blocks: Sidiora is bridged
// in, the account chooses it, and the block that spends its gas charges it in
// Sidiora while a second account pays the same gas in the network coin.
func TestSidioraFeePaid(t *testing.T) {
	s := newSidioraFeeChain(t, true, evmtypes.DefaultMaxFeeTokenRateAge, sidioraFeeBridgedIn)
	s.setPreference(t)
	require.Equal(t, s.denom, s.preference(), "the account's fee denom is Sidiora")

	testCase := TestCase{
		description: "gas paid in Sidiora beside gas paid in the network coin",
		input: []signing.Tx{
			msgs.FeeTokenTransfer(s.app, s.payer, 1, sidioraFeePrice(), sidioraFeeRecipient),
			msgs.FeeTokenTransfer(s.app, s.network, 0, sidioraFeePrice(), sidioraFeeRecipient),
		},
		verifier: []verify.Verifier{
			verify.FeeTokenBalances(verify.FeeTokenExpectation{
				Denom:            s.denom,
				Payer:            s.payer.EVMAddr,
				FeePaid:          sdk.NewInt(sidioraFeeConverted),
				CollectedTxIndex: 0,
				FeeCollected:     sdk.NewInt(sidioraFeeConverted),
				// The end of the block routes nothing further: the network coin's
				// own sweep is what moves a collected balance on to the fee
				// collector, and it moves the network coin alone.
				FeeRouted:      sdk.ZeroInt(),
				NetworkPayer:   s.network.EVMAddr,
				NetworkFeePaid: sdk.NewInt(sidioraFeeInNetworkCoin),
			}),
		},
		expectedCodes: []uint32{0, 0},
	}
	testCase.run(t, s.app)

	require.Equal(t, sdk.NewInt(sidioraFeeBridgedIn-sidioraFeeConverted).String(), s.sidiora(s.payer).String(),
		"the payer keeps the Sidiora the fee did not take")
}

// TestSidioraFeeRefused drives the refusals of the same path end to end.
func TestSidioraFeeRefused(t *testing.T) {
	t.Run("a preference set while the switch is off", func(t *testing.T) {
		s := newSidioraFeeChain(t, false, evmtypes.DefaultMaxFeeTokenRateAge, sidioraFeeBridgedIn)
		before := s.sidiora(s.payer)
		s.setPreferenceWithCode(t, sidioraFeeRevertedCode)
		require.Equal(t, s.app.EvmKeeper.GetBaseDenom(s.app.Ctx()), s.preference(),
			"the refused preference leaves the account on the network coin")
		require.Equal(t, before.String(), s.sidiora(s.payer).String(),
			"the refused preference spends no Sidiora")
	})

	t.Run("a payer whose Sidiora cannot cover the fee", func(t *testing.T) {
		s := newSidioraFeeChain(t, true, evmtypes.DefaultMaxFeeTokenRateAge, sidioraFeeShortOfTheFee)
		s.setPreference(t)
		require.Equal(t, s.denom, s.preference())

		testCase := TestCase{
			description: "a transaction whose fee denom cannot cover its gas",
			input: []signing.Tx{
				msgs.FeeTokenTransfer(s.app, s.payer, 1, sidioraFeePrice(), sidioraFeeRecipient),
			},
			verifier: []verify.Verifier{
				verify.FeeTokenBalances(verify.FeeTokenExpectation{
					Denom: s.denom,
					Payer: s.payer.EVMAddr,
				}),
			},
			expectedCodes: []uint32{sidioraFeeInsufficientFundsCode},
		}
		testCase.run(t, s.app)
		require.Equal(t, sdk.NewInt(sidioraFeeShortOfTheFee).String(), s.sidiora(s.payer).String())
	})

	t.Run("a block whose pair has no rate", func(t *testing.T) {
		// The governed rate was set at height one and may be a single block old,
		// so by the third block the pair the account prefers has no rate to
		// convert its fee at.
		s := newSidioraFeeChain(t, true, 1, sidioraFeeBridgedIn)
		s.setPreference(t)
		require.Equal(t, s.denom, s.preference())

		testCase := TestCase{
			description: "a transaction whose fee denom has no rate",
			input: []signing.Tx{
				msgs.FeeTokenTransfer(s.app, s.payer, 1, sidioraFeePrice(), sidioraFeeRecipient),
			},
			verifier: []verify.Verifier{
				verify.FeeTokenBalances(verify.FeeTokenExpectation{
					Denom: s.denom,
					Payer: s.payer.EVMAddr,
				}),
			},
			expectedCodes: []uint32{sidioraFeeUnusableRateCode},
		}
		testCase.run(t, s.app)
		require.Equal(t, sdk.NewInt(sidioraFeeBridgedIn).String(), s.sidiora(s.payer).String())
	})
}

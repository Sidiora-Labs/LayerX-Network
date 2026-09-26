package verify

import (
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	evmtypes "github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/auth/signing"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
	distrtypes "github.com/sidiora-labs/paxeer-network/sdk/x/distribution/types"
	"github.com/sidiora-labs/paxeer-network/testutil/processblock"
	"github.com/stretchr/testify/require"
)

// FeeTokenExpectation is the ledger one block moves when an account's own fee
// denom pays for its gas. An unset amount is zero, which asserts that nothing
// moved.
type FeeTokenExpectation struct {
	// Denom is the fee denom the payer prefers.
	Denom string
	// Payer is the EVM address whose preference is Denom.
	Payer common.Address
	// FeePaid is the fee-denom amount the payer's balance falls by.
	FeePaid sdk.Int
	// CollectedTxIndex is the index of the payer's transaction in the block,
	// which names the fee collection address its gas reward reaches.
	CollectedTxIndex int
	// FeeCollected is the fee-denom amount that collection address receives.
	FeeCollected sdk.Int
	// FeeRouted is the fee-denom amount the fee collector and the fee-token
	// holding module account hold more of once the block has routed them.
	FeeRouted sdk.Int
	// NetworkPayer is an account paying its own gas in the network coin, or the
	// zero address when the block carries none.
	NetworkPayer common.Address
	// NetworkFeePaid is the network-coin amount NetworkPayer's balance falls by.
	NetworkFeePaid sdk.Int
}

// FeeTokenBalances is the verifier of a fee-token expectation, for a test case
// that carries a list of them.
func FeeTokenBalances(expectation FeeTokenExpectation) Verifier {
	return func(t *testing.T, app *processblock.App, f BlockRunnable, _ []signing.Tx) BlockRunnable {
		return VerifyFeeTokenBalances(t, app, f, expectation)
	}
}

// VerifyFeeTokenBalances asserts across one block that the payer paid its gas
// in its own fee denom and nothing else: its fee-denom balance fell by the
// converted fee, the block's fee collection address and the accounts that fee
// is routed to received it, the payer's network-coin balance did not move, an
// account paying in the network coin was charged in the network coin alone, and
// no validator reward carries the fee denom.
func VerifyFeeTokenBalances(t *testing.T, app *processblock.App, f BlockRunnable,
	expectation FeeTokenExpectation) BlockRunnable {
	return func() []uint32 {
		baseDenom := app.EvmKeeper.GetBaseDenom(app.Ctx())
		payer := app.EvmKeeper.GetPaxAddressOrDefault(app.Ctx(), expectation.Payer)
		collection := state.GetCoinbaseAddress(expectation.CollectedTxIndex)
		collector := app.AccountKeeper.GetModuleAddress(authtypes.FeeCollectorName)
		holding := app.AccountKeeper.GetModuleAddress(evmtypes.FeeTokenHoldingAccount)
		require.NotNil(t, holding, "the fee-token holding module account is registered")

		feeBefore := feeTokenBalance(app, payer, expectation.Denom)
		payerBaseBefore := feeTokenBalance(app, payer, baseDenom)
		payerWeiBefore := app.BankKeeper.GetWeiBalance(app.Ctx(), payer)
		collectionBefore := feeTokenBalance(app, collection, expectation.Denom)
		routedBefore := feeTokenBalance(app, collector, expectation.Denom).
			Add(feeTokenBalance(app, holding, expectation.Denom))

		var networkPayer sdk.AccAddress
		networkBaseBefore, networkFeeBefore := sdk.ZeroInt(), sdk.ZeroInt()
		if expectation.NetworkPayer != (common.Address{}) {
			networkPayer = app.EvmKeeper.GetPaxAddressOrDefault(app.Ctx(), expectation.NetworkPayer)
			networkBaseBefore = feeTokenBalance(app, networkPayer, baseDenom)
			networkFeeBefore = feeTokenBalance(app, networkPayer, expectation.Denom)
		}

		results := f()

		feePaid := feeTokenAmount(expectation.FeePaid)
		require.Equal(t, feeBefore.Sub(feePaid).String(),
			feeTokenBalance(app, payer, expectation.Denom).String(),
			"the payer's %s balance falls by the converted fee", expectation.Denom)
		require.Equal(t, payerBaseBefore.String(), feeTokenBalance(app, payer, baseDenom).String(),
			"the payer's %s balance does not move", baseDenom)
		require.Equal(t, payerWeiBefore.String(), app.BankKeeper.GetWeiBalance(app.Ctx(), payer).String(),
			"the payer's wei balance does not move")

		require.Equal(t, collectionBefore.Add(feeTokenAmount(expectation.FeeCollected)).String(),
			feeTokenBalance(app, collection, expectation.Denom).String(),
			"the fee collection address of transaction %d receives the converted fee", expectation.CollectedTxIndex)
		require.Equal(t, routedBefore.Add(feeTokenAmount(expectation.FeeRouted)).String(),
			feeTokenBalance(app, collector, expectation.Denom).
				Add(feeTokenBalance(app, holding, expectation.Denom)).String(),
			"the fee collector and the holding account together hold the routed fee")

		if networkPayer != nil {
			require.Equal(t, networkBaseBefore.Sub(feeTokenAmount(expectation.NetworkFeePaid)).String(),
				feeTokenBalance(app, networkPayer, baseDenom).String(),
				"the network-coin payer is charged in %s exactly as before", baseDenom)
			require.Equal(t, networkFeeBefore.String(),
				feeTokenBalance(app, networkPayer, expectation.Denom).String(),
				"the network-coin payer's %s balance does not move", expectation.Denom)
		}

		app.DistrKeeper.IterateValidatorOutstandingRewards(
			app.Ctx(),
			func(val sdk.ValAddress, rewards distrtypes.ValidatorOutstandingRewards) (stop bool) {
				for _, reward := range rewards.Rewards {
					require.NotEqual(t, expectation.Denom, reward.Denom,
						"validator %s is rewarded in %s", val.String(), expectation.Denom)
				}
				return false
			},
		)

		return results
	}
}

func feeTokenBalance(app *processblock.App, account sdk.AccAddress, denom string) sdk.Int {
	return app.BankKeeper.GetBalance(app.Ctx(), account, denom).Amount
}

func feeTokenAmount(amount sdk.Int) sdk.Int {
	if amount.IsNil() {
		return sdk.ZeroInt()
	}
	return amount
}

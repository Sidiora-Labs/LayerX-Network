package keeper

import (
	"fmt"

	"github.com/sidiora-labs/paxeer-network/modules/launchpad/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

const SolvencyInvariantName = "launchpad-solvency"

// RegisterInvariants registers the launchpad invariants.
func RegisterInvariants(registry sdk.InvariantRegistry, k *Keeper) {
	registry.RegisterRoute(types.ModuleName, SolvencyInvariantName, SolvencyInvariant(k))
}

// SolvencyInvariant checks that the launchpad account holds every market's
// real quote balance, undistributed fees and airdrop balance, and each
// market's token reserve.
func SolvencyInvariant(k *Keeper) sdk.Invariant {
	return func(ctx sdk.Context) (string, bool) {
		params := k.GetParams(ctx)
		escrow := k.ModuleAddress()
		owedQuote := sdk.ZeroInt()
		message, broken := "", false
		k.IterateMarkets(ctx, func(market types.Market) bool {
			owedQuote = owedQuote.Add(market.RealQuoteBalance).Add(market.AccumulatedFees).Add(market.AirdropBalance)
			held := k.bankKeeper.GetBalance(ctx, escrow, market.Denom).Amount
			if held.LT(market.TokenReserve) {
				broken = true
				message += fmt.Sprintf("market %s: token balance %s below reserve %s\n", market.Denom, held, market.TokenReserve)
			}
			return false
		})
		quote := k.bankKeeper.GetBalance(ctx, escrow, params.QuoteDenom).Amount
		if quote.LT(owedQuote) {
			broken = true
			message += fmt.Sprintf("quote balance %s below liabilities %s\n", quote, owedQuote)
		}
		return sdk.FormatInvariant(types.ModuleName, SolvencyInvariantName, message), broken
	}
}

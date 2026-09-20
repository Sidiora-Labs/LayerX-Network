package keeper

import (
	"fmt"

	"github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

const SolvencyInvariantName = "custody-solvency"

// RegisterInvariants registers the custody invariants.
func RegisterInvariants(registry sdk.InvariantRegistry, k *Keeper) {
	registry.RegisterRoute(types.ModuleName, SolvencyInvariantName, SolvencyInvariant(k))
}

// SolvencyInvariant checks, per asset, that the module account holds at least
// the outstanding liabilities (deposited and not yet released), that queued
// claims never exceed them, and that the queued total equals the pending
// claims on record.
func SolvencyInvariant(k *Keeper) sdk.Invariant {
	return func(ctx sdk.Context) (string, bool) {
		queued := map[string]sdk.Int{}
		k.IterateClaims(ctx, func(claim types.Claim) bool {
			if claim.Status == types.ClaimStatus_CLAIM_STATUS_PENDING {
				amount, _ := sdk.NewIntFromString(claim.Amount)
				if current, ok := queued[claim.AssetId]; ok {
					amount = amount.Add(current)
				}
				queued[claim.AssetId] = amount
			}
			return false
		})
		message, broken := "", false
		k.IterateAssets(ctx, func(asset types.AssetMapping) bool {
			assetID, _ := types.ParseHash32(asset.AssetId)
			custodied, _, pending := k.totals(ctx, assetID)
			balance := k.bankKeeper.GetBalance(ctx, k.ModuleAddress(), asset.Denom).Amount
			expected, ok := queued[asset.AssetId]
			if !ok {
				expected = sdk.ZeroInt()
			}
			if balance.LT(custodied) || pending.GT(custodied) || !pending.Equal(expected) {
				broken = true
				message += fmt.Sprintf("asset %s: balance %s, liabilities %s, queued %s, pending claims %s\n",
					asset.AssetId, balance, custodied, pending, expected)
			}
			return false
		})
		return sdk.FormatInvariant(types.ModuleName, SolvencyInvariantName, message), broken
	}
}

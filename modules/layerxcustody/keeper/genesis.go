package keeper

import (
	"encoding/binary"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	"github.com/sidiora-labs/paxeer-network/sdk/store/prefix"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// InitGenesis loads a validated genesis state. The module account is created
// here so custody has an address before the first deposit.
func (k *Keeper) InitGenesis(ctx sdk.Context, gs types.GenesisState) {
	if err := gs.Validate(); err != nil {
		panic(err)
	}
	k.accountKeeper.GetModuleAccount(ctx, types.ModuleName)
	if err := k.SetParams(ctx, gs.Params); err != nil {
		panic(err)
	}
	for _, asset := range gs.Assets {
		if err := k.SetAsset(ctx, asset); err != nil {
			panic(err)
		}
	}
	k.setDepositCount(ctx, gs.DepositCount)
	for _, deposit := range gs.Deposits {
		k.setDeposit(ctx, deposit)
	}
	for _, nonce := range gs.DepositNonces {
		depositor, _ := types.ParseAddress(nonce.Depositor)
		assetID, _ := types.ParseHash32(nonce.AssetId)
		k.setDepositNonce(ctx, depositor, assetID, nonce.Nonce)
	}
	for _, claim := range gs.Claims {
		k.setClaim(ctx, claim)
	}
	for _, nullifier := range gs.Nullifiers {
		k.setNullifier(ctx, nullifier)
	}
	for _, checkpoint := range gs.Checkpoints {
		k.setCheckpoint(ctx, checkpoint)
	}
	for _, consumed := range gs.ConsumedBalances {
		account, _ := types.ParseHash32(consumed.Account)
		assetID, _ := types.ParseHash32(consumed.AssetId)
		anchor, _ := types.ParseHash32(consumed.Anchor)
		k.store(ctx).Set(types.ConsumedKey(account, assetID, anchor), []byte{1})
	}
	for _, totals := range gs.Totals {
		assetID, _ := types.ParseHash32(totals.AssetId)
		k.store(ctx).Set(types.AssetTotalsKey(assetID), k.cdc.MustMarshal(&totals))
	}
	if gs.Emergency {
		k.store(ctx).Set(types.EmergencyKey, []byte{1})
	}
}

func (k *Keeper) iterateKeys(ctx sdk.Context, keyPrefix []byte, visit func(key, value []byte)) {
	iterator := prefix.NewStore(k.store(ctx), keyPrefix).Iterator(nil, nil)
	defer func() { _ = iterator.Close() }()
	for ; iterator.Valid(); iterator.Next() {
		visit(iterator.Key(), iterator.Value())
	}
}

// ExportGenesis exports the full custody state in store order.
func (k *Keeper) ExportGenesis(ctx sdk.Context) *types.GenesisState {
	gs := &types.GenesisState{Params: k.GetParams(ctx), DepositCount: k.GetDepositCount(ctx), Emergency: k.GetEmergency(ctx)}
	k.IterateAssets(ctx, func(asset types.AssetMapping) bool { gs.Assets = append(gs.Assets, asset); return false })
	k.IterateDeposits(ctx, func(deposit types.Deposit) bool { gs.Deposits = append(gs.Deposits, deposit); return false })
	k.iterateKeys(ctx, types.DepositNoncePrefix, func(key, value []byte) {
		var assetID [32]byte
		copy(assetID[:], key[20:])
		gs.DepositNonces = append(gs.DepositNonces, types.DepositNonce{Depositor: types.Address(common.BytesToAddress(key[:20])),
			AssetId: types.Hash32(assetID), Nonce: binary.BigEndian.Uint64(value)})
	})
	k.IterateClaims(ctx, func(claim types.Claim) bool { gs.Claims = append(gs.Claims, claim); return false })
	k.IterateNullifiers(ctx, func(nullifier types.Nullifier) bool { gs.Nullifiers = append(gs.Nullifiers, nullifier); return false })
	k.IterateCheckpoints(ctx, func(checkpoint types.Checkpoint) bool {
		gs.Checkpoints = append(gs.Checkpoints, checkpoint)
		return false
	})
	k.iterateKeys(ctx, types.ConsumedPrefix, func(key, _ []byte) {
		var account, assetID, anchor [32]byte
		copy(account[:], key[:32])
		copy(assetID[:], key[32:64])
		copy(anchor[:], key[64:])
		gs.ConsumedBalances = append(gs.ConsumedBalances, types.ConsumedBalance{Account: types.Hash32(account),
			AssetId: types.Hash32(assetID), Anchor: types.Hash32(anchor)})
	})
	k.IterateTotals(ctx, func(totals types.AssetTotals) bool { gs.Totals = append(gs.Totals, totals); return false })
	return gs
}

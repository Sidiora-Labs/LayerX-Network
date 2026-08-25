package app_test

import (
	"fmt"
	"testing"
	"time"

	app "github.com/sidiora-labs/paxeer-network/node"
	"github.com/sidiora-labs/paxeer-network/sdk/crypto/keys/secp256k1"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/stretchr/testify/require"
)

func TestLightInvarianceChecks(t *testing.T) {
	tm := time.Now().UTC()
	valPub := secp256k1.GenPrivKey().PubKey()
	accounts := []sdk.AccAddress{
		sdk.AccAddress(secp256k1.GenPrivKey().PubKey().Address()),
		sdk.AccAddress(secp256k1.GenPrivKey().PubKey().Address()),
	}
	uhpxCoin := func(i int64) sdk.Coin { return sdk.NewCoin("uhpx", sdk.NewInt(i)) }
	uhpxCoins := func(i int64) sdk.Coins { return sdk.NewCoins(uhpxCoin(i)) }
	for i, tt := range []struct {
		preUhpx    []int64
		preWei     []int64
		preSupply  int64
		postUhpx   []int64
		postWei    []int64
		postSupply int64
		success    bool
	}{
		{
			preUhpx:    []int64{0, 0},
			preWei:     []int64{0, 0},
			preSupply:  5,
			postUhpx:   []int64{1, 2},
			postWei:    []int64{0, 0},
			postSupply: 8,
			success:    true,
		},
		{
			preUhpx:    []int64{2, 1},
			preWei:     []int64{0, 0},
			preSupply:  3,
			postUhpx:   []int64{0, 0},
			postWei:    []int64{0, 0},
			postSupply: 0,
			success:    true,
		},
		{
			preUhpx:    []int64{1, 0},
			preWei:     []int64{0, 0},
			preSupply:  10,
			postUhpx:   []int64{0, 1},
			postWei:    []int64{0, 0},
			postSupply: 10,
			success:    true,
		},
		{
			preUhpx:    []int64{1, 0},
			preWei:     []int64{0, 0},
			preSupply:  10,
			postUhpx:   []int64{0, 0},
			postWei:    []int64{500_000_000_000, 500_000_000_000},
			postSupply: 10,
			success:    true,
		},
		{
			preUhpx:    []int64{0, 0},
			preWei:     []int64{500_000_000_000, 500_000_000_000},
			preSupply:  10,
			postUhpx:   []int64{1, 0},
			postWei:    []int64{0, 0},
			postSupply: 10,
			success:    true,
		},
		{
			preUhpx:    []int64{0, 0},
			preWei:     []int64{1, 2},
			preSupply:  10,
			postUhpx:   []int64{0, 0},
			postWei:    []int64{2, 1},
			postSupply: 10,
			success:    true,
		},
		{
			preUhpx:    []int64{1, 0},
			preWei:     []int64{0, 0},
			preSupply:  10,
			postUhpx:   []int64{1, 1},
			postWei:    []int64{0, 0},
			postSupply: 10,
			success:    false,
		},
		{
			preUhpx:    []int64{1, 0},
			preWei:     []int64{0, 0},
			preSupply:  10,
			postUhpx:   []int64{0, 0},
			postWei:    []int64{0, 0},
			postSupply: 10,
			success:    false,
		},
		{
			preUhpx:    []int64{1, 0},
			preWei:     []int64{0, 0},
			preSupply:  10,
			postUhpx:   []int64{0, 1},
			postWei:    []int64{500_000_000_000, 500_000_000_000},
			postSupply: 10,
			success:    false,
		},
		{
			preUhpx:    []int64{1, 0},
			preWei:     []int64{500_000_000_000, 500_000_000_000},
			preSupply:  10,
			postUhpx:   []int64{0, 1},
			postWei:    []int64{0, 0},
			postSupply: 10,
			success:    false,
		},
		{
			preUhpx:    []int64{0, 0},
			preWei:     []int64{1, 2},
			preSupply:  10,
			postUhpx:   []int64{0, 0},
			postWei:    []int64{2, 2},
			postSupply: 10,
			success:    false,
		},
		{
			preUhpx:    []int64{0, 0},
			preWei:     []int64{1, 2},
			preSupply:  10,
			postUhpx:   []int64{0, 0},
			postWei:    []int64{1, 1},
			postSupply: 10,
			success:    false,
		},
	} {
		fmt.Printf("Running test %d\n", i)
		testWrapper := app.NewTestWrapperWithSc(t, tm, valPub, false)
		a, ctx := testWrapper.App, testWrapper.Ctx
		for i := range tt.preUhpx {
			if tt.preUhpx[i] > 0 {
				a.BankKeeper.AddCoins(ctx, accounts[i], uhpxCoins(tt.preUhpx[i]), false)
			}
			if tt.preWei[i] > 0 {
				a.BankKeeper.AddWei(ctx, accounts[i], sdk.NewInt(tt.preWei[i]))
			}
		}
		if tt.preSupply > 0 {
			a.BankKeeper.SetSupply(ctx, uhpxCoin(tt.preSupply))
		}
		a.SetDeliverStateToCommit()
		a.WriteState()
		a.GetWorkingHash() // flush to sc
		for i := range tt.postUhpx {
			uhpxDiff := tt.postUhpx[i] - tt.preUhpx[i]
			if uhpxDiff > 0 {
				a.BankKeeper.AddCoins(ctx, accounts[i], uhpxCoins(uhpxDiff), false)
			} else if uhpxDiff < 0 {
				a.BankKeeper.SubUnlockedCoins(ctx, accounts[i], uhpxCoins(-uhpxDiff), false)
			}

			weiDiff := tt.postWei[i] - tt.preWei[i]
			if weiDiff > 0 {
				a.BankKeeper.AddWei(ctx, accounts[i], sdk.NewInt(weiDiff))
			} else if weiDiff < 0 {
				a.BankKeeper.SubWei(ctx, accounts[i], sdk.NewInt(-weiDiff))
			}
		}
		a.BankKeeper.SetSupply(ctx, uhpxCoin(tt.postSupply))
		a.SetDeliverStateToCommit()
		f := func() {
			a.LightInvarianceChecks(t.Context(), a.WriteState(), app.LightInvarianceConfig{SupplyEnabled: true})
		}
		if tt.success {
			require.NotPanics(t, f)
		} else {
			require.Panics(t, f)
		}
		safeClose(a)
	}
}

// TODO: remove once snapshot manager can be closed gracefully in tests
func safeClose(a *app.App) {
	defer func() {
		_ = recover()
	}()
	a.Close()
}

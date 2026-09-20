package keeper_test

import (
	"bytes"
	"testing"

	evmkeeper "github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func TestSetGetAddressMapping(t *testing.T) {
	k := &keeper.EVMTestApp.EvmKeeper
	ctx := keeper.EVMTestApp.GetContextForDeliverTx([]byte{})
	paxAddr, evmAddr := keeper.MockAddressPair()
	_, ok := k.GetEVMAddress(ctx, paxAddr)
	require.False(t, ok)
	_, ok = k.GetPaxAddress(ctx, evmAddr)
	require.False(t, ok)
	k.SetAddressMapping(ctx, paxAddr, evmAddr)
	foundEVM, ok := k.GetEVMAddress(ctx, paxAddr)
	require.True(t, ok)
	require.Equal(t, evmAddr, foundEVM)
	foundPax, ok := k.GetPaxAddress(ctx, evmAddr)
	require.True(t, ok)
	require.Equal(t, paxAddr, foundPax)
	require.Equal(t, paxAddr, k.AccountKeeper().GetAccount(ctx, paxAddr).GetAddress())
}

func TestDeleteAddressMapping(t *testing.T) {
	k := &keeper.EVMTestApp.EvmKeeper
	ctx := keeper.EVMTestApp.GetContextForDeliverTx([]byte{})
	paxAddr, evmAddr := keeper.MockAddressPair()
	k.SetAddressMapping(ctx, paxAddr, evmAddr)
	foundEVM, ok := k.GetEVMAddress(ctx, paxAddr)
	require.True(t, ok)
	require.Equal(t, evmAddr, foundEVM)
	foundPax, ok := k.GetPaxAddress(ctx, evmAddr)
	require.True(t, ok)
	require.Equal(t, paxAddr, foundPax)
	k.DeleteAddressMapping(ctx, paxAddr, evmAddr)
	_, ok = k.GetEVMAddress(ctx, paxAddr)
	require.False(t, ok)
	_, ok = k.GetPaxAddress(ctx, evmAddr)
	require.False(t, ok)
}

func TestGetAddressOrDefault(t *testing.T) {
	k := &keeper.EVMTestApp.EvmKeeper
	ctx := keeper.EVMTestApp.GetContextForDeliverTx([]byte{})
	paxAddr, evmAddr := keeper.MockAddressPair()
	defaultEvmAddr := k.GetEVMAddressOrDefault(ctx, paxAddr)
	require.True(t, bytes.Equal(paxAddr, defaultEvmAddr[:]))
	defaultPaxAddr := k.GetPaxAddressOrDefault(ctx, evmAddr)
	require.True(t, bytes.Equal(defaultPaxAddr, evmAddr[:]))
}

func TestSendingToCastAddress(t *testing.T) {
	a := keeper.EVMTestApp
	ctx := a.GetContextForDeliverTx([]byte{})
	paxAddr, evmAddr := keeper.MockAddressPair()
	castAddr := sdk.AccAddress(evmAddr[:])
	sourceAddr, _ := keeper.MockAddressPair()
	require.Nil(t, a.BankKeeper.MintCoins(ctx, "evm", sdk.NewCoins(sdk.NewCoin("uhpx", sdk.NewInt(10)))))
	require.Nil(t, a.BankKeeper.SendCoinsFromModuleToAccount(ctx, "evm", sourceAddr, sdk.NewCoins(sdk.NewCoin("uhpx", sdk.NewInt(5)))))
	amt := sdk.NewCoins(sdk.NewCoin("uhpx", sdk.NewInt(1)))
	require.Nil(t, a.BankKeeper.SendCoinsFromModuleToAccount(ctx, "evm", castAddr, amt))
	require.Nil(t, a.BankKeeper.SendCoins(ctx, sourceAddr, castAddr, amt))
	require.Nil(t, a.BankKeeper.SendCoinsAndWei(ctx, sourceAddr, castAddr, sdk.OneInt(), sdk.OneInt()))

	a.EvmKeeper.SetAddressMapping(ctx, paxAddr, evmAddr)
	require.NotNil(t, a.BankKeeper.SendCoinsFromModuleToAccount(ctx, "evm", castAddr, amt))
	require.NotNil(t, a.BankKeeper.SendCoins(ctx, sourceAddr, castAddr, amt))
	require.NotNil(t, a.BankKeeper.SendCoinsAndWei(ctx, sourceAddr, castAddr, sdk.OneInt(), sdk.OneInt()))
}

func TestEvmAddressHandler_GetPaxAddressFromString(t *testing.T) {
	a := keeper.EVMTestApp
	ctx := a.GetContextForDeliverTx([]byte{})
	paxAddr, evmAddr := keeper.MockAddressPair()
	a.EvmKeeper.SetAddressMapping(ctx, paxAddr, evmAddr)

	_, notAssociatedEvmAddr := keeper.MockAddressPair()
	castAddr := sdk.AccAddress(notAssociatedEvmAddr[:])

	type args struct {
		ctx     sdk.Context
		address string
	}
	tests := []struct {
		name       string
		args       args
		want       sdk.AccAddress
		wantErr    bool
		wantErrMsg string
	}{
		{
			name: "returns associated Pax address if input address is a valid 0x and associated",
			args: args{
				ctx:     ctx,
				address: evmAddr.String(),
			},
			want: paxAddr,
		},
		{
			name: "returns default Pax address if input address is a valid 0x not associated",
			args: args{
				ctx:     ctx,
				address: notAssociatedEvmAddr.String(),
			},
			want: castAddr,
		},
		{
			name: "returns Pax address if input address is a valid bech32 address",
			args: args{
				ctx:     ctx,
				address: paxAddr.String(),
			},
			want: paxAddr,
		},
		{
			name: "returns error if address is invalid",
			args: args{
				ctx:     ctx,
				address: "invalid",
			},
			wantErr:    true,
			wantErrMsg: "decoding bech32 failed: invalid bech32 string length 7",
		}, {
			name: "returns error if address is empty",
			args: args{
				ctx:     ctx,
				address: "",
			},
			wantErr:    true,
			wantErrMsg: "empty address string is not allowed",
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			h := evmkeeper.NewEvmAddressHandler(&a.EvmKeeper)
			got, err := h.GetPaxAddressFromString(tt.args.ctx, tt.args.address)
			if tt.wantErr {
				require.NotNil(t, err)
				require.Equal(t, tt.wantErrMsg, err.Error())
				return
			} else {
				require.NoError(t, err)
				require.Equal(t, tt.want, got)
			}
		})
	}
}

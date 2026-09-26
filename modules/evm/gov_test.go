package evm_test

import (
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/evm"
	"github.com/sidiora-labs/paxeer-network/modules/evm/artifacts/native"
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func TestAddERCNativePointerProposalsV2(t *testing.T) {
	k := &testkeeper.EVMTestApp.EvmKeeper
	ctx := testkeeper.EVMTestApp.GetContextForDeliverTx(nil)
	require.Nil(t, evm.HandleAddERCNativePointerProposalV2(ctx, k, &types.AddERCNativePointerProposalV2{
		Token:    "test",
		Name:     "NAME",
		Symbol:   "SYMBOL",
		Decimals: 6,
	}))
	pointer, _, exists := k.GetERC20NativePointer(ctx, "test")
	require.True(t, exists)
	qName, _ := native.GetParsedABI().Pack("name")
	resName, err := k.StaticCallEVM(ctx, k.AccountKeeper().GetModuleAddress(types.ModuleName), &pointer, qName)
	require.Nil(t, err)
	oName, _ := native.GetParsedABI().Unpack("name", resName)
	require.Equal(t, "NAME", oName[0].(string))
	qSymbol, _ := native.GetParsedABI().Pack("symbol")
	resSymbol, err := k.StaticCallEVM(ctx, k.AccountKeeper().GetModuleAddress(types.ModuleName), &pointer, qSymbol)
	require.Nil(t, err)
	oSymbol, _ := native.GetParsedABI().Unpack("symbol", resSymbol)
	require.Equal(t, "SYMBOL", oSymbol[0].(string))
	qDecimals, _ := native.GetParsedABI().Pack("decimals")
	resDecimals, err := k.StaticCallEVM(ctx, k.AccountKeeper().GetModuleAddress(types.ModuleName), &pointer, qDecimals)
	require.Nil(t, err)
	oDecimals, _ := native.GetParsedABI().Unpack("decimals", resDecimals)
	require.Equal(t, uint8(6), oDecimals[0].(uint8))

	// make sure pointers deployed this way won't collide in address
	require.Nil(t, evm.HandleAddERCNativePointerProposalV2(ctx, k, &types.AddERCNativePointerProposalV2{
		Token:    "test2",
		Name:     "NAME2",
		Symbol:   "SYMBOL2",
		Decimals: 6,
	}))
	pointer2, _, exists2 := k.GetERC20NativePointer(ctx, "test2")
	require.True(t, exists2)
	require.NotEqual(t, pointer, pointer2)
}

// pointerBindingContext returns a fresh application's keeper and a deliver
// context whose height is past the native pointer binding upgrade.
func pointerBindingContext(t *testing.T) (*keeper.Keeper, sdk.Context) {
	t.Helper()
	k, ctx := testkeeper.MockEVMKeeper(t)
	k.UpgradeKeeper().SetDone(ctx.WithBlockHeight(ctx.BlockHeight()-1), types.BindERCNativePointerUpgrade)
	return k, ctx
}

func TestPointerBindingProposalBindsThroughTheProposalHandler(t *testing.T) {
	k, ctx := pointerBindingContext(t)
	pointer := common.HexToAddress("0x00000000000000000000000000000000000b1d11")
	k.SetCode(ctx, pointer, native.GetBin())
	const denom = "factory/pax1dzfx9mk4fl9kl2mysjmtvk2xp75ljumk6nynhf/ugovbind"

	proposal, err := types.NewPointerBindingProposal("Bind", "Bind a deployed ERC20",
		types.NewMsgBindERCNativePointer(types.GovernanceAuthority(), denom, pointer, 1))
	require.NoError(t, err)
	require.NoError(t, evm.NewProposalHandler(*k)(ctx, proposal))

	bound, version, exists := k.GetERC20NativePointer(ctx, denom)
	require.True(t, exists)
	require.Equal(t, pointer, bound)
	require.Equal(t, uint16(1), version)
	pointee, _, exists := k.GetAnyPointerInfo(ctx, types.PointerReverseRegistryKey(pointer))
	require.True(t, exists)
	require.Equal(t, denom, string(pointee))
}

func TestPointerBindingProposalChangesNothingWhenOneMessageFails(t *testing.T) {
	k, ctx := pointerBindingContext(t)
	first := common.HexToAddress("0x00000000000000000000000000000000000b1d12")
	withoutCode := common.HexToAddress("0x00000000000000000000000000000000000b1d13")
	k.SetCode(ctx, first, native.GetBin())
	const firstDenom = "factory/pax1dzfx9mk4fl9kl2mysjmtvk2xp75ljumk6nynhf/ugovfirst"
	const secondDenom = "factory/pax1dzfx9mk4fl9kl2mysjmtvk2xp75ljumk6nynhf/ugovsecond"

	proposal, err := types.NewPointerBindingProposal("Bind", "Bind two deployed ERC20s",
		types.NewMsgBindERCNativePointer(types.GovernanceAuthority(), firstDenom, first, 1),
		types.NewMsgBindERCNativePointer(types.GovernanceAuthority(), secondDenom, withoutCode, 1))
	require.NoError(t, err)
	err = evm.NewProposalHandler(*k)(ctx, proposal)
	require.ErrorContains(t, err, "message 1")
	require.ErrorContains(t, err, "no contract code")

	_, _, exists := k.GetERC20NativePointer(ctx, firstDenom)
	require.False(t, exists)
	_, _, exists = k.GetERC20NativePointer(ctx, secondDenom)
	require.False(t, exists)
}

func TestPointerBindingProposalRefusesAMessageForAnotherAuthority(t *testing.T) {
	k, ctx := pointerBindingContext(t)
	pointer := common.HexToAddress("0x00000000000000000000000000000000000b1d14")
	k.SetCode(ctx, pointer, native.GetBin())
	const denom = "factory/pax1dzfx9mk4fl9kl2mysjmtvk2xp75ljumk6nynhf/ugovother"

	proposal, err := types.NewPointerBindingProposal("Bind", "Bind for another authority",
		types.NewMsgBindERCNativePointer(sdk.AccAddress(pointer.Bytes()).String(), denom, pointer, 1))
	require.NoError(t, err)
	require.ErrorIs(t, evm.NewProposalHandler(*k)(ctx, proposal), sdkerrors.ErrUnauthorized)
	_, _, exists := k.GetERC20NativePointer(ctx, denom)
	require.False(t, exists)
}

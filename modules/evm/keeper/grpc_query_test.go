package keeper_test

import (
	"errors"
	"testing"
	"time"

	"github.com/sidiora-labs/paxeer-network/modules/evm/artifacts/cw1155"
	"github.com/sidiora-labs/paxeer-network/modules/evm/artifacts/cw20"
	"github.com/sidiora-labs/paxeer-network/modules/evm/artifacts/cw721"
	"github.com/sidiora-labs/paxeer-network/modules/evm/artifacts/erc1155"
	"github.com/sidiora-labs/paxeer-network/modules/evm/artifacts/erc20"
	"github.com/sidiora-labs/paxeer-network/modules/evm/artifacts/erc721"
	"github.com/sidiora-labs/paxeer-network/modules/evm/artifacts/native"
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func TestQueryPointer(t *testing.T) {
	k := &testkeeper.EVMTestApp.EvmKeeper
	ctx := testkeeper.EVMTestApp.GetContextForDeliverTx([]byte{}).WithBlockTime(time.Now())
	paxAddr1, evmAddr1 := testkeeper.MockAddressPair()
	paxAddr2, evmAddr2 := testkeeper.MockAddressPair()
	paxAddr3, evmAddr3 := testkeeper.MockAddressPair()
	paxAddr4, evmAddr4 := testkeeper.MockAddressPair()
	paxAddr5, evmAddr5 := testkeeper.MockAddressPair()
	paxAddr6, evmAddr6 := testkeeper.MockAddressPair()
	paxAddr7, evmAddr7 := testkeeper.MockAddressPair()
	_, evmAddr8 := testkeeper.MockAddressPair()
	goCtx := sdk.WrapSDKContext(ctx)
	k.SetERC20NativePointer(ctx, paxAddr1.String(), evmAddr1)
	k.SetERC20CW20Pointer(ctx, paxAddr2.String(), evmAddr2)
	k.SetERC721CW721Pointer(ctx, paxAddr3.String(), evmAddr3)
	k.SetCW20ERC20Pointer(ctx, evmAddr4, paxAddr4.String())
	k.SetCW721ERC721Pointer(ctx, evmAddr5, paxAddr5.String())
	k.SetERC1155CW1155Pointer(ctx, paxAddr6.String(), evmAddr6)
	k.SetCW1155ERC1155Pointer(ctx, evmAddr7, paxAddr7.String())
	q := keeper.Querier{k}
	res, err := q.Pointer(goCtx, &types.QueryPointerRequest{PointerType: types.PointerType_NATIVE, Pointee: paxAddr1.String()})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointerResponse{Pointer: evmAddr1.Hex(), Version: uint32(native.CurrentVersion), Exists: true}, *res)
	res, err = q.Pointer(goCtx, &types.QueryPointerRequest{PointerType: types.PointerType_CW20, Pointee: paxAddr2.String()})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointerResponse{Pointer: evmAddr2.Hex(), Version: uint32(cw20.CurrentVersion(ctx)), Exists: true}, *res)
	res, err = q.Pointer(goCtx, &types.QueryPointerRequest{PointerType: types.PointerType_CW721, Pointee: paxAddr3.String()})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointerResponse{Pointer: evmAddr3.Hex(), Version: uint32(cw721.CurrentVersion), Exists: true}, *res)
	res, err = q.Pointer(goCtx, &types.QueryPointerRequest{PointerType: types.PointerType_ERC20, Pointee: evmAddr4.Hex()})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointerResponse{Pointer: paxAddr4.String(), Version: uint32(erc20.CurrentVersion), Exists: true}, *res)
	res, err = q.Pointer(goCtx, &types.QueryPointerRequest{PointerType: types.PointerType_ERC721, Pointee: evmAddr5.Hex()})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointerResponse{Pointer: paxAddr5.String(), Version: uint32(erc721.CurrentVersion), Exists: true}, *res)
	res, err = q.Pointer(goCtx, &types.QueryPointerRequest{PointerType: types.PointerType_CW1155, Pointee: paxAddr6.String()})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointerResponse{Pointer: evmAddr6.Hex(), Version: uint32(cw1155.CurrentVersion), Exists: true}, *res)
	res, err = q.Pointer(goCtx, &types.QueryPointerRequest{PointerType: types.PointerType_ERC1155, Pointee: evmAddr7.Hex()})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointerResponse{Pointer: paxAddr7.String(), Version: uint32(erc1155.CurrentVersion), Exists: true}, *res)
	_, err = q.Pointer(goCtx, &types.QueryPointerRequest{PointerType: types.PointerType_NATIVE})
	require.NotNil(t, err)
	res, err = q.Pointer(goCtx, &types.QueryPointerRequest{PointerType: types.PointerType_NATIVE, Pointee: evmAddr8.Hex()})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointerResponse{Exists: false}, *res)
	res, err = q.Pointer(goCtx, &types.QueryPointerRequest{PointerType: types.PointerType_CW20, Pointee: evmAddr8.Hex()})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointerResponse{Exists: false}, *res)
	res, err = q.Pointer(goCtx, &types.QueryPointerRequest{PointerType: types.PointerType_CW721, Pointee: evmAddr8.Hex()})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointerResponse{Exists: false}, *res)
	res, err = q.Pointer(goCtx, &types.QueryPointerRequest{PointerType: types.PointerType_ERC20, Pointee: evmAddr8.Hex()})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointerResponse{Exists: false}, *res)
	res, err = q.Pointer(goCtx, &types.QueryPointerRequest{PointerType: types.PointerType_ERC721, Pointee: evmAddr8.Hex()})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointerResponse{Exists: false}, *res)
	res, err = q.Pointer(goCtx, &types.QueryPointerRequest{PointerType: types.PointerType_CW1155, Pointee: evmAddr8.Hex()})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointerResponse{Exists: false}, *res)
	res, err = q.Pointer(goCtx, &types.QueryPointerRequest{PointerType: types.PointerType_ERC1155, Pointee: evmAddr8.Hex()})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointerResponse{Exists: false}, *res)
}

func TestQueryPointee(t *testing.T) {
	k, ctx := testkeeper.MockEVMKeeper(t)
	_, pointerAddr1 := testkeeper.MockAddressPair()
	paxAddr2, evmAddr2 := testkeeper.MockAddressPair()
	paxAddr3, evmAddr3 := testkeeper.MockAddressPair()
	paxAddr4, evmAddr4 := testkeeper.MockAddressPair()
	paxAddr5, evmAddr5 := testkeeper.MockAddressPair()
	paxAddr6, evmAddr6 := testkeeper.MockAddressPair()
	paxAddr7, evmAddr7 := testkeeper.MockAddressPair()
	goCtx := sdk.WrapSDKContext(ctx)

	// Set up pointers for each type
	k.SetERC20NativePointer(ctx, "ufoo", pointerAddr1)
	k.SetERC20CW20Pointer(ctx, paxAddr2.String(), evmAddr2)
	k.SetERC721CW721Pointer(ctx, paxAddr3.String(), evmAddr3)
	k.SetCW20ERC20Pointer(ctx, evmAddr4, paxAddr4.String())
	k.SetCW721ERC721Pointer(ctx, evmAddr5, paxAddr5.String())
	k.SetERC1155CW1155Pointer(ctx, paxAddr6.String(), evmAddr6)
	k.SetCW1155ERC1155Pointer(ctx, evmAddr7, paxAddr7.String())

	q := keeper.Querier{k}

	// Test for Native Pointee
	res, err := q.Pointee(goCtx, &types.QueryPointeeRequest{PointerType: types.PointerType_NATIVE, Pointer: pointerAddr1.Hex()})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointeeResponse{Pointee: "ufoo", Version: uint32(native.CurrentVersion), Exists: true}, *res)

	// Test for CW20 Pointee
	res, err = q.Pointee(goCtx, &types.QueryPointeeRequest{PointerType: types.PointerType_CW20, Pointer: evmAddr2.Hex()})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointeeResponse{Pointee: paxAddr2.String(), Version: uint32(cw20.CurrentVersion(ctx)), Exists: true}, *res)

	// Test for CW721 Pointee
	res, err = q.Pointee(goCtx, &types.QueryPointeeRequest{PointerType: types.PointerType_CW721, Pointer: evmAddr3.Hex()})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointeeResponse{Pointee: paxAddr3.String(), Version: uint32(cw721.CurrentVersion), Exists: true}, *res)

	// Test for CW1155 Pointee
	res, err = q.Pointee(goCtx, &types.QueryPointeeRequest{PointerType: types.PointerType_CW1155, Pointer: evmAddr6.Hex()})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointeeResponse{Pointee: paxAddr6.String(), Version: uint32(cw1155.CurrentVersion), Exists: true}, *res)

	// Test for ERC20 Pointee
	res, err = q.Pointee(goCtx, &types.QueryPointeeRequest{PointerType: types.PointerType_ERC20, Pointer: paxAddr4.String()})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointeeResponse{Pointee: evmAddr4.Hex(), Version: uint32(erc20.CurrentVersion), Exists: true}, *res)

	// Test for ERC721 Pointee
	res, err = q.Pointee(goCtx, &types.QueryPointeeRequest{PointerType: types.PointerType_ERC721, Pointer: paxAddr5.String()})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointeeResponse{Pointee: evmAddr5.Hex(), Version: uint32(erc721.CurrentVersion), Exists: true}, *res)

	// Test for ERC1155 Pointee
	res, err = q.Pointee(goCtx, &types.QueryPointeeRequest{PointerType: types.PointerType_ERC1155, Pointer: paxAddr7.String()})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointeeResponse{Pointee: evmAddr7.Hex(), Version: uint32(erc1155.CurrentVersion), Exists: true}, *res)

	// Test for not registered Native Pointee
	res, err = q.Pointee(goCtx, &types.QueryPointeeRequest{PointerType: types.PointerType_NATIVE, Pointer: "0x1234567890123456789012345678901234567890"})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointeeResponse{Pointee: "", Version: 0, Exists: false}, *res)

	// Test for not registered CW20 Pointee
	res, err = q.Pointee(goCtx, &types.QueryPointeeRequest{PointerType: types.PointerType_CW20, Pointer: "0x1234567890123456789012345678901234567890"})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointeeResponse{Pointee: "", Version: 0, Exists: false}, *res)

	// Test for not registered CW721 Pointee
	res, err = q.Pointee(goCtx, &types.QueryPointeeRequest{PointerType: types.PointerType_CW721, Pointer: "0x1234567890123456789012345678901234567890"})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointeeResponse{Pointee: "", Version: 0, Exists: false}, *res)

	// Test for not registered CW1155 Pointee
	res, err = q.Pointee(goCtx, &types.QueryPointeeRequest{PointerType: types.PointerType_CW1155, Pointer: "0x1234567890123456789012345678901234567890"})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointeeResponse{Pointee: "", Version: 0, Exists: false}, *res)

	// Test for not registered ERC20 Pointee
	res, err = q.Pointee(goCtx, &types.QueryPointeeRequest{PointerType: types.PointerType_ERC20, Pointer: "pax1notregistered"})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointeeResponse{Pointee: "", Version: 0, Exists: false}, *res)

	// Test for not registered ERC721 Pointee
	res, err = q.Pointee(goCtx, &types.QueryPointeeRequest{PointerType: types.PointerType_ERC721, Pointer: "pax1notregistered"})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointeeResponse{Pointee: "", Version: 0, Exists: false}, *res)

	// Test for not registered ERC1155 Pointee
	res, err = q.Pointee(goCtx, &types.QueryPointeeRequest{PointerType: types.PointerType_ERC1155, Pointer: "pax1notregistered"})
	require.Nil(t, err)
	require.Equal(t, types.QueryPointeeResponse{Pointee: "", Version: 0, Exists: false}, *res)

	_, err = q.Pointee(goCtx, &types.QueryPointeeRequest{PointerType: types.PointerType_NATIVE})
	require.NotNil(t, err)

	// Test cases for invalid inputs
	testCases := []struct {
		name        string
		req         *types.QueryPointeeRequest
		expectedRes *types.QueryPointeeResponse
		expectedErr error
	}{
		{
			name:        "Invalid pointer type",
			req:         &types.QueryPointeeRequest{PointerType: 999, Pointer: pointerAddr1.Hex()},
			expectedRes: nil,
			expectedErr: errors.ErrUnsupported,
		},
		{
			name:        "Empty pointer",
			req:         &types.QueryPointeeRequest{PointerType: types.PointerType_NATIVE, Pointer: ""},
			expectedRes: nil,
			expectedErr: keeper.ErrMustSpecifyPointer,
		},
		{
			name:        "Invalid hex address for EVM-based pointer types",
			req:         &types.QueryPointeeRequest{PointerType: types.PointerType_CW20, Pointer: "not-a-hex-address"},
			expectedRes: &types.QueryPointeeResponse{Pointee: "", Version: 0, Exists: false},
			expectedErr: nil,
		},
		{
			name:        "Invalid bech32 address for Cosmos-based pointer types",
			req:         &types.QueryPointeeRequest{PointerType: types.PointerType_ERC20, Pointer: "not-a-bech32-address"},
			expectedRes: &types.QueryPointeeResponse{Pointee: "", Version: 0, Exists: false},
			expectedErr: nil,
		},
	}

	for _, tc := range testCases {
		t.Run(tc.name, func(t *testing.T) {
			res, err := q.Pointee(goCtx, tc.req)
			if tc.expectedErr != nil {
				require.ErrorIs(t, err, tc.expectedErr)
				require.Nil(t, res)
			} else {
				require.NoError(t, err)
				require.Equal(t, tc.expectedRes, res)
			}
		})
	}
}

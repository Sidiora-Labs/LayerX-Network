package wasm_test

import (
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/evm/client/wasm"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func TestERCQueriesRejectMalformedAddresses(t *testing.T) {
	keeper := &testkeeper.EVMTestApp.EvmKeeper
	ctx, _ := testkeeper.EVMTestApp.GetContextForDeliverTx(nil).CacheContext()
	address := sdk.AccAddress(append(make([]byte, 19), 42))
	keeper.SetAddressMapping(ctx, address, common.Address{42})
	valid := address.String()
	invalid := "invalid-address"
	handler := wasm.NewEVMQueryHandler(keeper)
	amount := sdk.NewInt(1)
	contract := common.Address{43}.Hex()
	cases := []struct {
		name string
		call func() ([]byte, error)
	}{
		{"erc20 transfer recipient", func() ([]byte, error) { return handler.HandleERC20TransferPayload(ctx, invalid, &amount) }},
		{"erc20 transfer owner", func() ([]byte, error) { return handler.HandleERC20TransferFromPayload(ctx, invalid, valid, &amount) }},
		{"erc20 transfer from recipient", func() ([]byte, error) { return handler.HandleERC20TransferFromPayload(ctx, valid, invalid, &amount) }},
		{"erc20 approve spender", func() ([]byte, error) { return handler.HandleERC20ApprovePayload(ctx, invalid, &amount) }},
		{"erc20 allowance spender", func() ([]byte, error) { return handler.HandleERC20Allowance(ctx, contract, valid, invalid) }},
		{"erc721 transfer sender", func() ([]byte, error) { return handler.HandleERC721TransferPayload(ctx, invalid, valid, "1") }},
		{"erc721 transfer recipient", func() ([]byte, error) { return handler.HandleERC721TransferPayload(ctx, valid, invalid, "1") }},
		{"erc721 approve spender", func() ([]byte, error) { return handler.HandleERC721ApprovePayload(ctx, invalid, "1") }},
		{"erc721 approve all", func() ([]byte, error) { return handler.HandleERC721SetApprovalAllPayload(ctx, invalid, true) }},
		{"erc721 approval owner", func() ([]byte, error) {
			return handler.HandleERC721IsApprovedForAll(ctx, valid, contract, invalid, valid)
		}},
		{"erc721 approval operator", func() ([]byte, error) {
			return handler.HandleERC721IsApprovedForAll(ctx, valid, contract, valid, invalid)
		}},
		{"erc1155 transfer sender", func() ([]byte, error) { return handler.HandleERC1155TransferPayload(ctx, invalid, valid, "1", &amount) }},
		{"erc1155 transfer recipient", func() ([]byte, error) { return handler.HandleERC1155TransferPayload(ctx, valid, invalid, "1", &amount) }},
		{"erc1155 batch sender", func() ([]byte, error) {
			return handler.HandleERC1155BatchTransferPayload(ctx, invalid, valid, []string{"1"}, []*sdk.Int{&amount})
		}},
		{"erc1155 batch recipient", func() ([]byte, error) {
			return handler.HandleERC1155BatchTransferPayload(ctx, valid, invalid, []string{"1"}, []*sdk.Int{&amount})
		}},
		{"erc1155 approve all", func() ([]byte, error) { return handler.HandleERC1155SetApprovalAllPayload(ctx, invalid, true) }},
		{"erc1155 approval owner", func() ([]byte, error) {
			return handler.HandleERC1155IsApprovedForAll(ctx, valid, contract, invalid, valid)
		}},
		{"erc1155 approval operator", func() ([]byte, error) {
			return handler.HandleERC1155IsApprovedForAll(ctx, valid, contract, valid, invalid)
		}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			result, err := tc.call()
			require.Error(t, err)
			require.Nil(t, result)
		})
	}
	result, err := handler.HandleERC20TransferPayload(ctx, valid, &amount)
	require.NoError(t, err)
	require.NotEmpty(t, result)
	result, err = handler.HandleERC721TransferPayload(ctx, valid, valid, "1")
	require.NoError(t, err)
	require.NotEmpty(t, result)
	result, err = handler.HandleERC1155BatchTransferPayload(ctx, valid, valid, []string{"1"}, []*sdk.Int{&amount})
	require.NoError(t, err)
	require.NotEmpty(t, result)
}

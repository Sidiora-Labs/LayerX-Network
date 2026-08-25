package antedecorators_test

import (
	"testing"

	tmproto "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	evmtypes "github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/node/antedecorators"
	"github.com/sidiora-labs/paxeer-network/sdk/crypto/keys/secp256k1"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/authz"
	banktypes "github.com/sidiora-labs/paxeer-network/sdk/x/bank/types"
	"github.com/stretchr/testify/require"
)

func TestAuthzNestedEvmMessage(t *testing.T) {
	priv1 := secp256k1.GenPrivKey()
	addr1 := sdk.AccAddress(priv1.PubKey().Address())
	output = ""
	anteDecorators := []sdk.AnteDecorator{
		antedecorators.NewAuthzNestedMessageDecorator(),
	}
	ctx := sdk.NewContext(nil, tmproto.Header{}, false)
	chainedHandler := sdk.ChainAnteDecorators(anteDecorators...)

	nestedEvmMessage := authz.NewMsgExec(addr1, []sdk.Msg{&evmtypes.MsgEVMTransaction{}})
	// test with nested evm message
	_, err := chainedHandler(
		ctx.WithPriority(0),
		FakeTx{
			FakeMsgs: []sdk.Msg{&nestedEvmMessage},
		},
		false,
	)
	require.NotNil(t, err)

	// Multiple nested layers to evm message
	doubleNestedEvmMessage := authz.NewMsgExec(addr1, []sdk.Msg{&nestedEvmMessage})
	_, err = chainedHandler(
		ctx.WithPriority(0),
		FakeTx{
			FakeMsgs: []sdk.Msg{&doubleNestedEvmMessage},
		},
		false,
	)
	require.NotNil(t, err)

	// No error
	nestedMessage := authz.NewMsgExec(addr1, []sdk.Msg{&banktypes.MsgSend{}})
	_, err = chainedHandler(
		ctx.WithPriority(0),
		FakeTx{
			FakeMsgs: []sdk.Msg{&nestedMessage},
		},
		false,
	)
	require.Nil(t, err)
}

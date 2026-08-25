package types

import (
	"testing"

	"github.com/stretchr/testify/require"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func TestMsgUnjailGetSignBytes(t *testing.T) {
	addr := sdk.AccAddress("abcd")
	msg := NewMsgUnjail(sdk.ValAddress(addr))
	bytes := msg.GetSignBytes()
	require.Equal(
		t,
		`{"type":"cosmos-sdk/MsgUnjail","value":{"address":"paxvaloper1v93xxeq2e4z3p"}}`,
		string(bytes),
	)
}

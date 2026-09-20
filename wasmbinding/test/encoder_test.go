package wasmbinding

import (
	"encoding/json"
	"testing"

	tokenfactorywasm "github.com/sidiora-labs/paxeer-network/modules/tokenfactory/client/wasm"
	tokenfactorytypes "github.com/sidiora-labs/paxeer-network/modules/tokenfactory/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/wasmbinding/bindings"
	"github.com/stretchr/testify/require"
)

const (
	TEST_TARGET_CONTRACT = "pax14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9snf99un"
	TEST_CREATOR         = "pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288"
)

func TestEncodeCreateDenom(t *testing.T) {
	contractAddr, err := sdk.AccAddressFromBech32("pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288")
	require.NoError(t, err)
	msg := bindings.CreateDenom{
		Subdenom: "subdenom",
	}
	serializedMsg, _ := json.Marshal(msg)

	decodedMsgs, err := tokenfactorywasm.EncodeTokenFactoryCreateDenom(serializedMsg, contractAddr)
	require.NoError(t, err)
	require.Equal(t, 1, len(decodedMsgs))
	typedDecodedMsg, ok := decodedMsgs[0].(*tokenfactorytypes.MsgCreateDenom)
	require.True(t, ok)
	expectedMsg := tokenfactorytypes.MsgCreateDenom{
		Sender:   "pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288",
		Subdenom: "subdenom",
	}
	require.Equal(t, expectedMsg, *typedDecodedMsg)
}

func TestEncodeMint(t *testing.T) {
	contractAddr, err := sdk.AccAddressFromBech32("pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288")
	require.NoError(t, err)
	msg := bindings.MintTokens{
		Amount: sdk.Coin{Amount: sdk.NewInt(100), Denom: "subdenom"},
	}
	serializedMsg, _ := json.Marshal(msg)

	decodedMsgs, err := tokenfactorywasm.EncodeTokenFactoryMint(serializedMsg, contractAddr)
	require.NoError(t, err)
	require.Equal(t, 1, len(decodedMsgs))
	typedDecodedMsg, ok := decodedMsgs[0].(*tokenfactorytypes.MsgMint)
	require.True(t, ok)
	expectedMsg := tokenfactorytypes.MsgMint{
		Sender: "pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288",
		Amount: sdk.Coin{Amount: sdk.NewInt(100), Denom: "subdenom"},
	}
	require.Equal(t, expectedMsg, *typedDecodedMsg)
}

func TestEncodeBurn(t *testing.T) {
	contractAddr, err := sdk.AccAddressFromBech32("pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288")
	require.NoError(t, err)
	msg := bindings.BurnTokens{
		Amount: sdk.Coin{Amount: sdk.NewInt(10), Denom: "subdenom"},
	}
	serializedMsg, _ := json.Marshal(msg)

	decodedMsgs, err := tokenfactorywasm.EncodeTokenFactoryBurn(serializedMsg, contractAddr)
	require.NoError(t, err)
	require.Equal(t, 1, len(decodedMsgs))
	typedDecodedMsg, ok := decodedMsgs[0].(*tokenfactorytypes.MsgBurn)
	require.True(t, ok)
	expectedMsg := tokenfactorytypes.MsgBurn{
		Sender: "pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288",
		Amount: sdk.Coin{Amount: sdk.NewInt(10), Denom: "subdenom"},
	}
	require.Equal(t, expectedMsg, *typedDecodedMsg)
}

func TestEncodeChangeAdmin(t *testing.T) {
	contractAddr, err := sdk.AccAddressFromBech32("pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288")
	require.NoError(t, err)
	msg := bindings.ChangeAdmin{
		Denom:           "factory/pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288/subdenom",
		NewAdminAddress: "pax1hjfwcza3e3uzeznf3qthhakdr9juetl7ee472u",
	}
	serializedMsg, _ := json.Marshal(msg)

	decodedMsgs, err := tokenfactorywasm.EncodeTokenFactoryChangeAdmin(serializedMsg, contractAddr)
	require.NoError(t, err)
	require.Equal(t, 1, len(decodedMsgs))
	typedDecodedMsg, ok := decodedMsgs[0].(*tokenfactorytypes.MsgChangeAdmin)
	require.True(t, ok)
	expectedMsg := tokenfactorytypes.MsgChangeAdmin{
		Sender:   "pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288",
		Denom:    "factory/pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288/subdenom",
		NewAdmin: "pax1hjfwcza3e3uzeznf3qthhakdr9juetl7ee472u",
	}
	require.Equal(t, expectedMsg, *typedDecodedMsg)
}

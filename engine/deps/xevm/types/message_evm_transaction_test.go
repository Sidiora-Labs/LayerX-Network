package types_test

import (
	"encoding/hex"
	"math/big"
	"testing"

	ethtypes "github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
	testkeeper "github.com/sidiora-labs/paxeer-network/engine/deps/testutil/keeper"
	"github.com/sidiora-labs/paxeer-network/node"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	wasmtypes "github.com/sidiora-labs/paxeer-network/wasm/x/wasm/types"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/engine/deps/xevm/types"
	"github.com/sidiora-labs/paxeer-network/engine/deps/xevm/types/ethtx"
	"github.com/stretchr/testify/require"
)

func TestIsAssociate(t *testing.T) {
	tx, err := types.NewMsgEVMTransaction(&ethtx.AssociateTx{})
	require.Nil(t, err)
	require.True(t, tx.IsAssociateTx())
}

func TestAssociateEnvelopeIsNotEthereumTransaction(t *testing.T) {
	associate := &ethtx.AssociateTx{V: []byte{1}, R: []byte{2}, S: []byte{3}, CustomMessage: "associate"}
	msg, err := types.NewMsgEVMTransaction(associate)
	require.NoError(t, err)

	ethTx, txData := msg.AsTransaction()
	require.Nil(t, ethTx)
	require.Equal(t, associate, txData)

	copyTx := associate.Copy().(*ethtx.AssociateTx)
	copyTx.V[0] = 9
	require.Equal(t, byte(1), associate.V[0])
}

func TestMalformedEnvelopeAssociationCheckFailsClosed(t *testing.T) {
	msg := &types.MsgEVMTransaction{}
	require.False(t, msg.IsAssociateTx())
	associate, ok := msg.GetAssociateTx()
	require.False(t, ok)
	require.Nil(t, associate)
}

func TestIsNotAssociate(t *testing.T) {
	tx, err := types.NewMsgEVMTransaction(nil)
	require.Error(t, err)

	tx, err = types.NewMsgEVMTransaction(&ethtx.AccessTuple{})
	require.Nil(t, err)
	require.False(t, tx.IsAssociateTx())
}

func TestAsTransaction(t *testing.T) {
	k, ctx := testkeeper.MockEVMKeeper(t)
	chainID := k.ChainID(ctx)
	chainCfg := types.DefaultChainConfig()
	ethCfg := chainCfg.EthereumConfig(chainID)
	blockNum := big.NewInt(ctx.BlockHeight())
	privKey := testkeeper.MockPrivateKey()
	testPrivHex := hex.EncodeToString(privKey.Bytes())
	key, _ := crypto.HexToECDSA(testPrivHex)
	to := new(common.Address)
	txData := ethtypes.DynamicFeeTx{
		Nonce:     1,
		GasFeeCap: big.NewInt(10000000000000),
		Gas:       1000,
		To:        to,
		Value:     big.NewInt(1000000000000000),
		Data:      []byte("abc"),
		ChainID:   chainID,
	}

	signer := ethtypes.MakeSigner(ethCfg, blockNum, uint64(ctx.BlockTime().Unix()))
	tx, err := ethtypes.SignTx(ethtypes.NewTx(&txData), signer, key)
	typedTx, err := ethtx.NewDynamicFeeTx(tx)
	msg, err := types.NewMsgEVMTransaction(typedTx)
	require.Nil(t, err)
	ethTx, ethTxData := msg.AsTransaction()
	require.Equal(t, chainID, ethTx.ChainId())
	require.Equal(t, uint64(1), ethTx.Nonce())
	require.Equal(t, []byte("abc"), ethTx.Data())
	require.Nil(t, ethTxData.Validate())

}

func TestMustGetEVMTransactionMessage(t *testing.T) {
	testMsg := types.MsgEVMTransaction{
		Data:    nil,
		Derived: nil,
	}
	testTx := app.NewTestTx([]sdk.Msg{&testMsg})

	types.MustGetEVMTransactionMessage(testTx)
}

func TestMustGetEVMTransactionMessageWrongType(t *testing.T) {

	// Non-EVM tx
	testMsg := wasmtypes.MsgExecuteContract{
		Contract: "pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288",
		Msg:      []byte("{\"xyz\":{}}"),
	}
	testTx := app.NewTestTx([]sdk.Msg{&testMsg})

	defer func() { recover() }()
	types.MustGetEVMTransactionMessage(testTx)
	t.Errorf("Should not be able to convert a non evm emssage")
}

func TestMustGetEVMTransactionMessageMultipleMsgs(t *testing.T) {
	testMsg := types.MsgEVMTransaction{
		Data:    nil,
		Derived: nil,
	}
	testTx := app.NewTestTx([]sdk.Msg{&testMsg, &testMsg})

	defer func() { recover() }()
	types.MustGetEVMTransactionMessage(testTx)
	t.Errorf("Should not be able to convert a non evm emssage")
}

package tests

import (
	"crypto/sha256"
	"encoding/hex"
	"strings"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/common/bitutil"
	ethtypes "github.com/ethereum/go-ethereum/core/types"
	"github.com/sidiora-labs/paxeer-network/node"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/stretchr/testify/require"
)

func TestGetBlockByHash(t *testing.T) {
	txBz := signAndEncodeTx(send(0), mnemonic1)
	SetupTestServer(t, [][][]byte{{txBz}}, mnemonicInitializer(mnemonic1)).Run(
		func(port int) {
			res := sendRequestWithNamespace("eth", port, "getBlockByHash", common.HexToHash("0x6f2168eb453152b1f68874fe32cea6fcb199bfd63836acb72a8eb33e666613fe").Hex(), true)
			blockHash := res["result"].(map[string]interface{})["hash"]
			require.Equal(t, "0x6f2168eb453152b1f68874fe32cea6fcb199bfd63836acb72a8eb33e666613fe", blockHash.(string))
		},
	)
}

func TestGetPaxBlockByHash(t *testing.T) {
	cw20 := "pax18cszlvm6pze0x9sz32qnjq4vtd45xehqs8dq7cwy8yhq35wfnn3qcm2ty5" // hardcoded
	tx1 := signAndEncodeTx(registerCW20Pointer(0, cw20), mnemonic1)
	tx2 := signAndEncodeCosmosTx(transferCW20Msg(mnemonic1, cw20), mnemonic1, 7, 0)
	SetupTestServer(t, [][][]byte{{tx1}, {tx2}}, mnemonicInitializer(mnemonic1), cw20Initializer(mnemonic1, true)).Run(
		func(port int) {
			res := sendRequestWithNamespace("pax", port, "getBlockByHash", common.HexToHash("0x9dd3e6c427b6936f973b240cd5780b8ee4bf8fab0c8d281afb28089db51bb4af").Hex(), true)
			txs := res["result"].(map[string]interface{})["transactions"]
			require.Len(t, txs.([]interface{}), 1)
		},
	)
}

func TestGetBlockByNumber(t *testing.T) {
	txBz1 := signAndEncodeTx(send(0), mnemonic1)
	txBz2 := signAndEncodeTx(send(1), mnemonic1)
	txBz3 := signAndEncodeTx(send(2), mnemonic1)
	SetupTestServer(t, [][][]byte{{txBz1}, {txBz2}, {txBz3}}, mnemonicInitializer(mnemonic1)).Run(
		func(port int) {
			res := sendRequestWithNamespace("eth", port, "getBlockByNumber", "earliest", true)
			earliestBlock := res["result"].(map[string]interface{})
			blockHash := earliestBlock["hash"]
			require.NotEqual(t, common.Hash{}.Hex(), blockHash.(string))
			require.NotEqual(t, common.Hash{}.Hex(), earliestBlock["stateRoot"].(string))
			require.NotEqual(t, common.Hash{}.Hex(), earliestBlock["transactionsRoot"].(string))
			require.NotEqual(t, common.Hash{}.Hex(), earliestBlock["receiptsRoot"].(string))
			res = sendRequestWithNamespace("eth", port, "getBlockByNumber", "safe", true)
			blockHash = res["result"].(map[string]interface{})["hash"]
			require.Equal(t, "0x8ace0b4e9ced0ef792034128d37eb19b9b2b06bf016d51d533216a9afd7c0e8f", blockHash.(string))
			res = sendRequestWithNamespace("eth", port, "getBlockByNumber", "latest", true)
			blockHash = res["result"].(map[string]interface{})["hash"]
			require.Equal(t, "0x8ace0b4e9ced0ef792034128d37eb19b9b2b06bf016d51d533216a9afd7c0e8f", blockHash.(string))
			res = sendRequestWithNamespace("eth", port, "getBlockByNumber", "finalized", true)
			blockHash = res["result"].(map[string]interface{})["hash"]
			require.Equal(t, "0x8ace0b4e9ced0ef792034128d37eb19b9b2b06bf016d51d533216a9afd7c0e8f", blockHash.(string))
			res = sendRequestWithNamespace("eth", port, "getBlockByNumber", "pending", true)
			blockHash = res["result"].(map[string]interface{})["hash"]
			require.Equal(t, "0x8ace0b4e9ced0ef792034128d37eb19b9b2b06bf016d51d533216a9afd7c0e8f", blockHash.(string))
			res = sendRequestWithNamespace("eth", port, "getBlockByNumber", "0x2", true)
			blockHash = res["result"].(map[string]interface{})["hash"]
			require.Equal(t, "0x6f2168eb453152b1f68874fe32cea6fcb199bfd63836acb72a8eb33e666613fe", blockHash.(string))
		},
	)
}

func TestGetBlockSkipTxIndex(t *testing.T) {
	tx1 := signAndEncodeCosmosTx(bankSendMsg(mnemonic1), mnemonic1, 7, 0)
	tx2 := signAndEncodeTx(send(0), mnemonic1)
	SetupTestServer(t, [][][]byte{{tx1, tx2}}, mnemonicInitializer(mnemonic1)).Run(
		func(port int) {
			res := sendRequestWithNamespace("eth", port, "getBlockByHash", common.HexToHash("0x6f2168eb453152b1f68874fe32cea6fcb199bfd63836acb72a8eb33e666613fe").Hex(), true)
			txs := res["result"].(map[string]any)["transactions"].([]any)
			require.Len(t, txs, 1)
			require.Equal(t, "0x0", txs[0].(map[string]any)["transactionIndex"].(string))
		},
	)
}

func TestAnteFailureNonce(t *testing.T) {
	txBz := signAndEncodeTx(send(1), mnemonic1) // wrong nonce
	// incorrect nonce should always be excluded
	SetupTestServer(t, [][][]byte{{txBz}}, mnemonicInitializer(mnemonic1), mockUpgrade("v5.8.0", 0)).Run(
		func(port int) {
			res := sendRequestWithNamespace("eth", port, "getBlockByHash", common.HexToHash("0x6f2168eb453152b1f68874fe32cea6fcb199bfd63836acb72a8eb33e666613fe").Hex(), true)
			txs := res["result"].(map[string]interface{})["transactions"].([]interface{})
			require.Len(t, txs, 0)
		},
	)
	SetupTestServer(t, [][][]byte{{txBz}}, mnemonicInitializer(mnemonic1), mockUpgrade("v5.5.5", 0)).Run(
		func(port int) {
			res := sendRequestWithNamespace("eth", port, "getBlockByHash", common.HexToHash("0x6f2168eb453152b1f68874fe32cea6fcb199bfd63836acb72a8eb33e666613fe").Hex(), true)
			txs := res["result"].(map[string]interface{})["transactions"].([]interface{})
			require.Len(t, txs, 0)
		},
	)
}

func TestAnteFailureOthers(t *testing.T) {
	txBz := signAndEncodeTx(send(0), mnemonic1)
	// insufficient fund should be included post v5.8.0
	SetupTestServer(t, [][][]byte{{txBz}}, mockUpgrade("v5.8.0", 0)).Run(
		func(port int) {
			res := sendRequestWithNamespace("eth", port, "getBlockByHash", common.HexToHash("0x6f2168eb453152b1f68874fe32cea6fcb199bfd63836acb72a8eb33e666613fe").Hex(), true)
			txs := res["result"].(map[string]interface{})["transactions"].([]interface{})
			require.Len(t, txs, 1)
		},
	)
	// insufficient fund should not be included pre v5.8.0
	SetupTestServer(t, [][][]byte{{txBz}}, mockUpgrade("v5.5.5", 0)).Run(
		func(port int) {
			res := sendRequestWithNamespace("eth", port, "getBlockByHash", common.HexToHash("0x6f2168eb453152b1f68874fe32cea6fcb199bfd63836acb72a8eb33e666613fe").Hex(), true)
			txs := res["result"].(map[string]interface{})["transactions"].([]interface{})
			require.Len(t, txs, 0)
		},
	)
}

// Mirrors TestAnteFailureOthers but inverts the expectation: where the
// regular eth_getBlockByHash includes insufficient-funds stub receipts
// post-v5.8.0 (per PR #2343), the *ExcludeTraceFail variant must drop
// them — the tx bumped its nonce in ante but never reached the VM.
func TestGetBlockByHashExcludeTraceFail_AnteStub(t *testing.T) {
	stubTxBz := signAndEncodeTx(sendInsufficientFunds(0), mnemonic1)
	SetupTestServer(t, [][][]byte{{stubTxBz}}, mnemonicInitializer(mnemonic1)).Run(
		func(port int) {
			// Confirm the stub flows through the regular endpoint (PR #2343 baseline).
			res := sendRequestWithNamespace("eth", port, "getBlockByHash", common.HexToHash("0x6f2168eb453152b1f68874fe32cea6fcb199bfd63836acb72a8eb33e666613fe").Hex(), true)
			txs := res["result"].(map[string]interface{})["transactions"].([]interface{})
			require.Len(t, txs, 1, "regular eth_getBlockByHash should keep the insufficient-funds tx, got %v", txs)

			// The *ExcludeTraceFail variant must drop it.
			res = sendRequestWithNamespace("pax", port, "getBlockByHashExcludeTraceFail", common.HexToHash("0x6f2168eb453152b1f68874fe32cea6fcb199bfd63836acb72a8eb33e666613fe").Hex(), true)
			txs = res["result"].(map[string]interface{})["transactions"].([]interface{})
			require.Len(t, txs, 0, "pax_getBlockByHashExcludeTraceFail should drop the insufficient-funds stub, got %v", txs)
		},
	)
}

func TestGetBlockReceipts(t *testing.T) {
	txBz1 := signAndEncodeTx(send(0), mnemonic1)
	txBz2 := signAndEncodeTx(send(1), mnemonic1)
	SetupTestServer(t, [][][]byte{{txBz1, txBz2}}, mnemonicInitializer(mnemonic1)).Run(
		func(port int) {
			res := sendRequestWithNamespace("eth", port, "getBlockReceipts", common.HexToHash("0x6f2168eb453152b1f68874fe32cea6fcb199bfd63836acb72a8eb33e666613fe").Hex())
			require.Len(t, res["result"], 2)
		},
	)
}

func TestGetBlockAfterBaseFeeChange(t *testing.T) {
	mockBaseFee := func(baseFee int64) func(ctx sdk.Context, a *app.App) {
		return func(ctx sdk.Context, a *app.App) {
			a.EvmKeeper.SetCurrBaseFeePerGas(ctx, sdk.NewDec(baseFee))
		}
	}
	unsigned := send(1)
	tx := signTxWithMnemonic(unsigned, mnemonic1)
	txBz := encodeEvmTx(unsigned, tx)
	ts := SetupTestServer(t, [][][]byte{{txBz}}, mnemonicInitializer(mnemonic1), mockBaseFee(1000000001))
	ts.Run(func(port int) {
		res := sendRequestWithNamespace("eth", port, "getBlockByNumber", "0x2", true)
		txs := res["result"].(map[string]interface{})["transactions"].([]interface{})
		require.Len(t, txs, 0) // should be excluded since nonce didn't increment

		ts.SetupBlocks([][][]byte{{signAndEncodeTx(send(0), mnemonic1)}, {txBz}}, mnemonicInitializer(mnemonic1), mockBaseFee(1000000000))

		res = sendRequestWithNamespace("eth", port, "getBlockByNumber", "0x4", true)
		txs = res["result"].(map[string]interface{})["transactions"].([]interface{})
		require.Len(t, txs, 1)

		res = sendRequestWithNamespace("eth", port, "getVMError", tx.Hash().Hex())
		require.Equal(t, "", res["result"].(string))
	})
}

func TestBlockBloom(t *testing.T) {
	txdata1 := depositErc20(1)
	txdata2 := sendErc20(2)
	signedTx1 := signTxWithMnemonic(txdata1, erc20DeployerMnemonics)
	signedTx2 := signTxWithMnemonic(txdata2, erc20DeployerMnemonics)
	tx1 := encodeEvmTx(txdata1, signedTx1)
	tx2 := encodeEvmTx(txdata2, signedTx2)
	cw20 := "pax18cszlvm6pze0x9sz32qnjq4vtd45xehqs8dq7cwy8yhq35wfnn3qcm2ty5" // hardcoded
	tx3 := signAndEncodeCosmosTx(transferCW20Msg(mnemonic1, cw20), mnemonic1, 9, 0)
	SetupTestServer(t, [][][]byte{{tx1, tx2, tx3}}, erc20Initializer(), mnemonicInitializer(mnemonic1), cw20Initializer(mnemonic1, true)).Run(
		func(port int) {
			res := sendRequestWithNamespace("eth", port, "getBlockByNumber", "0x2", false)
			blockBloomString := res["result"].(map[string]interface{})["logsBloom"]
			blockBloomBz, _ := hex.DecodeString(strings.TrimPrefix(blockBloomString.(string), "0x"))
			blockBloom := ethtypes.Bloom{}
			blockBloom.SetBytes(blockBloomBz)

			receipt1 := sendRequestWithNamespace("eth", port, "getTransactionReceipt", signedTx1.Hash().Hex())
			tx1BloomString := receipt1["result"].(map[string]interface{})["logsBloom"]
			tx1BloomBz, _ := hex.DecodeString(strings.TrimPrefix(tx1BloomString.(string), "0x"))
			tx1Bloom := ethtypes.Bloom{}
			tx1Bloom.SetBytes(tx1BloomBz)

			receipt2 := sendRequestWithNamespace("eth", port, "getTransactionReceipt", signedTx2.Hash().Hex())
			tx2BloomString := receipt2["result"].(map[string]interface{})["logsBloom"]
			tx2BloomBz, _ := hex.DecodeString(strings.TrimPrefix(tx2BloomString.(string), "0x"))
			tx2Bloom := ethtypes.Bloom{}
			tx2Bloom.SetBytes(tx2BloomBz)

			expected := make([]byte, ethtypes.BloomByteLength)
			bitutil.ORBytes(expected, tx1Bloom[:], tx2Bloom[:])
			require.Equal(t, expected, blockBloom[:])

			res = sendRequestWithNamespace("pax", port, "getBlockByNumber", "0x2", false)
			blockBloomString = res["result"].(map[string]interface{})["logsBloom"]
			blockBloomBz, _ = hex.DecodeString(strings.TrimPrefix(blockBloomString.(string), "0x"))
			blockBloom = ethtypes.Bloom{}
			blockBloom.SetBytes(blockBloomBz)

			tx3Sum := sha256.Sum256(tx3)
			tx3Hash := common.BytesToHash(tx3Sum[:])
			receipt3 := sendRequestWithNamespace("pax", port, "getTransactionReceipt", tx3Hash.Hex())
			tx3BloomString := receipt3["result"].(map[string]interface{})["logsBloom"]
			tx3BloomBz, _ := hex.DecodeString(strings.TrimPrefix(tx3BloomString.(string), "0x"))
			tx3Bloom := ethtypes.Bloom{}
			tx3Bloom.SetBytes(tx3BloomBz)

			bitutil.ORBytes(expected, expected, tx3Bloom[:])
			require.Equal(t, expected, blockBloom[:])
		},
	)
}

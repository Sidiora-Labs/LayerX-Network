package evmrpc_test

import (
	"fmt"
	"log"
	"math/big"
	"testing"

	"github.com/ethereum/go-ethereum/crypto"
	"github.com/stretchr/testify/require"
)

func TestAssocation(t *testing.T) {
	privateKey, err := crypto.GenerateKey()
	if err != nil {
		log.Fatalf("Failed to generate private key: %v", err)
	}

	// Sign empty payload prepended with Ethereum Signed Message
	customMessageHash := crypto.Keccak256Hash([]byte("\x19Ethereum Signed Message:\n0"))
	signature, err := crypto.Sign(customMessageHash[:], privateKey)
	if err != nil {
		log.Fatalf("Failed to sign payload: %v", err)
	}

	txArgs := map[string]interface{}{
		"r":              fmt.Sprintf("0x%v", new(big.Int).SetBytes(signature[:32]).Text(16)),
		"s":              fmt.Sprintf("0x%v", new(big.Int).SetBytes(signature[32:64]).Text(16)),
		"v":              fmt.Sprintf("0x%v", new(big.Int).SetBytes([]byte{signature[64]}).Text(16)),
		"custom_message": "\x19Ethereum Signed Message:\n0",
	}

	body := sendRequestGoodWithNamespace(t, "pax", "associate", txArgs)
	require.Equal(t, nil, body["result"])
}

func TestGetPaxAddress(t *testing.T) {
	body := sendRequestGoodWithNamespace(t, "pax", "getPaxAddress", "0x1df809C639027b465B931BD63Ce71c8E5834D9d6")
	require.Equal(t, "pax1mf0llhmqane5w2y8uynmghmk2w4mh0xlwxuh3j", body["result"])
}

func TestGetEvmAddress(t *testing.T) {
	body := sendRequestGoodWithNamespace(t, "pax", "getEVMAddress", "pax1mf0llhmqane5w2y8uynmghmk2w4mh0xlwxuh3j")
	require.Equal(t, "0x1df809C639027b465B931BD63Ce71c8E5834D9d6", body["result"])
}

func TestGetCosmosTx(t *testing.T) {
	body := sendRequestGoodWithNamespace(t, "pax", "getCosmosTx", "0xa16d8f7ea8741acd23f15fc19b0dd26512aff68c01c6260d7c3a51b297399d32")
	fmt.Println(body)
	require.Equal(t, "8157F5367C049F7E6F5B46FA00D01FE6A22D8E74E038E1FE61A63C9718A8DC0A", body["result"])
}

func TestGetEvmTx(t *testing.T) {
	body := sendRequestGoodWithNamespace(t, "pax", "getEvmTx", "8157F5367C049F7E6F5B46FA00D01FE6A22D8E74E038E1FE61A63C9718A8DC0A")
	fmt.Println(body)
	require.Equal(t, "0xf02362077ac075a397344172496b28e913ce5294879d811bb0269b3be20a872e", body["result"])
}

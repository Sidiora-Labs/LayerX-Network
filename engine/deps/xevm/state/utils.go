package state

import (
	"encoding/binary"
	"math/big"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// UhpxToSweiMultiplier Fields that were denominated in uhpx will be converted to swei (1uhpx = 10^12swei)
// for existing Ethereum application (which assumes 18 decimal points) to display properly.
var UhpxToSweiMultiplier = big.NewInt(1_000_000_000_000)
var SdkUhpxToSweiMultiplier = sdk.NewIntFromBigInt(UhpxToSweiMultiplier)

var CoinbaseAddressPrefix = []byte("evm_coinbase")

func GetCoinbaseAddress(txIdx int) sdk.AccAddress {
	txIndexBz := make([]byte, 8)
	binary.BigEndian.PutUint64(txIndexBz, uint64(txIdx)) //nolint:gosec
	return append(CoinbaseAddressPrefix, txIndexBz...)
}

func SplitUhpxWeiAmount(amt *big.Int) (sdk.Int, sdk.Int) {
	wei := new(big.Int).Mod(amt, UhpxToSweiMultiplier)
	uhpx := new(big.Int).Quo(amt, UhpxToSweiMultiplier)
	return sdk.NewIntFromBigInt(uhpx), sdk.NewIntFromBigInt(wei)
}

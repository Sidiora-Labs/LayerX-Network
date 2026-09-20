package wpax

import (
	"embed"
	"encoding/hex"
	"strings"

	"github.com/ethereum/go-ethereum/accounts/abi"
)

const CurrentVersion uint16 = 1

//go:embed WPAX.abi
//go:embed WPAX.bin
var f embed.FS

var cachedBin []byte
var cachedABI *abi.ABI

func GetABI() []byte {
	bz, err := f.ReadFile("WPAX.abi")
	if err != nil {
		panic("failed to read WPAX contract ABI")
	}
	return bz
}

func GetParsedABI() *abi.ABI {
	if cachedABI != nil {
		return cachedABI
	}
	parsedABI, err := abi.JSON(strings.NewReader(string(GetABI())))
	if err != nil {
		panic(err)
	}
	cachedABI = &parsedABI
	return cachedABI
}

func GetBin() []byte {
	if cachedBin != nil {
		return cachedBin
	}
	code, err := f.ReadFile("WPAX.bin")
	if err != nil {
		panic("failed to read WPAX contract binary")
	}
	bz, err := hex.DecodeString(string(code))
	if err != nil {
		panic("failed to decode WPAX contract binary")
	}
	cachedBin = bz
	return bz
}

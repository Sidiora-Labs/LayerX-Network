//go:build cgo

package cosmwasm

import (
	"github.com/sidiora-labs/paxeer-network/wasm-runtime/internal/api"
)

func libwasmvmVersionImpl() (string, error) {
	return api.LibwasmvmVersion()
}

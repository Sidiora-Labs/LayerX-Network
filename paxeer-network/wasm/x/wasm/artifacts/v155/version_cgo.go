//go:build cgo

package v155

import (
	"github.com/sidiora-labs/paxeer-network/wasm/x/wasm/artifacts/v155/api"
)

func libwasmvmVersionImpl() (string, error) {
	return api.LibwasmvmVersion()
}

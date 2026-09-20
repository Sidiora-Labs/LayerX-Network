package evm_test

import (
	"encoding/json"
	"testing"

	"github.com/sidiora-labs/paxeer-network/modules/evm"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/node"
	"github.com/stretchr/testify/require"
)

func TestGenesisStreamWaitsForValidationAndReturnsDecodeError(t *testing.T) {
	config := app.MakeEncodingConfig()
	valid := config.Marshaler.MustMarshalJSON(types.DefaultGenesis())
	invalid := config.Marshaler.MustMarshalJSON(&types.GenesisState{})
	cases := []struct {
		name      string
		chunks    []json.RawMessage
		wantError bool
	}{
		{"valid", []json.RawMessage{valid}, false},
		{"invalid parameters", []json.RawMessage{invalid}, true},
		{"malformed first", []json.RawMessage{json.RawMessage(`{`)}, true},
		{"malformed after valid", []json.RawMessage{valid, json.RawMessage(`{`)}, true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			for iteration := 0; iteration < 100; iteration++ {
				chunks := make(chan json.RawMessage, len(tc.chunks))
				for _, chunk := range tc.chunks {
					chunks <- chunk
				}
				close(chunks)
				err := (evm.AppModuleBasic{}).ValidateGenesisStream(config.Marshaler, config.TxConfig, chunks)
				if tc.wantError {
					require.Error(t, err)
				} else {
					require.NoError(t, err)
				}
			}
		})
	}
}

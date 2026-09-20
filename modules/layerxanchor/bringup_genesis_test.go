package layerxanchor_test

import (
	"encoding/hex"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/require"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"

	"github.com/sidiora-labs/paxeer-network/modules/layerxanchor"
	"github.com/sidiora-labs/paxeer-network/modules/layerxanchor/types"
)

// The bring-up writes the anchor section with platform/hosted/paxeer/anchor-genesis.py and paxd
// validate-genesis runs AppModuleBasic.ValidateGenesis over it. This runs the same script and
// the same validation.
func TestBringUpAnchorGenesisValidates(t *testing.T) {
	python, err := exec.LookPath("python3")
	require.NoError(t, err)
	script, err := filepath.Abs("../../platform/hosted/paxeer/anchor-genesis.py")
	require.NoError(t, err)
	deployer := "70997970c51812dc3a010c7d01b50e0d17dc79c8"
	sequencer := "11" + hex.EncodeToString(make([]byte, 31))
	key := "22" + hex.EncodeToString(make([]byte, 31))
	run := func(output string, extra ...string) ([]byte, error) {
		args := append([]string{script, "--authority-evm", "0x" + deployer, "--paxeer-chain-id", "125",
			"--network-id", "7", "--threshold", "2", "--sequencer-id", sequencer,
			"--sequencer-public-key", "0x" + key, "--output", output}, extra...)
		return exec.Command(python, args...).CombinedOutput()
	}

	output := filepath.Join(t.TempDir(), "genesis.json")
	out, err := run(output)
	require.NoError(t, err, string(out))
	raw, err := os.ReadFile(output)
	require.NoError(t, err)
	require.NoError(t, layerxanchor.AppModuleBasic{}.ValidateGenesis(nil, nil, raw))

	var genesis types.GenesisState
	require.NoError(t, json.Unmarshal(raw, &genesis))
	authority, err := hex.DecodeString(deployer)
	require.NoError(t, err)
	require.Equal(t, sdk.AccAddress(authority).String(), genesis.Params.Authority)
	require.Equal(t, uint64(125), genesis.Params.PaxeerChainID)
	require.Equal(t, types.AnchorPrecompileAddress, genesis.Params.SettlementContract)
	require.Equal(t, uint32(7), genesis.Params.NetworkID)
	require.Equal(t, uint32(2), genesis.Params.Threshold)
	require.Equal(t, types.DefaultParams(genesis.Params.Authority).MinBond, genesis.Params.MinBond)
	require.Equal(t, types.DefaultParams(genesis.Params.Authority).BondDenom, genesis.Params.BondDenom)
	require.False(t, genesis.Anchor.Set)
	require.Empty(t, genesis.Guarantors)
	require.Len(t, genesis.Sequencers, 1)
	require.Equal(t, uint64(1), genesis.Sequencers[0].FirstBatchNumber)
	require.Equal(t, uint64(1)<<40, genesis.Sequencers[0].LastBatchNumber)

	// A guarantor set known before the chain starts is genesis state too.
	output = filepath.Join(t.TempDir(), "genesis.json")
	first := "01" + hex.EncodeToString(make([]byte, 31))
	second := "02" + hex.EncodeToString(make([]byte, 31))
	out, err = run(output,
		"--guarantor", first+":00000000000000000000000000000000000000a1:0x"+deployer+":1000000",
		"--guarantor", second+":00000000000000000000000000000000000000a2:0x"+deployer+":2000000")
	require.NoError(t, err, string(out))
	raw, err = os.ReadFile(output)
	require.NoError(t, err)
	require.NoError(t, layerxanchor.AppModuleBasic{}.ValidateGenesis(nil, nil, raw))
	require.NoError(t, json.Unmarshal(raw, &genesis))
	require.Len(t, genesis.Guarantors, 2)
	require.Equal(t, types.GuarantorActive, genesis.Guarantors[0].Status)

	// The script refuses what the module refuses.
	out, err = run(filepath.Join(t.TempDir(), "genesis.json"), "--guarantor", first+":00000000000000000000000000000000000000a1:0x"+deployer+":1")
	require.Error(t, err, string(out))
	out, err = run(filepath.Join(t.TempDir(), "genesis.json"), "--last-batch", "0")
	require.Error(t, err, string(out))
}

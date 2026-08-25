package cmd_test

import (
	"fmt"
	"testing"

	svrcmd "github.com/sidiora-labs/paxeer-network/sdk/server/cmd"
	"github.com/sidiora-labs/paxeer-network/sdk/x/genutil/client/cli"
	"github.com/stretchr/testify/require"

	"github.com/sidiora-labs/paxeer-network/interchain/testing/simapp"
	"github.com/sidiora-labs/paxeer-network/interchain/testing/simapp/simd/cmd"
)

func TestInitCmd(t *testing.T) {
	rootCmd, _ := cmd.NewRootCmd()
	rootCmd.SetArgs([]string{
		"init",        // Test the init cmd
		"simapp-test", // Moniker
		fmt.Sprintf("--%s=%s", cli.FlagOverwrite, "true"), // Overwrite genesis.json, in case it already exists
	})

	require.NoError(t, svrcmd.Execute(rootCmd, simapp.DefaultNodeHome))
}

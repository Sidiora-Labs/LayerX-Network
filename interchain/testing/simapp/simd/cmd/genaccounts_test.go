package cmd_test

import (
	"context"
	"fmt"
	"testing"

	"github.com/sidiora-labs/paxeer-network/sdk/client"
	"github.com/sidiora-labs/paxeer-network/sdk/client/flags"
	"github.com/sidiora-labs/paxeer-network/sdk/server"
	"github.com/sidiora-labs/paxeer-network/sdk/testutil/testdata"
	"github.com/sidiora-labs/paxeer-network/sdk/types/module"
	"github.com/sidiora-labs/paxeer-network/sdk/x/genutil"
	genutiltest "github.com/sidiora-labs/paxeer-network/sdk/x/genutil/client/testutil"
	"github.com/spf13/viper"
	"github.com/stretchr/testify/require"

	"github.com/sidiora-labs/paxeer-network/interchain/testing/simapp"
	simcmd "github.com/sidiora-labs/paxeer-network/interchain/testing/simapp/simd/cmd"
)

var testMbm = module.NewBasicManager(genutil.AppModuleBasic{})

func TestAddGenesisAccountCmd(t *testing.T) {
	_, _, addr1 := testdata.KeyTestPubAddr()
	tests := []struct {
		name      string
		addr      string
		denom     string
		expectErr bool
	}{
		{
			name:      "invalid address",
			addr:      "",
			denom:     "1000atom",
			expectErr: true,
		},
		{
			name:      "valid address",
			addr:      addr1.String(),
			denom:     "1000atom",
			expectErr: false,
		},
		{
			name:      "multiple denoms",
			addr:      addr1.String(),
			denom:     "1000atom, 2000uhpx",
			expectErr: false,
		},
	}

	for _, tc := range tests {
		tc := tc
		t.Run(tc.name, func(t *testing.T) {
			home := t.TempDir()

			cfg, err := genutiltest.CreateDefaultTendermintConfig(home)
			require.NoError(t, err)

			appCodec := simapp.MakeTestEncodingConfig().Marshaler
			err = genutiltest.ExecInitCmd(testMbm, home, appCodec)
			require.NoError(t, err)

			serverCtx := server.NewContext(viper.New(), cfg)
			clientCtx := client.Context{}.WithJSONCodec(appCodec).WithHomeDir(home)

			ctx := t.Context()
			ctx = context.WithValue(ctx, client.ClientContextKey, &clientCtx)
			ctx = context.WithValue(ctx, server.ServerContextKey, serverCtx)

			cmd := simcmd.AddGenesisAccountCmd(home)
			cmd.SetArgs([]string{
				tc.addr,
				tc.denom,
				fmt.Sprintf("--%s=home", flags.FlagHome),
			})

			if tc.expectErr {
				require.Error(t, cmd.ExecuteContext(ctx))
			} else {
				require.NoError(t, cmd.ExecuteContext(ctx))
			}
		})
	}
}

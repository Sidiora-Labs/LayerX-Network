package testutil

import (
	"github.com/sidiora-labs/paxeer-network/sdk/testutil"
	clitestutil "github.com/sidiora-labs/paxeer-network/sdk/testutil/cli"
	"github.com/sidiora-labs/paxeer-network/sdk/testutil/network"
	"github.com/sidiora-labs/paxeer-network/sdk/x/authz/client/cli"
)

func ExecGrant(val *network.Validator, args []string) (testutil.BufferWriter, error) {
	cmd := cli.NewCmdGrantAuthorization()
	clientCtx := val.ClientCtx
	return clitestutil.ExecTestCLICmd(clientCtx, cmd, args)
}

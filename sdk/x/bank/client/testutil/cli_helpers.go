package testutil

import (
	"fmt"

	"github.com/sidiora-labs/paxeer-network/consensus/libs/cli"

	"github.com/sidiora-labs/paxeer-network/sdk/client"
	"github.com/sidiora-labs/paxeer-network/sdk/testutil"
	clitestutil "github.com/sidiora-labs/paxeer-network/sdk/testutil/cli"
	bankcli "github.com/sidiora-labs/paxeer-network/sdk/x/bank/client/cli"
)

func MsgSendExec(clientCtx client.Context, from, to, amount fmt.Stringer, extraArgs ...string) (testutil.BufferWriter, error) {
	args := make([]string, 0, 3+len(extraArgs))
	args = append(args, from.String(), to.String(), amount.String())
	args = append(args, extraArgs...)

	return clitestutil.ExecTestCLICmd(clientCtx, bankcli.NewSendTxCmd(), args)
}

func QueryBalancesExec(clientCtx client.Context, address fmt.Stringer, extraArgs ...string) (testutil.BufferWriter, error) {
	args := make([]string, 0, 2+len(extraArgs))
	args = append(args, address.String(), fmt.Sprintf("--%s=json", cli.OutputFlag))
	args = append(args, extraArgs...)

	return clitestutil.ExecTestCLICmd(clientCtx, bankcli.GetBalancesCmd(), args)
}

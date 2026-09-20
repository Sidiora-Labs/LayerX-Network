package cli

import (
	"context"

	"github.com/sidiora-labs/paxeer-network/modules/epoch/types"
	"github.com/sidiora-labs/paxeer-network/sdk/client"
	"github.com/sidiora-labs/paxeer-network/sdk/client/flags"
	"github.com/spf13/cobra"
)

func CmdQueryEpoch() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "epoch",
		Short: "gets the current epoch",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			clientCtx := client.GetClientContextFromCmd(cmd)

			queryClient := types.NewQueryClient(clientCtx)

			res, err := queryClient.Epoch(context.Background(), &types.QueryEpochRequest{})
			if err != nil {
				return err
			}

			return clientCtx.PrintProto(res)
		},
	}

	flags.AddQueryFlagsToCmd(cmd)

	return cmd
}

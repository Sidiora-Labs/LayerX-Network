package cli

import (
	"fmt"
	"os"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	govcli "github.com/sidiora-labs/paxeer-network/sdk/x/gov/client/cli"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
	"github.com/spf13/cobra"

	mintrest "github.com/sidiora-labs/paxeer-network/modules/mint/client/rest"
	"github.com/sidiora-labs/paxeer-network/modules/mint/types"
	"github.com/sidiora-labs/paxeer-network/sdk/client"
	"github.com/sidiora-labs/paxeer-network/sdk/client/flags"
	"github.com/sidiora-labs/paxeer-network/sdk/client/tx"
	govclient "github.com/sidiora-labs/paxeer-network/sdk/x/gov/client"
)

var UpdateMinterHandler = govclient.NewProposalHandler(MsgUpdateMinterProposalCmd, mintrest.UpdateResourceDependencyProposalRESTHandler)

func GetTxCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:                        types.ModuleName,
		Short:                      fmt.Sprintf("%s transactions subcommands", types.ModuleName),
		DisableFlagParsing:         true,
		SuggestionsMinimumDistance: 2,
		RunE:                       client.ValidateCmd,
	}

	updateMinterProposalCmd := MsgUpdateMinterProposalCmd()
	flags.AddTxFlagsToCmd(updateMinterProposalCmd)

	cmd.AddCommand(updateMinterProposalCmd)
	return cmd
}

func MsgUpdateMinterProposalCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "update-minter [proposal-file]",
		Args:  cobra.ExactArgs(1),
		Short: "Submit an UpdateMinter proposal",
		Long: "Submit a proposal to update the current minter. \n" +
			"E.g. $ paxd tx gov submit-proposal update-minter [proposal-file]\n" +
			"The proposal file should contain the following:\n" +
			"{\n" +
			"\t title: [title],\n" +
			"\t description: [description],\n" +
			"\t minter: [new minter object] \n" +
			"}",
		RunE: func(cmd *cobra.Command, args []string) error {
			clientCtx, err := client.GetClientTxContext(cmd)
			if err != nil {
				return err
			}

			proposal := types.UpdateMinterProposal{}

			contents, err := os.ReadFile(args[0])
			if err != nil {
				return err
			}

			clientCtx.Codec.MustUnmarshalJSON(contents, &proposal)

			from := clientCtx.GetFromAddress()

			content := types.UpdateMinterProposal{
				Title:       proposal.Title,
				Description: proposal.Description,
				Minter:      proposal.Minter,
			}

			depositInput, err := cmd.Flags().GetString(govcli.FlagDeposit)
			if err != nil {
				return err
			}

			deposit, err := sdk.ParseCoinsNormalized(depositInput)
			if err != nil {
				return err
			}

			msg, err := govtypes.NewMsgSubmitProposal(&content, deposit, from)
			if err != nil {

				return err
			}

			return tx.GenerateOrBroadcastTxCLI(cmd.Context(), clientCtx, cmd.Flags(), msg)
		},
	}

	cmd.Flags().String(govcli.FlagDeposit, "", "The proposal deposit")

	return cmd
}

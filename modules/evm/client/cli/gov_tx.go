package cli

import (
	"fmt"
	"os"
	"strconv"
	"strings"

	"github.com/sidiora-labs/paxeer-network/modules/evm/types"

	"github.com/sidiora-labs/paxeer-network/sdk/client"
	"github.com/sidiora-labs/paxeer-network/sdk/client/flags"
	"github.com/sidiora-labs/paxeer-network/sdk/client/tx"
	"github.com/sidiora-labs/paxeer-network/sdk/codec"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	govcli "github.com/sidiora-labs/paxeer-network/sdk/x/gov/client/cli"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"

	"github.com/spf13/cobra"
)

func NewAddERCNativePointerProposalTxCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "add-erc-native-pointer title description token name symbol decimals deposit",
		Args:  cobra.ExactArgs(7),
		Short: "Submit an add ERC-native pointer proposal",
		Long: strings.TrimSpace(`
			Submit a proposal to register an ERC pointer contract for a native token with
			provided metadata.
		`),
		RunE: func(cmd *cobra.Command, args []string) error {
			clientCtx, err := client.GetClientTxContext(cmd)
			if err != nil {
				return err
			}

			decimals, err := strconv.ParseUint(args[5], 10, 8)
			if err != nil {
				return err
			}
			deposit, err := sdk.ParseCoinsNormalized(args[6])
			if err != nil {
				return err
			}

			// Convert proposal to RegisterPairsProposal Type
			from := clientCtx.GetFromAddress()

			content := types.AddERCNativePointerProposalV2{
				Title:       args[0],
				Description: args[1],
				Token:       args[2],
				Name:        args[3],
				Symbol:      args[4],
				Decimals:    uint32(decimals),
			}

			msg, err := govtypes.NewMsgSubmitProposal(&content, deposit, from)
			if err != nil {
				return err
			}

			return tx.GenerateOrBroadcastTxCLI(cmd.Context(), clientCtx, cmd.Flags(), msg)
		},
	}

	flags.AddTxFlagsToCmd(cmd)

	return cmd
}

// NewBindERCNativePointerProposalTxCmd submits a pointer binding proposal
// file: the PointerBindingProposal under its type URL, its title and
// description, and the MsgBindERCNativePointer messages it carries, each
// naming the governance module account as its authority. Once the proposal
// passes, the chain binds each already deployed ERC20 address as the pointer
// of its native denom without deploying a pointer contract.
func NewBindERCNativePointerProposalTxCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "bind-erc-native-pointer [proposal-file]",
		Args:  cobra.ExactArgs(1),
		Short: "Submit a proposal binding a deployed ERC20 as a native denom's pointer",
		Long: strings.TrimSpace(`
			Submit a pointer binding proposal from a file holding the proposal content
			under its type URL, with the MsgBindERCNativePointer messages it carries.
			A file that is not a pointer binding proposal, or that governance would
			refuse, is refused.
		`),
		RunE: func(cmd *cobra.Command, args []string) error {
			clientCtx, err := client.GetClientTxContext(cmd)
			if err != nil {
				return err
			}

			contents, err := os.ReadFile(args[0])
			if err != nil {
				return err
			}

			depositInput, err := cmd.Flags().GetString(govcli.FlagDeposit)
			if err != nil {
				return err
			}

			msg, err := NewSubmitPointerBindingProposalMsg(clientCtx.Codec, contents, depositInput, clientCtx.GetFromAddress())
			if err != nil {
				return fmt.Errorf("%s: %w", args[0], err)
			}

			return tx.GenerateOrBroadcastTxCLI(cmd.Context(), clientCtx, cmd.Flags(), msg)
		},
	}

	cmd.Flags().String(govcli.FlagDeposit, "", "The proposal deposit")
	flags.AddTxFlagsToCmd(cmd)

	return cmd
}

// NewSubmitPointerBindingProposalMsg decodes a pointer binding proposal file
// and builds the MsgSubmitProposal that carries it. It refuses a file that is
// not a pointer binding proposal, a proposal governance would refuse, a
// missing or zero deposit and a missing proposer.
func NewSubmitPointerBindingProposalMsg(cdc codec.JSONCodec, body []byte, depositInput string, proposer sdk.AccAddress) (*govtypes.MsgSubmitProposal, error) {
	content, err := DecodePointerBindingProposal(cdc, body)
	if err != nil {
		return nil, err
	}
	if strings.TrimSpace(depositInput) == "" {
		return nil, fmt.Errorf("the proposal deposit is missing: pass --%s", govcli.FlagDeposit)
	}
	deposit, err := sdk.ParseCoinsNormalized(depositInput)
	if err != nil {
		return nil, fmt.Errorf("the proposal deposit %q: %w", depositInput, err)
	}
	if deposit.IsZero() {
		return nil, fmt.Errorf("the proposal deposit %q is zero", depositInput)
	}
	if proposer.Empty() {
		return nil, fmt.Errorf("the proposer is missing: name the signer with --from")
	}
	msg, err := govtypes.NewMsgSubmitProposal(content, deposit, proposer)
	if err != nil {
		return nil, err
	}
	if err := msg.ValidateBasic(); err != nil {
		return nil, err
	}
	return msg, nil
}

// DecodePointerBindingProposal reads a proposal file into the pointer binding
// proposal content through the codec, resolving every carried message under
// its type URL, and requires the content to pass ValidateBasic.
func DecodePointerBindingProposal(cdc codec.JSONCodec, body []byte) (*types.PointerBindingProposal, error) {
	var content govtypes.Content
	if err := cdc.UnmarshalInterfaceJSON(body, &content); err != nil {
		return nil, fmt.Errorf("decode the proposal: %w", err)
	}
	proposal, ok := content.(*types.PointerBindingProposal)
	if !ok {
		return nil, fmt.Errorf("the proposal is a %T, not a %T", content, proposal)
	}
	if err := proposal.ValidateBasic(); err != nil {
		return nil, err
	}
	return proposal, nil
}

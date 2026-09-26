// Package cli mounts the bridge module's governance proposal on the node's
// governance submit command. The module's messages are sent only by
// governance, so the module has no transaction command of its own: an operator
// submits a proposal file the bridge proposal generator wrote, as it stands, as
// the content of a governance proposal.
package cli

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"sort"
	"strings"

	"github.com/spf13/cobra"

	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	"github.com/sidiora-labs/paxeer-network/sdk/client"
	"github.com/sidiora-labs/paxeer-network/sdk/client/tx"
	"github.com/sidiora-labs/paxeer-network/sdk/codec"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	typesrest "github.com/sidiora-labs/paxeer-network/sdk/types/rest"
	govclient "github.com/sidiora-labs/paxeer-network/sdk/x/gov/client"
	govcli "github.com/sidiora-labs/paxeer-network/sdk/x/gov/client/cli"
	govrest "github.com/sidiora-labs/paxeer-network/sdk/x/gov/client/rest"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
)

const (
	// ProposalCommandName is the subcommand of paxd tx gov submit-proposal
	// that submits a bridge proposal.
	ProposalCommandName = "layerxbridge-proposal"

	// ProposalRESTSubRoute is the sub-route of the governance proposal REST
	// endpoint that submits a bridge proposal.
	ProposalRESTSubRoute = "layerxbridge"
)

// BridgeProposalHandler mounts the bridge proposal on the governance submit
// command and on the governance proposal REST endpoint.
var BridgeProposalHandler = govclient.NewProposalHandler(NewSubmitBridgeProposalCmd, BridgeProposalRESTHandler)

// NewSubmitBridgeProposalCmd returns the subcommand of paxd tx gov
// submit-proposal that submits one proposal file the bridge proposal
// generator wrote. The deposit comes from --deposit and the proposer is the
// signer the standard transaction flags name.
func NewSubmitBridgeProposalCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   ProposalCommandName + " [proposal-file]",
		Args:  cobra.ExactArgs(1),
		Short: "Submit a bridge proposal the bridge proposal generator wrote",
		Long: "Submit a bridge governance proposal.\n" +
			"E.g. $ paxd tx gov submit-proposal " + ProposalCommandName + " 04-proposal-open-chain.json --deposit [coins] --from [key]\n" +
			"The proposal file is 04-proposal-open-chain.json or 05-proposal-sidiora-cap.json exactly as the\n" +
			"generator writes it through -proposals: the bridge proposal under its type URL, its title and\n" +
			"description, and every message it carries in submission order. A file with an unknown or a missing\n" +
			"field is refused.",
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

			msg, err := NewSubmitBridgeProposalMsg(clientCtx.Codec, contents, depositInput, clientCtx.GetFromAddress())
			if err != nil {
				return fmt.Errorf("%s: %w", args[0], err)
			}

			return tx.GenerateOrBroadcastTxCLI(cmd.Context(), clientCtx, cmd.Flags(), msg)
		},
	}

	cmd.Flags().String(govcli.FlagDeposit, "", "The proposal deposit")

	return cmd
}

// NewSubmitBridgeProposalMsg decodes one generated proposal file into the
// bridge proposal content and builds the MsgSubmitProposal that carries it.
// It refuses a file that is not a bridge proposal, a file with an unknown or a
// missing field, a proposal the governance route would refuse, a missing or
// zero deposit and a missing proposer.
func NewSubmitBridgeProposalMsg(cdc codec.JSONCodec, body []byte, depositInput string, proposer sdk.AccAddress) (*govtypes.MsgSubmitProposal, error) {
	content, err := DecodeBridgeProposal(cdc, body)
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

// DecodeBridgeProposal reads a proposal file into the bridge proposal content
// through the codec, resolving every carried message under its type URL with
// unknown fields forbidden. Every field the codec writes for that content
// must be present in the file, so a file only partly written is refused
// rather than read with defaults, and the content must pass ValidateBasic.
func DecodeBridgeProposal(cdc codec.JSONCodec, body []byte) (*types.BridgeProposal, error) {
	var content govtypes.Content
	if err := cdc.UnmarshalInterfaceJSON(body, &content); err != nil {
		return nil, fmt.Errorf("decode the proposal: %w", err)
	}
	proposal, ok := content.(*types.BridgeProposal)
	if !ok {
		return nil, fmt.Errorf("the proposal is a %T, not a %T", content, proposal)
	}
	written, err := cdc.MarshalInterfaceJSON(proposal)
	if err != nil {
		return nil, fmt.Errorf("encode the decoded proposal: %w", err)
	}
	var given, want any
	if err := decodeJSON(body, &given); err != nil {
		return nil, fmt.Errorf("decode the proposal: %w", err)
	}
	if err := decodeJSON(written, &want); err != nil {
		return nil, fmt.Errorf("decode the encoded proposal: %w", err)
	}
	if err := requireFields("", given, want); err != nil {
		return nil, err
	}
	if err := proposal.ValidateBasic(); err != nil {
		return nil, err
	}
	return proposal, nil
}

func decodeJSON(raw []byte, out *any) error {
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.UseNumber()
	if err := decoder.Decode(out); err != nil {
		return err
	}
	if _, err := decoder.Token(); err != io.EOF {
		return fmt.Errorf("more than one JSON value")
	}
	return nil
}

// requireFields refuses the first object key the codec writes that the given
// JSON does not carry, naming its path.
func requireFields(path string, given, want any) error {
	switch w := want.(type) {
	case map[string]any:
		g, ok := given.(map[string]any)
		if !ok {
			return fmt.Errorf("the field %s is not an object", fieldName(path))
		}
		keys := make([]string, 0, len(w))
		for key := range w {
			keys = append(keys, key)
		}
		sort.Strings(keys)
		for _, key := range keys {
			value, present := g[key]
			if !present {
				return fmt.Errorf("the field %s is missing", fieldName(path+"."+key))
			}
			if err := requireFields(path+"."+key, value, w[key]); err != nil {
				return err
			}
		}
	case []any:
		g, ok := given.([]any)
		if !ok || len(g) != len(w) {
			return fmt.Errorf("the field %s does not carry %d entries", fieldName(path), len(w))
		}
		for i := range w {
			if err := requireFields(fmt.Sprintf("%s[%d]", path, i), g[i], w[i]); err != nil {
				return err
			}
		}
	}
	return nil
}

func fieldName(path string) string {
	if path == "" {
		return "(document)"
	}
	return strings.TrimPrefix(path, ".")
}

// BridgeProposalRequest is the body of the governance proposal REST endpoint's
// bridge sub-route: the transaction's base request, the deposit in the form
// --deposit takes and the proposal file as the generator writes it.
type BridgeProposalRequest struct {
	BaseReq  typesrest.BaseReq `json:"base_req" yaml:"base_req"`
	Deposit  string            `json:"deposit" yaml:"deposit"`
	Proposal json.RawMessage   `json:"proposal" yaml:"proposal"`
}

// BridgeProposalRESTHandler returns the bridge sub-route of the governance
// proposal REST endpoint, which writes the unsigned submit-proposal
// transaction for a generated proposal.
func BridgeProposalRESTHandler(clientCtx client.Context) govrest.ProposalRESTHandler {
	return govrest.ProposalRESTHandler{
		SubRoute: ProposalRESTSubRoute,
		Handler:  newBridgeProposalPostHandler(clientCtx),
	}
}

func newBridgeProposalPostHandler(clientCtx client.Context) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		raw, err := io.ReadAll(r.Body)
		if typesrest.CheckBadRequestError(w, err) {
			return
		}
		var req BridgeProposalRequest
		decoder := json.NewDecoder(bytes.NewReader(raw))
		decoder.DisallowUnknownFields()
		if typesrest.CheckBadRequestError(w, decoder.Decode(&req)) {
			return
		}

		req.BaseReq = req.BaseReq.Sanitize()
		if !req.BaseReq.ValidateBasic(w) {
			return
		}

		fromAddr, err := sdk.AccAddressFromBech32(req.BaseReq.From)
		if typesrest.CheckBadRequestError(w, err) {
			return
		}

		msg, err := NewSubmitBridgeProposalMsg(clientCtx.Codec, req.Proposal, req.Deposit, fromAddr)
		if typesrest.CheckBadRequestError(w, err) {
			return
		}

		tx.WriteGeneratedTxResponse(clientCtx, w, req.BaseReq, msg)
	}
}

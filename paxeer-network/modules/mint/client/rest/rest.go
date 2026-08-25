package rest

import (
	"net/http"

	"github.com/gorilla/mux"
	"github.com/sidiora-labs/paxeer-network/modules/mint/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	govrest "github.com/sidiora-labs/paxeer-network/sdk/x/gov/client/rest"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"

	"github.com/sidiora-labs/paxeer-network/sdk/client"
	clientrest "github.com/sidiora-labs/paxeer-network/sdk/client/rest"
	"github.com/sidiora-labs/paxeer-network/sdk/client/tx"
	typesrest "github.com/sidiora-labs/paxeer-network/sdk/types/rest"
)

// RegisterRoutes registers minting module REST handlers on the provided router.
func RegisterRoutes(clientCtx client.Context, rtr *mux.Router) {
	r := clientrest.WithHTTPDeprecationHeaders(rtr)
	registerQueryRoutes(clientCtx, r)
}

// PlanRequest defines a proposal for a new upgrade plan.
type UpdateMinterRequest struct {
	BaseReq     typesrest.BaseReq `json:"base_req" yaml:"base_req"`
	Title       string            `json:"title" yaml:"title"`
	Description string            `json:"description" yaml:"description"`
	Deposit     sdk.Coins         `json:"deposit" yaml:"deposit"`
	Minter      types.Minter      `json:"minter" yaml:"minter"`
}

func UpdateResourceDependencyProposalRESTHandler(clientCtx client.Context) govrest.ProposalRESTHandler {
	return govrest.ProposalRESTHandler{
		SubRoute: "update_minter",
		Handler:  newUpdateMinterPostHandler(clientCtx),
	}
}

func newUpdateMinterPostHandler(clientCtx client.Context) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		var req UpdateMinterRequest

		if !typesrest.ReadRESTReq(w, r, clientCtx.LegacyAmino, &req) {
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

		content := types.NewUpdateMinterProposalHandler(
			req.Title, req.Description, req.Minter,
		)
		msg, err := govtypes.NewMsgSubmitProposal(content, req.Deposit, fromAddr)
		if typesrest.CheckBadRequestError(w, err) {
			return
		}
		if typesrest.CheckBadRequestError(w, msg.ValidateBasic()) {
			return
		}

		tx.WriteGeneratedTxResponse(clientCtx, w, req.BaseReq, msg)
	}
}

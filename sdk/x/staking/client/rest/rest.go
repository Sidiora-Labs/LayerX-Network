package rest

import (
	"github.com/gorilla/mux"

	"github.com/sidiora-labs/paxeer-network/sdk/client"
	"github.com/sidiora-labs/paxeer-network/sdk/client/rest"
)

func RegisterHandlers(clientCtx client.Context, rtr *mux.Router) {
	r := rest.WithHTTPDeprecationHeaders(rtr)
	registerQueryRoutes(clientCtx, r)
	registerTxHandlers(clientCtx, r)
}

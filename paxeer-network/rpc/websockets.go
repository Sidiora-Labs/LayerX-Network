package evmrpc

import (
	"net/http"

	utilmetrics "github.com/sidiora-labs/paxeer-network/utils/metrics"
)

type wsConnectionHandler struct {
	underlying http.Handler
}

func (h *wsConnectionHandler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	recordWebsocketConnect(r.Context())
	// TODO(PLT-326): remove legacy dual-emit once dashboards are migrated to evmrpc_* OTEL metrics. Use metrics.wsConnectionCount instead.
	utilmetrics.IncWebsocketConnects()
	h.underlying.ServeHTTP(w, r)
}

func NewWSConnectionHandler(handler http.Handler) http.Handler {
	return &wsConnectionHandler{underlying: handler}
}

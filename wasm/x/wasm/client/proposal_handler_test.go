package client

import (
	"bytes"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"testing"

	"github.com/gorilla/mux"
	"github.com/sidiora-labs/paxeer-network/sdk/client"
	"github.com/sidiora-labs/paxeer-network/sdk/client/flags"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
	"github.com/stretchr/testify/require"

	"github.com/sidiora-labs/paxeer-network/wasm/x/wasm/keeper"
)

func TestGovRestHandlers(t *testing.T) {
	type dict map[string]interface{}
	var (
		anyAddress = "pax10xwrnrezdg227cgt82az7f7j47q3zklvdh3g0l"
		aBaseReq   = dict{
			"from":           anyAddress,
			"memo":           "rest test",
			"chain_id":       "testing",
			"account_number": "1",
			"sequence":       "1",
			"fees":           []dict{{"denom": "ustake", "amount": "1000000"}},
		}
	)
	encodingConfig := keeper.MakeEncodingConfig(t)
	clientCtx := client.Context{}.
		WithCodec(encodingConfig.Marshaler).
		WithTxConfig(encodingConfig.TxConfig).
		WithLegacyAmino(encodingConfig.Amino).
		WithInput(os.Stdin).
		WithAccountRetriever(authtypes.AccountRetriever{}).
		WithBroadcastMode(flags.BroadcastBlock).
		WithChainID("testing")

	// router setup as in gov/client/rest/tx.go
	propSubRtr := mux.NewRouter().PathPrefix("/gov/proposals").Subrouter()
	for _, ph := range ProposalHandlers {
		r := ph.RESTHandler(clientCtx)
		propSubRtr.HandleFunc(fmt.Sprintf("/%s", r.SubRoute), r.Handler).Methods("POST")
	}

	specs := map[string]struct {
		srcBody dict
		srcPath string
		expCode int
	}{
		"store-code": {
			srcPath: "/gov/proposals/wasm_store_code",
			srcBody: dict{
				"title":          "Test Proposal",
				"description":    "My proposal",
				"type":           "store-code",
				"run_as":         "pax10xwrnrezdg227cgt82az7f7j47q3zklvdh3g0l",
				"wasm_byte_code": []byte("valid wasm byte code"),
				"source":         "https://example.com/",
				"builder":        "my/builder:tag",
				"instantiate_permission": dict{
					"permission": "OnlyAddress",
					"address":    "pax1rs8v2232uv5nw8c88ruvyjy08mmxfx25sl0l5k",
				},
				"deposit":  []dict{{"denom": "ustake", "amount": "10"}},
				"proposer": "pax1rs8v2232uv5nw8c88ruvyjy08mmxfx25sl0l5k",
				"base_req": aBaseReq,
			},
			expCode: http.StatusOK,
		},
		"store-code without permission": {
			srcPath: "/gov/proposals/wasm_store_code",
			srcBody: dict{
				"title":          "Test Proposal",
				"description":    "My proposal",
				"type":           "store-code",
				"run_as":         "pax10xwrnrezdg227cgt82az7f7j47q3zklvdh3g0l",
				"wasm_byte_code": []byte("valid wasm byte code"),
				"source":         "https://example.com/",
				"builder":        "my/builder:tag",
				"deposit":        []dict{{"denom": "ustake", "amount": "10"}},
				"proposer":       "pax1rs8v2232uv5nw8c88ruvyjy08mmxfx25sl0l5k",
				"base_req":       aBaseReq,
			},
			expCode: http.StatusOK,
		},
		"store-code invalid permission": {
			srcPath: "/gov/proposals/wasm_store_code",
			srcBody: dict{
				"title":          "Test Proposal",
				"description":    "My proposal",
				"type":           "store-code",
				"run_as":         "pax10xwrnrezdg227cgt82az7f7j47q3zklvdh3g0l",
				"wasm_byte_code": []byte("valid wasm byte code"),
				"source":         "https://example.com/",
				"builder":        "my/builder:tag",
				"instantiate_permission": dict{
					"permission": "Nobody",
					"address":    "pax1rs8v2232uv5nw8c88ruvyjy08mmxfx25sl0l5k",
				},
				"deposit":  []dict{{"denom": "ustake", "amount": "10"}},
				"proposer": "pax1rs8v2232uv5nw8c88ruvyjy08mmxfx25sl0l5k",
				"base_req": aBaseReq,
			},
			expCode: http.StatusBadRequest,
		},
		"store-code with incomplete proposal data: blank title": {
			srcPath: "/gov/proposals/wasm_store_code",
			srcBody: dict{
				"title":          "",
				"description":    "My proposal",
				"type":           "store-code",
				"run_as":         "pax10xwrnrezdg227cgt82az7f7j47q3zklvdh3g0l",
				"wasm_byte_code": []byte("valid wasm byte code"),
				"source":         "https://example.com/",
				"builder":        "my/builder:tag",
				"instantiate_permission": dict{
					"permission": "OnlyAddress",
					"address":    "pax1rs8v2232uv5nw8c88ruvyjy08mmxfx25sl0l5k",
				},
				"deposit":  []dict{{"denom": "ustake", "amount": "10"}},
				"proposer": "pax1rs8v2232uv5nw8c88ruvyjy08mmxfx25sl0l5k",
				"base_req": aBaseReq,
			},
			expCode: http.StatusBadRequest,
		},
		"store-code with incomplete content data: no wasm_byte_code": {
			srcPath: "/gov/proposals/wasm_store_code",
			srcBody: dict{
				"title":          "Test Proposal",
				"description":    "My proposal",
				"type":           "store-code",
				"run_as":         "pax10xwrnrezdg227cgt82az7f7j47q3zklvdh3g0l",
				"wasm_byte_code": "",
				"source":         "https://example.com/",
				"builder":        "my/builder:tag",
				"instantiate_permission": dict{
					"permission": "OnlyAddress",
					"address":    "pax1rs8v2232uv5nw8c88ruvyjy08mmxfx25sl0l5k",
				},
				"deposit":  []dict{{"denom": "ustake", "amount": "10"}},
				"proposer": "pax1rs8v2232uv5nw8c88ruvyjy08mmxfx25sl0l5k",
				"base_req": aBaseReq,
			},
			expCode: http.StatusBadRequest,
		},
		"instantiate contract": {
			srcPath: "/gov/proposals/wasm_instantiate",
			srcBody: dict{
				"title":       "Test Proposal",
				"description": "My proposal",
				"type":        "instantiate",
				"run_as":      "pax10xwrnrezdg227cgt82az7f7j47q3zklvdh3g0l",
				"admin":       "pax10xwrnrezdg227cgt82az7f7j47q3zklvdh3g0l",
				"code_id":     "1",
				"label":       "https://example.com/",
				"msg":         dict{"recipient": "pax10xwrnrezdg227cgt82az7f7j47q3zklvdh3g0l"},
				"funds":       []dict{{"denom": "ustake", "amount": "100"}},
				"deposit":     []dict{{"denom": "ustake", "amount": "10"}},
				"proposer":    "pax1rs8v2232uv5nw8c88ruvyjy08mmxfx25sl0l5k",
				"base_req":    aBaseReq,
			},
			expCode: http.StatusOK,
		},
		"migrate contract": {
			srcPath: "/gov/proposals/wasm_migrate",
			srcBody: dict{
				"title":       "Test Proposal",
				"description": "My proposal",
				"type":        "migrate",
				"contract":    "pax14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9snf99un",
				"code_id":     "1",
				"msg":         dict{"foo": "bar"},
				"run_as":      "pax10xwrnrezdg227cgt82az7f7j47q3zklvdh3g0l",
				"deposit":     []dict{{"denom": "ustake", "amount": "10"}},
				"proposer":    "pax1rs8v2232uv5nw8c88ruvyjy08mmxfx25sl0l5k",
				"base_req":    aBaseReq,
			},
			expCode: http.StatusOK,
		},
		"execute contract": {
			srcPath: "/gov/proposals/wasm_execute",
			srcBody: dict{
				"title":       "Test Proposal",
				"description": "My proposal",
				"type":        "migrate",
				"contract":    "pax14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9snf99un",
				"msg":         dict{"foo": "bar"},
				"run_as":      "pax10xwrnrezdg227cgt82az7f7j47q3zklvdh3g0l",
				"deposit":     []dict{{"denom": "ustake", "amount": "10"}},
				"proposer":    "pax1rs8v2232uv5nw8c88ruvyjy08mmxfx25sl0l5k",
				"base_req":    aBaseReq,
			},
			expCode: http.StatusOK,
		},
		"execute contract fails with no run_as": {
			srcPath: "/gov/proposals/wasm_execute",
			srcBody: dict{
				"title":       "Test Proposal",
				"description": "My proposal",
				"type":        "migrate",
				"contract":    "pax14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9snf99un",
				"msg":         dict{"foo": "bar"},
				"deposit":     []dict{{"denom": "ustake", "amount": "10"}},
				"proposer":    "pax1rs8v2232uv5nw8c88ruvyjy08mmxfx25sl0l5k",
				"base_req":    aBaseReq,
			},
			expCode: http.StatusBadRequest,
		},
		"execute contract fails with no message": {
			srcPath: "/gov/proposals/wasm_execute",
			srcBody: dict{
				"title":       "Test Proposal",
				"description": "My proposal",
				"type":        "migrate",
				"contract":    "pax14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9snf99un",
				"run_as":      "pax10xwrnrezdg227cgt82az7f7j47q3zklvdh3g0l",
				"deposit":     []dict{{"denom": "ustake", "amount": "10"}},
				"proposer":    "pax1rs8v2232uv5nw8c88ruvyjy08mmxfx25sl0l5k",
				"base_req":    aBaseReq,
			},
			expCode: http.StatusBadRequest,
		},
		"sudo contract": {
			srcPath: "/gov/proposals/wasm_sudo",
			srcBody: dict{
				"title":       "Test Proposal",
				"description": "My proposal",
				"type":        "migrate",
				"contract":    "pax14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9snf99un",
				"msg":         dict{"foo": "bar"},
				"deposit":     []dict{{"denom": "ustake", "amount": "10"}},
				"proposer":    "pax1rs8v2232uv5nw8c88ruvyjy08mmxfx25sl0l5k",
				"base_req":    aBaseReq,
			},
			expCode: http.StatusOK,
		},
		"sudo contract fails with no message": {
			srcPath: "/gov/proposals/wasm_sudo",
			srcBody: dict{
				"title":       "Test Proposal",
				"description": "My proposal",
				"type":        "migrate",
				"contract":    "pax14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9snf99un",
				"deposit":     []dict{{"denom": "ustake", "amount": "10"}},
				"proposer":    "pax1rs8v2232uv5nw8c88ruvyjy08mmxfx25sl0l5k",
				"base_req":    aBaseReq,
			},
			expCode: http.StatusBadRequest,
		},
		"update contract admin": {
			srcPath: "/gov/proposals/wasm_update_admin",
			srcBody: dict{
				"title":       "Test Proposal",
				"description": "My proposal",
				"type":        "migrate",
				"contract":    "pax14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9snf99un",
				"new_admin":   "pax10xwrnrezdg227cgt82az7f7j47q3zklvdh3g0l",
				"deposit":     []dict{{"denom": "ustake", "amount": "10"}},
				"proposer":    "pax1rs8v2232uv5nw8c88ruvyjy08mmxfx25sl0l5k",
				"base_req":    aBaseReq,
			},
			expCode: http.StatusOK,
		},
		"clear contract admin": {
			srcPath: "/gov/proposals/wasm_clear_admin",
			srcBody: dict{
				"title":       "Test Proposal",
				"description": "My proposal",
				"type":        "migrate",
				"contract":    "pax14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9snf99un",
				"deposit":     []dict{{"denom": "ustake", "amount": "10"}},
				"proposer":    "pax1rs8v2232uv5nw8c88ruvyjy08mmxfx25sl0l5k",
				"base_req":    aBaseReq,
			},
			expCode: http.StatusOK,
		},
	}
	for msg, spec := range specs {
		t.Run(msg, func(t *testing.T) {
			src, err := json.Marshal(spec.srcBody)
			require.NoError(t, err)

			// when
			r := httptest.NewRequest("POST", spec.srcPath, bytes.NewReader(src))
			w := httptest.NewRecorder()
			propSubRtr.ServeHTTP(w, r)

			// then
			require.Equal(t, spec.expCode, w.Code, w.Body.String())
		})
	}
}

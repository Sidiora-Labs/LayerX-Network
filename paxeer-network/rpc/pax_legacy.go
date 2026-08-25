package evmrpc

import (
	"sort"
	"strings"

	"github.com/ethereum/go-ethereum/rpc"
)

// PaxLegacyDeprecationHTTPHeader is set on HTTP responses that successfully forwarded an allowlisted
// gated pax_* / pax2_* JSON-RPC call (body is unchanged; clients should not rely on JSON result mutation).
const (
	PaxLegacyDeprecationHTTPHeader = "Pax-Legacy-RPC-Deprecation"
	PaxLegacyDeprecationMessage    = "All pax_* and pax2_* JSON-RPC methods are deprecated and scheduled for removal; migrate to eth_* and supported APIs."
)

// errPaxLegacyNotEnabled is returned when a gated pax_* / pax2_* method is not listed in enabled_legacy_pax_apis.
// It follows github.com/ethereum/go-ethereum/rpc error encoding (jsonrpcMessage.error via rpc.Error / rpc.DataError).
type errPaxLegacyNotEnabled struct {
	method string
}

func (e *errPaxLegacyNotEnabled) Error() string {
	return paxLegacyMethodDisabledMessage(e.method)
}

func (e *errPaxLegacyNotEnabled) ErrorCode() int {
	return paxLegacyNotEnabled
}

func (e *errPaxLegacyNotEnabled) ErrorData() interface{} {
	return "legacy_pax_deprecated"
}

var (
	_ rpc.Error     = (*errPaxLegacyNotEnabled)(nil)
	_ rpc.DataError = (*errPaxLegacyNotEnabled)(nil)
)

// paxLegacyGatedMethods is the full set of JSON-RPC methods on the pax and pax2 namespaces that
// are subject to [evm] enabled_legacy_pax_apis in app.toml (same allowlist for both prefixes).
var paxLegacyGatedMethods = map[string]struct{}{
	"pax_associate":                             {},
	"pax_getBlockByHash":                        {},
	"pax_getBlockByNumber":                      {},
	"pax_getBlockReceipts":                      {},
	"pax_getBlockTransactionCountByHash":        {},
	"pax_getBlockTransactionCountByNumber":      {},
	"pax_getBlockByHashExcludeTraceFail":        {},
	"pax_getBlockByNumberExcludeTraceFail":      {},
	"pax_getCosmosTx":                           {},
	"pax_getEVMAddress":                         {},
	"pax_getEvmTx":                              {},
	"pax_getFilterChanges":                      {},
	"pax_getFilterLogs":                         {},
	"pax_getLogs":                               {},
	"pax_getPaxAddress":                         {},
	"pax_getTransactionByBlockHashAndIndex":     {},
	"pax_getTransactionByBlockNumberAndIndex":   {},
	"pax_getTransactionByHash":                  {},
	"pax_getTransactionCount":                   {},
	"pax_getTransactionErrorByHash":             {},
	"pax_getTransactionReceipt":                 {},
	"pax_getTransactionReceiptExcludeTraceFail": {},
	"pax_getVMError":                            {},
	"pax_newBlockFilter":                        {},
	"pax_newFilter":                             {},
	"pax_sign":                                  {},
	"pax_traceBlockByHashExcludeTraceFail":      {},
	"pax_traceBlockByNumberExcludeTraceFail":    {},
	"pax_uninstallFilter":                       {},
	// pax2_* block namespace (HTTP only; bank transfers in blocks). Gated via the same allowlist.
	"pax2_getBlockByHash":                   {},
	"pax2_getBlockByHashExcludeTraceFail":   {},
	"pax2_getBlockByNumber":                 {},
	"pax2_getBlockByNumberExcludeTraceFail": {},
	"pax2_getBlockReceipts":                 {},
	"pax2_getBlockTransactionCountByHash":   {},
	"pax2_getBlockTransactionCountByNumber": {},
}

// PaxLegacyAllExtraMethodNames returns gated pax_* methods other than the usual default trio
// (pax_getPaxAddress, pax_getEVMAddress, pax_getCosmosTx). Used to compose full test configs.
func PaxLegacyAllExtraMethodNames() []string {
	out := make([]string, 0, len(paxLegacyGatedMethods))
	for m := range paxLegacyGatedMethods {
		switch strings.ToLower(m) {
		case "pax_getpaxaddress", "pax_getevmaddress", "pax_getcosmostx":
			continue
		default:
			out = append(out, m)
		}
	}
	sort.Strings(out)
	return out
}

// PaxLegacyAllGatedMethodNames returns every gated pax_* and pax2_* method (sorted). Use when tests need full parity.
func PaxLegacyAllGatedMethodNames() []string {
	out := make([]string, 0, len(paxLegacyGatedMethods))
	for m := range paxLegacyGatedMethods {
		out = append(out, m)
	}
	sort.Strings(out)
	return out
}

// BuildPaxLegacyEnabledSet returns the set of allowed gated pax_* / pax2_* JSON-RPC methods from
// config only ([evm].enabled_legacy_pax_apis). Names are matched case-insensitively to canonical RPC names.
func BuildPaxLegacyEnabledSet(enabledLegacyPaxApis []string) map[string]struct{} {
	enabled := make(map[string]struct{}, len(enabledLegacyPaxApis))
	for _, raw := range enabledLegacyPaxApis {
		name := strings.TrimSpace(raw)
		if name == "" {
			continue
		}
		canonical := canonicalizePaxLegacyMethodName(name)
		if canonical == "" {
			continue
		}
		if _, ok := paxLegacyGatedMethods[canonical]; ok {
			enabled[canonical] = struct{}{}
		}
	}
	return enabled
}

func canonicalizePaxLegacyMethodName(name string) string {
	lower := strings.ToLower(strings.TrimSpace(name))
	for m := range paxLegacyGatedMethods {
		if strings.ToLower(m) == lower {
			return m
		}
	}
	return ""
}

func paxLegacyMethodDisabledMessage(method string) string {
	return method + " is not enabled on this node. The pax_* and pax2_* JSON-RPC surfaces are deprecated, scheduled for removal, and should not be used for new integrations - " +
		"prefer standard eth_* (and debug_*) methods and official migration guidance. " +
		"To allow this legacy method, add it to enabled_legacy_pax_apis under [evm] in app.toml."
}

func paxLegacyIsGatedNamespaceMethod(method string) bool {
	return strings.HasPrefix(method, "pax2_") || strings.HasPrefix(method, "pax_")
}

// paxLegacyGateError enforces [evm].enabled_legacy_pax_apis when allowlist is non-nil.
// allowlist nil means ungated (HTTP middleware disabled, or non-enforcing paths).
func paxLegacyGateError(method string, allowlist map[string]struct{}) error {
	if allowlist == nil {
		return nil
	}
	if !paxLegacyIsGatedNamespaceMethod(method) {
		return nil
	}
	canon := canonicalizePaxLegacyMethodName(method)
	if canon == "" {
		// Fail closed: pax_* / pax2_* names not in paxLegacyGatedMethods must not bypass the allowlist
		// (e.g. future handlers or typos would otherwise reach the inner server).
		return &errPaxLegacyNotEnabled{method: strings.TrimSpace(method)}
	}
	if _, ok := allowlist[canon]; ok {
		return nil
	}
	return &errPaxLegacyNotEnabled{method: canon}
}

// paxLegacyForwardedGatedMethod is true when the request method is a gated pax_* / pax2_* name listed
// in the allowlist (the call was forwarded to the inner JSON-RPC server). Used only for optional HTTP metadata.
func paxLegacyForwardedGatedMethod(method string, allowlist map[string]struct{}) bool {
	if allowlist == nil {
		return false
	}
	if !paxLegacyIsGatedNamespaceMethod(method) {
		return false
	}
	canon := canonicalizePaxLegacyMethodName(method)
	if canon == "" {
		return false
	}
	_, ok := allowlist[canon]
	return ok
}

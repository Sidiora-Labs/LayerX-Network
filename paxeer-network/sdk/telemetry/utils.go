package telemetry

import "strings"

// DenomClass buckets a denom into a cardinality-bounded class.
// Returns "uhpx", "ibc", "factory", or "other".
func DenomClass(denom string) string {
	switch {
	case denom == "uhpx":
		return "uhpx"
	case strings.HasPrefix(denom, "ibc/"):
		return "ibc"
	case strings.HasPrefix(denom, "factory/"):
		return "factory"
	default:
		return "other"
	}
}

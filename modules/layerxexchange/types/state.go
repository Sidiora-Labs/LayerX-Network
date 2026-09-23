package types

// LayerX perps state lives in native module 6. Its keys are the prefixes of
// src/modules/perps followed by the 32-byte identifiers.
const (
	PerpsModuleID   uint16 = 6
	AccountModuleID uint16 = 0

	SideBuy  uint8 = 1
	SideSell uint8 = 2

	// Time in force is carried to LayerX unchanged.
	TimeInForceGoodTillCancelled uint8 = 0
	TimeInForceImmediateOrCancel uint8 = 1
	TimeInForceFillOrKill        uint8 = 2
	TimeInForcePostOnly          uint8 = 3

	// MaxWitnessBytes bounds every caller-supplied state proof.
	MaxWitnessBytes = 64 * 1024
)

func perpsKey(prefix string, ids ...[32]byte) []byte {
	out := []byte(prefix)
	for _, id := range ids {
		out = append(out, id[:]...)
	}
	return out
}

// MarketStateKey is "market:" || market_id.
func MarketStateKey(marketID [32]byte) []byte { return perpsKey("market:", marketID) }

// OrderStateKey is "order:" || market_id || order_id.
func OrderStateKey(marketID, orderID [32]byte) []byte { return perpsKey("order:", marketID, orderID) }

// PositionStateKey is "position:" || market_id || position_id.
func PositionStateKey(marketID, positionID [32]byte) []byte {
	return perpsKey("position:", marketID, positionID)
}

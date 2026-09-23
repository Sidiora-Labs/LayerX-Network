package types

const (
	// ModuleName defines the module name.
	ModuleName = "layerxexchange"

	// StoreKey defines the primary module store key.
	StoreKey = ModuleName

	// RouterKey is the message route.
	RouterKey = ModuleName

	// QuerierRoute defines the module's query routing key.
	QuerierRoute = ModuleName
)

var (
	ParamsKey        = []byte{0x01}
	IntentCountKey   = []byte{0x02}
	IntentPrefix     = []byte{0x10}
	OwnerNoncePrefix = []byte{0x20}
)

func join(prefix []byte, parts ...[]byte) []byte {
	out := append([]byte(nil), prefix...)
	for _, part := range parts {
		out = append(out, part...)
	}
	return out
}

func IntentKey(intentID [32]byte) []byte  { return join(IntentPrefix, intentID[:]) }
func OwnerNonceKey(owner [20]byte) []byte { return join(OwnerNoncePrefix, owner[:]) }

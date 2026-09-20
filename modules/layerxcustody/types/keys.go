package types

import "encoding/binary"

const (
	// ModuleName defines the module name and the custody module account.
	ModuleName = "layerxcustody"

	// StoreKey defines the primary module store key.
	StoreKey = ModuleName

	// RouterKey is the message route.
	RouterKey = ModuleName

	// QuerierRoute defines the module's query routing key.
	QuerierRoute = ModuleName
)

var (
	ParamsKey          = []byte{0x01}
	EmergencyKey       = []byte{0x02}
	DepositCountKey    = []byte{0x03}
	AssetPrefix        = []byte{0x10}
	AssetPointerPrefix = []byte{0x11}
	AssetTotalsPrefix  = []byte{0x12}
	DepositPrefix      = []byte{0x20}
	DepositIndexPrefix = []byte{0x21}
	DepositNoncePrefix = []byte{0x22}
	ClaimPrefix        = []byte{0x30}
	NullifierPrefix    = []byte{0x40}
	WithdrawalIDPrefix = []byte{0x41}
	ConsumedPrefix     = []byte{0x42}
	CheckpointPrefix   = []byte{0x50}
	LatestCheckpoint   = []byte{0x51}
	DepositRootPrefix  = []byte{0x60}
)

func join(prefix []byte, parts ...[]byte) []byte {
	out := append([]byte(nil), prefix...)
	for _, part := range parts {
		out = append(out, part...)
	}
	return out
}

func AssetKey(assetID [32]byte) []byte       { return join(AssetPrefix, assetID[:]) }
func AssetTotalsKey(assetID [32]byte) []byte { return join(AssetTotalsPrefix, assetID[:]) }
func AssetPointerKey(pointer [20]byte) []byte {
	return join(AssetPointerPrefix, pointer[:])
}
func DepositKey(depositID [32]byte) []byte { return join(DepositPrefix, depositID[:]) }
func DepositRootKey(checkpointID [32]byte) []byte {
	return join(DepositRootPrefix, checkpointID[:])
}
func DepositIndexKey(index uint64) []byte {
	return binary.BigEndian.AppendUint64(append([]byte(nil), DepositIndexPrefix...), index)
}
func DepositNonceKey(depositor [20]byte, assetID [32]byte) []byte {
	return join(DepositNoncePrefix, depositor[:], assetID[:])
}
func ClaimKey(claimID [32]byte) []byte       { return join(ClaimPrefix, claimID[:]) }
func NullifierKey(nullifier [32]byte) []byte { return join(NullifierPrefix, nullifier[:]) }
func WithdrawalIDKey(id [32]byte) []byte     { return join(WithdrawalIDPrefix, id[:]) }
func ConsumedKey(account, assetID, anchor [32]byte) []byte {
	return join(ConsumedPrefix, account[:], assetID[:], anchor[:])
}
func CheckpointKey(batchNumber uint64) []byte {
	return binary.BigEndian.AppendUint64(append([]byte(nil), CheckpointPrefix...), batchNumber)
}

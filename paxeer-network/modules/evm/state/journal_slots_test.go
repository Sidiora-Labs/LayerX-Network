package state

import (
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/stretchr/testify/require"
)

func TestAccessListSlotRollbackPreservesOtherAddressIndexes(t *testing.T) {
	for _, removed := range []int{0, 1} {
		db := &DBImpl{tempState: NewTemporaryState()}
		addresses := []common.Address{{1}, {2}, {3}}
		slots := []common.Hash{{11}, {12}, {13}}
		for i, address := range addresses {
			db.AddSlotToAccessList(address, slots[i])
		}
		(&accessListAddSlotChange{address: addresses[removed], slot: slots[removed]}).revert(db)
		for i, address := range addresses {
			addressFound, slotFound := db.SlotInAccessList(address, slots[i])
			require.True(t, addressFound)
			require.Equal(t, i != removed, slotFound)
		}
		for i := len(addresses) - 1; i >= 0; i-- {
			if i == removed {
				continue
			}
			(&accessListAddSlotChange{address: addresses[i], slot: slots[i]}).revert(db)
			addressFound, slotFound := db.SlotInAccessList(addresses[i], slots[i])
			require.True(t, addressFound)
			require.False(t, slotFound)
		}
		require.Empty(t, db.tempState.transientAccessLists.Slots)
		for _, address := range addresses {
			require.Equal(t, -1, db.tempState.transientAccessLists.Addresses[address])
		}
	}
}

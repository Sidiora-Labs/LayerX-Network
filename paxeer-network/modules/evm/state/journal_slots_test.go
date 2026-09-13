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
			db.tempState.transientAccessLists.Addresses[address] = i
			db.tempState.transientAccessLists.Slots = append(db.tempState.transientAccessLists.Slots, map[common.Hash]struct{}{slots[i]: {}})
		}
		(&accessListAddSlotChange{address: addresses[removed], slot: slots[removed]}).revert(db)
		for i, address := range addresses {
			index, addressFound := db.tempState.transientAccessLists.Addresses[address]
			require.True(t, addressFound)
			if i == removed {
				require.Equal(t, -1, index)
			} else {
				require.Contains(t, db.tempState.transientAccessLists.Slots[index], slots[i])
				require.Len(t, db.tempState.transientAccessLists.Slots[index], 1)
			}
		}
		for i := len(addresses) - 1; i >= 0; i-- {
			if i == removed {
				continue
			}
			(&accessListAddSlotChange{address: addresses[i], slot: slots[i]}).revert(db)
			require.Equal(t, -1, db.tempState.transientAccessLists.Addresses[addresses[i]])
		}
		require.Empty(t, db.tempState.transientAccessLists.Slots)
		for _, address := range addresses {
			require.Equal(t, -1, db.tempState.transientAccessLists.Addresses[address])
		}
	}
}

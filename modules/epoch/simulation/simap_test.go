package simulation_test

import (
	"testing"

	epochsimulation "github.com/sidiora-labs/paxeer-network/modules/epoch/simulation"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	simtypes "github.com/sidiora-labs/paxeer-network/sdk/types/simulation"

	"github.com/stretchr/testify/require"
)

func TestFindAccount(t *testing.T) {
	// Setup
	var accs []simtypes.Account
	accs = append(accs, simtypes.Account{
		Address: sdk.AccAddress([]byte("pax1qzdrwc3806zfdl98608nqnsvhg8hn854hna5pp")),
	})
	accs = append(accs, simtypes.Account{
		Address: sdk.AccAddress([]byte("pax1jdppe6fnj2q7hjsepty5crxtrryzhuqsrq7tpd")),
	})

	// Test with account present
	addr1 := sdk.AccAddress([]byte("pax1qzdrwc3806zfdl98608nqnsvhg8hn854hna5pp")).String()
	account, found := epochsimulation.FindAccount(accs, addr1)
	require.True(t, found)
	require.Equal(t, sdk.AccAddress([]byte("pax1qzdrwc3806zfdl98608nqnsvhg8hn854hna5pp")), account.Address)

	// Test with account not present
	addr3 := sdk.AccAddress([]byte("address3")).String()
	account, found = epochsimulation.FindAccount(accs, addr3)
	require.False(t, found)
	require.Equal(t, simtypes.Account{}, account)

	// Test with invalid account address
	require.Panics(t, func() { epochsimulation.FindAccount(accs, "invalid") }, "The function did not panic with an invalid account address")
}

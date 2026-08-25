package feegrant_test

import (
	"testing"

	"github.com/stretchr/testify/require"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/feegrant"
)

func TestMarshalAndUnmarshalFeegrantKey(t *testing.T) {
	grantee, err := sdk.AccAddressFromBech32("pax1rs8v2232uv5nw8c88ruvyjy08mmxfx25sl0l5k")
	require.NoError(t, err)
	granter, err := sdk.AccAddressFromBech32("pax1l976cvcndrr6hnuyzn93azaxx8sc2xre9m095t")
	require.NoError(t, err)

	key := feegrant.FeeAllowanceKey(granter, grantee)
	require.Len(t, key, len(grantee.Bytes())+len(granter.Bytes())+3)
	require.Equal(t, feegrant.FeeAllowancePrefixByGrantee(grantee), key[:len(grantee.Bytes())+2])

	g1, g2 := feegrant.ParseAddressesFromFeeAllowanceKey(key)
	require.Equal(t, granter, g1)
	require.Equal(t, grantee, g2)
}

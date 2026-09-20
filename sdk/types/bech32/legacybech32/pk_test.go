//go:build ledger || test_ledger_mock

package legacybech32

import (
	"testing"

	"github.com/stretchr/testify/require"

	"github.com/sidiora-labs/paxeer-network/sdk/crypto/hd"
	"github.com/sidiora-labs/paxeer-network/sdk/crypto/ledger"
	"github.com/sidiora-labs/paxeer-network/sdk/testutil/testdata"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func TestBeach32ifPbKey(t *testing.T) {
	require := require.New(t)
	path := *hd.NewFundraiserParams(0, sdk.CoinType, 0)
	priv, err := ledger.NewPrivKeySecp256k1Unsafe(path)
	require.Nil(err, "%s", err)
	require.NotNil(priv)

	pubKeyAddr, err := MarshalPubKey(AccPK, priv.PubKey())
	require.NoError(err)
	require.Equal("paxpub1addwnpepqd87l8xhcnrrtzxnkql7k55ph8fr9jarf4hn6udwukfprlalu8lgw4ln7qg",
		pubKeyAddr, "Is your device using test mnemonic: %s ?", testdata.TestMnemonic)
}

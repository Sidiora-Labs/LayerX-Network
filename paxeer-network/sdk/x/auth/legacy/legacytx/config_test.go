package legacytx_test

import (
	"testing"

	"github.com/stretchr/testify/suite"

	"github.com/sidiora-labs/paxeer-network/sdk/codec"
	cryptoAmino "github.com/sidiora-labs/paxeer-network/sdk/crypto/codec"
	"github.com/sidiora-labs/paxeer-network/sdk/testutil/testdata"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/auth/legacy/legacytx"
	"github.com/sidiora-labs/paxeer-network/sdk/x/auth/testutil"
)

func testCodec() *codec.LegacyAmino {
	cdc := codec.NewLegacyAmino()
	sdk.RegisterLegacyAminoCodec(cdc)
	cryptoAmino.RegisterCrypto(cdc)
	cdc.RegisterConcrete(&testdata.TestMsg{}, "cosmos-sdk/Test", nil)
	return cdc
}

func TestStdTxConfig(t *testing.T) {
	cdc := testCodec()
	txGen := legacytx.StdTxConfig{Cdc: cdc}
	suite.Run(t, testutil.NewTxConfigTestSuite(txGen))
}

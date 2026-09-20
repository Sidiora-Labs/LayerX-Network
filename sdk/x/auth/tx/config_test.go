package tx

import (
	"testing"

	"github.com/stretchr/testify/suite"

	"github.com/sidiora-labs/paxeer-network/sdk/codec"
	codectypes "github.com/sidiora-labs/paxeer-network/sdk/codec/types"
	"github.com/sidiora-labs/paxeer-network/sdk/std"
	"github.com/sidiora-labs/paxeer-network/sdk/testutil/testdata"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/auth/testutil"
)

func TestGenerator(t *testing.T) {
	interfaceRegistry := codectypes.NewInterfaceRegistry()
	std.RegisterInterfaces(interfaceRegistry)
	interfaceRegistry.RegisterImplementations((*sdk.Msg)(nil), &testdata.TestMsg{})
	protoCodec := codec.NewProtoCodec(interfaceRegistry)
	suite.Run(t, testutil.NewTxConfigTestSuite(NewTxConfig(protoCodec, DefaultSignModes)))
}

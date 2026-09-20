package cli_test

import (
	"context"
	"testing"

	"github.com/sidiora-labs/paxeer-network/node"
	"github.com/spf13/pflag"

	"github.com/sidiora-labs/paxeer-network/sdk/crypto/keys/secp256k1"
	"github.com/sidiora-labs/paxeer-network/sdk/testutil"
	"github.com/sidiora-labs/paxeer-network/sdk/testutil/testdata"
	"github.com/sidiora-labs/paxeer-network/sdk/x/distribution/client/cli"

	"github.com/stretchr/testify/require"

	"github.com/stretchr/testify/assert"

	"github.com/sidiora-labs/paxeer-network/sdk/client"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func Test_splitAndCall_NoMessages(t *testing.T) {
	clientCtx := client.Context{}

	err := cli.NewSplitAndApply(t.Context(), nil, clientCtx, nil, nil, 10)
	assert.NoError(t, err, "")
}

func Test_splitAndCall_Splitting(t *testing.T) {
	clientCtx := client.Context{}

	addr := sdk.AccAddress(secp256k1.GenPrivKey().PubKey().Address())

	// Add five messages
	msgs := []sdk.Msg{
		testdata.NewTestMsg(addr),
		testdata.NewTestMsg(addr),
		testdata.NewTestMsg(addr),
		testdata.NewTestMsg(addr),
		testdata.NewTestMsg(addr),
	}

	// Keep track of number of calls
	const chunkSize = 2

	callCount := 0
	err := cli.NewSplitAndApply(
		t.Context(),
		func(_ context.Context, clientCtx client.Context, fs *pflag.FlagSet, msgs ...sdk.Msg) error {
			callCount++

			assert.NotNil(t, clientCtx)
			assert.NotNil(t, msgs)

			if callCount < 3 {
				assert.Equal(t, len(msgs), 2)
			} else {
				assert.Equal(t, len(msgs), 1)
			}

			return nil
		},
		clientCtx, nil, msgs, chunkSize)

	assert.NoError(t, err, "")
	assert.Equal(t, 3, callCount)
}

func TestParseProposal(t *testing.T) {
	encodingConfig := app.MakeEncodingConfig()

	okJSON := testutil.WriteToNewTempFile(t, `
{
  "title": "Community Pool Spend",
  "description": "Pay me some Atoms!",
  "recipient": "cosmos1s5afhd6gxevu37mkqcvvsj8qeylhn0rz46zdlq",
  "amount": "1000uhpx",
  "deposit": "1000uhpx"
}
`)

	proposal, err := cli.ParseCommunityPoolSpendProposalWithDeposit(encodingConfig.Marshaler, okJSON.Name())
	require.NoError(t, err)

	require.Equal(t, "Community Pool Spend", proposal.Title)
	require.Equal(t, "Pay me some Atoms!", proposal.Description)
	require.Equal(t, "cosmos1s5afhd6gxevu37mkqcvvsj8qeylhn0rz46zdlq", proposal.Recipient)
	require.Equal(t, "1000uhpx", proposal.Deposit)
	require.Equal(t, "1000uhpx", proposal.Amount)
}

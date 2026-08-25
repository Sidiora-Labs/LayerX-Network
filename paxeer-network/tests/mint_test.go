package tests

import (
	"testing"
	"time"

	"github.com/sidiora-labs/paxeer-network/sdk/x/auth/signing"
	"github.com/sidiora-labs/paxeer-network/testutil/processblock"
	"github.com/sidiora-labs/paxeer-network/testutil/processblock/verify"
)

func TestMint(t *testing.T) {
	app := processblock.NewTestApp(t)
	_ = processblock.CommonPreset(app)
	app.NewMinter(1000000)
	app.FastEpoch()
	for i, testCase := range []TestCase{
		{
			description: "first epoch",
			input:       []signing.Tx{},
			verifier: []verify.Verifier{
				verify.MintRelease,
			},
			expectedCodes: []uint32{},
		},
		{
			description: "second epoch",
			input:       []signing.Tx{},
			verifier: []verify.Verifier{
				verify.MintRelease,
			},
			expectedCodes: []uint32{},
		},
	} {
		if i > 0 {
			time.Sleep(6 * time.Second)
		}
		testCase.run(t, app)
	}
}

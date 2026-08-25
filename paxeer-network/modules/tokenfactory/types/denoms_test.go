package types_test

import (
	"testing"

	"github.com/stretchr/testify/require"

	"github.com/sidiora-labs/paxeer-network/modules/tokenfactory/types"
	appparams "github.com/sidiora-labs/paxeer-network/node/params"
)

func TestDecomposeDenoms(t *testing.T) {
	appparams.SetAddressPrefixes()
	for _, tc := range []struct {
		desc  string
		denom string
		valid bool
	}{
		{
			desc:  "empty is invalid",
			denom: "",
			valid: false,
		},
		{
			desc:  "normal",
			denom: "factory/pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288/bitcoin",
			valid: true,
		},
		{
			desc:  "multiple slashes in subdenom",
			denom: "factory/pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288/bitcoin/1",
			valid: true,
		},
		{
			desc:  "no subdenom",
			denom: "factory/pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288/",
			valid: true,
		},
		{
			desc:  "incorrect prefix",
			denom: "ibc/pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288/bitcoin",
			valid: false,
		},
		{
			desc:  "subdenom of only slashes",
			denom: "factory/pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288/////",
			valid: true,
		},
		{
			desc:  "too long name",
			denom: "factory/pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288/adsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsf",
			valid: false,
		},
		{
			desc:  "too long creator name",
			denom: "factory/pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288asdfasdfasdfasdfasdfasdfadfasdfasdfasdfasdfasdfas/bitcoin",
			valid: false,
		},
		{
			desc:  "empty subdenom",
			denom: "factory/pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288/",
			valid: true,
		},
	} {
		t.Run(tc.desc, func(t *testing.T) {
			_, _, err := types.DeconstructDenom(tc.denom)
			if tc.valid {
				require.NoError(t, err)
			} else {
				require.Error(t, err)
			}
		})
	}
}

func TestGetTokenDenom(t *testing.T) {
	for _, tc := range []struct {
		desc     string
		creator  string
		subdenom string
		valid    bool
	}{
		{
			desc:     "normal",
			creator:  "pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288",
			subdenom: "bitcoin",
			valid:    true,
		},
		{
			desc:     "multiple slashes in subdenom",
			creator:  "pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288",
			subdenom: "bitcoin/1",
			valid:    true,
		},
		{
			desc:     "no subdenom",
			creator:  "pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288",
			subdenom: "",
			valid:    true,
		},
		{
			desc:     "subdenom of only slashes",
			creator:  "pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288",
			subdenom: "/////",
			valid:    true,
		},
		{
			desc:     "too long name",
			creator:  "pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288",
			subdenom: "adsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsfadsf",
			valid:    false,
		},
		{
			desc:     "subdenom is exactly max length",
			creator:  "pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288",
			subdenom: "bitcoinfsadfsdfeadfsafwefsefsefsdfsdafasefsf",
			valid:    true,
		},
		{
			desc:     "creator is exactly max length",
			creator:  "pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288hjkljkljkljkljkljkljkljkljkljkljk",
			subdenom: "bitcoin",
			valid:    true,
		},
		{
			desc:     "empty subdenom",
			creator:  "pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288",
			subdenom: "",
			valid:    true,
		},
		{
			desc:     "non standard UTF-8",
			creator:  "pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288",
			subdenom: "\u2603",
			valid:    false,
		},
		{
			desc:     "non standard ASCII",
			creator:  "pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288",
			subdenom: "\n\t",
			valid:    false,
		},
	} {
		t.Run(tc.desc, func(t *testing.T) {
			_, err := types.GetTokenDenom(tc.creator, tc.subdenom)
			if tc.valid {
				require.NoError(t, err)
			} else {
				require.Error(t, err)
			}
		})
	}
}

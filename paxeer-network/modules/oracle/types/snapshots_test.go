package types

import (
	"testing"

	"github.com/sidiora-labs/paxeer-network/modules/oracle/utils"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/stretchr/testify/require"
)

func TestNewPriceSnapshotItem(t *testing.T) {
	item := NewPriceSnapshotItem(utils.MicroAtomDenom, OracleExchangeRate{
		ExchangeRate: sdk.NewDec(11),
		LastUpdate:   sdk.NewInt(20),
	})

	expected := PriceSnapshotItem{
		Denom: utils.MicroAtomDenom,
		OracleExchangeRate: OracleExchangeRate{
			ExchangeRate: sdk.NewDec(11),
			LastUpdate:   sdk.NewInt(20),
		},
	}

	require.Equal(t, expected, item)
}

func TestNewPriceSnapshot(t *testing.T) {
	snapshot := NewPriceSnapshot([]PriceSnapshotItem{
		NewPriceSnapshotItem(utils.MicroEthDenom, OracleExchangeRate{
			ExchangeRate: sdk.NewDec(11),
			LastUpdate:   sdk.NewInt(20),
		}),
		NewPriceSnapshotItem(utils.MicroAtomDenom, OracleExchangeRate{
			ExchangeRate: sdk.NewDec(12),
			LastUpdate:   sdk.NewInt(20),
		}),
	}, 1)

	expected := PriceSnapshot{
		SnapshotTimestamp: 1,
		PriceSnapshotItems: []PriceSnapshotItem{
			{
				Denom: utils.MicroEthDenom,
				OracleExchangeRate: OracleExchangeRate{
					ExchangeRate: sdk.NewDec(11),
					LastUpdate:   sdk.NewInt(20),
				},
			},
			{
				Denom: utils.MicroAtomDenom,
				OracleExchangeRate: OracleExchangeRate{
					ExchangeRate: sdk.NewDec(12),
					LastUpdate:   sdk.NewInt(20),
				},
			},
		},
	}

	require.Equal(t, expected, snapshot)
}

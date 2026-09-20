package utils

import (
	"testing"

	cryptotypes "github.com/sidiora-labs/paxeer-network/sdk/crypto/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/types/tx/signing"
	banktypes "github.com/sidiora-labs/paxeer-network/sdk/x/bank/types"
	"github.com/stretchr/testify/require"

	oracletypes "github.com/sidiora-labs/paxeer-network/modules/oracle/types"
)

func TestIsTxPrioritized(t *testing.T) {
	tests := []struct {
		name     string
		tx       sdk.Tx
		expected bool
	}{
		{
			name:     "Empty transaction",
			tx:       createTestTx([]sdk.Msg{}),
			expected: true,
		},
		{
			name: "Oracle aggregate vote message",
			tx: createTestTx([]sdk.Msg{
				&oracletypes.MsgAggregateExchangeRateVote{
					ExchangeRates: "1.0uhpx,2.0uusd",
					Feeder:        "pax1abc123",
					Validator:     "paxvaloper1abc123",
				},
			}),
			expected: true,
		},
		{
			name: "Oracle delegate feed consent message",
			tx: createTestTx([]sdk.Msg{
				&oracletypes.MsgDelegateFeedConsent{
					Operator: "paxvaloper1abc123",
					Delegate: "pax1abc123",
				},
			}),
			expected: true,
		},
		{
			name: "Multiple oracle messages",
			tx: createTestTx([]sdk.Msg{
				&oracletypes.MsgAggregateExchangeRateVote{
					ExchangeRates: "1.0uhpx",
					Feeder:        "pax1abc123",
					Validator:     "paxvaloper1abc123",
				},
				&oracletypes.MsgDelegateFeedConsent{
					Operator: "paxvaloper1abc123",
					Delegate: "pax1abc123",
				},
			}),
			expected: true,
		},
		{
			name: "Bank send message (not prioritized)",
			tx: createTestTx([]sdk.Msg{
				&banktypes.MsgSend{
					FromAddress: "pax1abc123",
					ToAddress:   "pax1def456",
					Amount:      sdk.NewCoins(sdk.NewInt64Coin("uhpx", 100)),
				},
			}),
			expected: false,
		},
		{
			name: "Mixed messages (oracle + bank)",
			tx: createTestTx([]sdk.Msg{
				&oracletypes.MsgAggregateExchangeRateVote{
					ExchangeRates: "1.0uhpx",
					Feeder:        "pax1abc123",
					Validator:     "paxvaloper1abc123",
				},
				&banktypes.MsgSend{
					FromAddress: "pax1abc123",
					ToAddress:   "pax1def456",
					Amount:      sdk.NewCoins(sdk.NewInt64Coin("uhpx", 100)),
				},
			}),
			expected: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := IsTxPrioritized(tt.tx)
			require.Equal(t, tt.expected, result)
		})
	}
}

func TestIsTxPrioritizedEdgeCases(t *testing.T) {
	// Test with transaction containing no messages
	emptyTx := createTestTx([]sdk.Msg{})
	require.True(t, IsTxPrioritized(emptyTx))
}

// Helper function to create a test transaction with given messages
func createTestTx(msgs []sdk.Msg) sdk.Tx {
	return &TestTx{msgs: msgs}
}

// TestTx is a simple implementation of sdk.Tx for testing
type TestTx struct {
	msgs []sdk.Msg
}

func (tx *TestTx) GetMsgs() []sdk.Msg {
	return tx.msgs
}

func (tx *TestTx) ValidateBasic() error {
	return nil
}

func (tx *TestTx) GetSigners() []sdk.AccAddress {
	return nil
}

func (tx *TestTx) GetPubKeys() ([]cryptotypes.PubKey, error) {
	return nil, nil
}

func (tx *TestTx) GetSignaturesV2() ([]signing.SignatureV2, error) {
	return nil, nil
}

func (tx *TestTx) GetGasEstimate() uint64 {
	return 0
}

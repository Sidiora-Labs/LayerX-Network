package types

import (
	"testing"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/stretchr/testify/require"
)

func TestPaxAddressHandler_GetPaxAddressFromString(t *testing.T) {
	type args struct {
		address string
	}
	tests := []struct {
		name       string
		args       args
		want       sdk.AccAddress
		wantErr    bool
		wantErrMsg string
	}{
		{
			name: "returns address if address is valid",
			args: args{
				address: sdk.MustAccAddressFromBech32("pax1k73v0ec39mxdgr3wnayy3x4t5w7pdncaz96t92").String(),
			},
			want: sdk.MustAccAddressFromBech32("pax1k73v0ec39mxdgr3wnayy3x4t5w7pdncaz96t92"),
		},
		{
			name: "returns error if address is invalid",
			args: args{
				address: "invalid",
			},
			wantErr:    true,
			wantErrMsg: "decoding bech32 failed: invalid bech32 string length 7",
		}, {
			name: "returns error if address is empty",
			args: args{
				address: "",
			},
			wantErr:    true,
			wantErrMsg: "empty address string is not allowed",
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			h := PaxAddressHandler{}
			got, err := h.GetPaxAddressFromString(sdk.Context{}, tt.args.address)
			if tt.wantErr {
				require.NotNil(t, err)
				require.Equal(t, tt.wantErrMsg, err.Error())
				return
			} else {
				require.NoError(t, err)
				require.Equal(t, tt.want, got)
			}
		})
	}
}

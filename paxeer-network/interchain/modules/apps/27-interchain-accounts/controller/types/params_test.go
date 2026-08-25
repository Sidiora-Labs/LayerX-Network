package types_test

import (
	"testing"

	"github.com/stretchr/testify/require"

	"github.com/sidiora-labs/paxeer-network/interchain/modules/apps/27-interchain-accounts/controller/types"
)

func TestValidateParams(t *testing.T) {
	require.NoError(t, types.DefaultParams().Validate())
	require.NoError(t, types.NewParams(false).Validate())
}

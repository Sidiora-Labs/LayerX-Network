package utils_test

import (
	"errors"
	"testing"

	"github.com/sidiora-labs/paxeer-network/utils"
	"github.com/stretchr/testify/require"
)

func TestHardFail(t *testing.T) {
	hardFailer := func() {
		panic(utils.DecorateHardFailError(errors.New("some error")))
	}
	panicHandlingFn := func() {
		defer utils.PanicHandler(func(_ any) {})()
		hardFailer()
	}
	require.Panics(t, panicHandlingFn)
}

package utils

import (
	"fmt"
	"runtime/debug"
	"strings"

	"github.com/paxeer-network/paxlog"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

var logger = paxlog.NewLogger("utils")

const HardFailPrefix = "hard fail error occurred"

func PanicHandler(recoverCallback func(any)) func() {
	return func() {
		if err := recover(); err != nil {
			if shouldErrorHardFail(fmt.Sprintf("%s", err)) {
				panic(err)
			}
			recoverCallback(err)
		}
	}
}

// LogPanicCallback returns a callback function, given a context and a recovered
// error value, that logs the error and a stack trace.
func LogPanicCallback(ctx sdk.Context, r any) func(any) {
	return func(a any) {
		stackTrace := string(debug.Stack())
		logger.Error("recovered panic", "recover_err", r, "recover_type", fmt.Sprintf("%T", r), "stack_trace", stackTrace)
	}
}

func DecorateHardFailError(err error) error {
	return fmt.Errorf("%s:%s", HardFailPrefix, err.Error())
}

func shouldErrorHardFail(err string) bool {
	// use Contains instead of HasPrefix in case the error is further decorated
	return strings.Contains(err, HardFailPrefix)
}

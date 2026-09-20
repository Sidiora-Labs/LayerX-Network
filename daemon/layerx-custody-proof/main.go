package main

import (
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"time"

	"github.com/sidiora-labs/paxeer-network/custodyproof"
)

func run(output io.Writer) error {
	flags := flag.NewFlagSet("layerx-custody-proof", flag.ContinueOnError)
	state := flags.String("history-state", "", "protected authenticated history directory")
	key := flags.String("attestor-key", "", "protected attestor seed file")
	if err := flags.Parse(os.Args[1:]); err != nil {
		return err
	}
	if flags.NArg() != 0 || (*state == "") != (*key == "") {
		return fmt.Errorf("history state and authority must be supplied together")
	}
	input, err := io.ReadAll(io.LimitReader(os.Stdin, custodyproof.MaxInputBytes+1))
	if err != nil {
		return err
	}
	if len(input) == 0 || len(input) > custodyproof.MaxInputBytes {
		return fmt.Errorf("proof request size")
	}
	request, err := custodyproof.Decode(input)
	if err != nil {
		return err
	}
	now := time.Now().UTC()
	var result any
	if *state == "" {
		if request.Operation != "" && request.Operation != "verify" {
			return fmt.Errorf("history operation requires durable state")
		}
		result, err = custodyproof.Verify(request, now)
	} else {
		history, openErr := custodyproof.OpenHistory(*state, *key, request.Expected, request.Bundle.Genesis, now)
		if openErr != nil {
			return openErr
		}
		switch request.Operation {
		case "status":
			result, err = history.Status(now)
		case "advance":
			err = history.Advance(request.Bundle.History, now)
			if err == nil {
				result, err = history.Status(now)
			}
		case "verify":
			result, err = history.Verify(request, now)
		case "export":
			result, err = history.Export()
		default:
			err = fmt.Errorf("unsupported history operation")
		}
		err = errors.Join(err, history.Close())
	}
	if err != nil {
		return err
	}
	return json.NewEncoder(output).Encode(result)
}

func main() {
	output := os.Stdout
	os.Stdout = os.Stderr
	if err := run(output); err != nil {
		fmt.Fprintln(os.Stderr, "custody proof refused:", err)
		os.Exit(1)
	}
}

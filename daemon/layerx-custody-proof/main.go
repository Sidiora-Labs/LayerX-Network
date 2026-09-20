package main

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"time"

	"github.com/sidiora-labs/paxeer-network/custodyproof"
)

func run(output io.Writer) error {
	if len(os.Args) > 1 {
		switch os.Args[1] {
		case "light-profile":
			return lightProfile(os.Args[2:])
		case "light-credit":
			return lightCredit(os.Args[2:])
		}
	}
	if len(os.Args) != 1 {
		return fmt.Errorf("usage: layerx-custody-proof [light-profile|light-credit] (a JSON verify request on stdin otherwise)")
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
	if request.Operation != "" && request.Operation != "verify" {
		return fmt.Errorf("unsupported operation")
	}
	result, err := custodyproof.Verify(request, time.Now().UTC())
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

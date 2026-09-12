package main

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"time"

	"github.com/sidiora-labs/paxeer-network/custodyproof"
)

func run() error {
	if len(os.Args) != 1 { return fmt.Errorf("usage: layerx-custody-proof < request.json") }
	input, err := io.ReadAll(io.LimitReader(os.Stdin, custodyproof.MaxInputBytes+1))
	if err != nil { return err }
	if len(input) == 0 || len(input) > custodyproof.MaxInputBytes { return fmt.Errorf("proof request size") }
	var request custodyproof.Request
	if err := json.Unmarshal(input, &request); err != nil { return err }
	result, err := custodyproof.Verify(&request, time.Now().UTC())
	if err != nil { return err }
	return json.NewEncoder(os.Stdout).Encode(result)
}

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, "custody proof refused:", err)
		os.Exit(1)
	}
}

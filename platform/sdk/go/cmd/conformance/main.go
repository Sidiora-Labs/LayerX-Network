package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	layerx "github.com/Sidiora-Labs/LayerX-Network/platform/sdk/go"
)

type requestVector struct {
	Method  string                     `json:"method"`
	Path    string                     `json:"path"`
	Headers map[string]string          `json:"headers"`
	Body    map[string]json.RawMessage `json:"body"`
}

type responseVector struct {
	Status int `json:"status"`
	Body   struct {
		OK     bool            `json:"ok"`
		Result json.RawMessage `json:"result"`
		Error  *struct {
			Code  layerx.HumanErrorCode `json:"code"`
			Retry string                `json:"retry"`
		} `json:"error"`
		Trace string `json:"trace"`
	} `json:"body"`
}

func main() {
	repo := flag.String("repo", "../../..", "LayerX repository root")
	flag.Parse()
	if err := verifyHumanVectors(*repo); err != nil {
		fmt.Fprintln(os.Stderr, "layerx-go-conformance:", err)
		os.Exit(1)
	}
	metadata := layerx.PlatformSDKGo()
	fmt.Printf("agent_operations=%d human_operations=%d human_golden_triplets=%d\n", metadata.AgentOperations, metadata.HumanOperations, metadata.HumanOperations)
}

func verifyHumanVectors(repo string) error {
	goldenRoot := filepath.Join(repo, "human", "schema", "human-api", "golden")
	for _, operation := range layerx.AllHumanOperations() {
		metadata, ok := operation.Metadata()
		if !ok {
			return fmt.Errorf("generated metadata missing for %s", operation)
		}
		prefix := filepath.Join(goldenRoot, string(operation))
		request, err := readJSON[requestVector](prefix + ".request.json")
		if err != nil {
			return err
		}
		if request.Method != metadata.Method || !pathMatches(metadata.Path, request.Path) {
			return fmt.Errorf("request vector for %s diverges from generated method/path", operation)
		}
		if operation.RequiresIdempotency() && request.Headers["Idempotency-Key"] == "" {
			return fmt.Errorf("request vector for %s omits Idempotency-Key", operation)
		}
		for name, value := range request.Body {
			if err := verifyMoneyEncoding(name, value); err != nil {
				return fmt.Errorf("request vector for %s: %w", operation, err)
			}
		}
		success, err := readJSON[responseVector](prefix + ".response.json")
		if err != nil {
			return err
		}
		if success.Status < 200 || success.Status >= 300 || !success.Body.OK || success.Body.Trace == "" || len(success.Body.Result) == 0 || success.Body.Error != nil {
			return fmt.Errorf("success vector for %s is not a typed success envelope", operation)
		}
		if err := verifyMoneyEncoding("result", success.Body.Result); err != nil {
			return fmt.Errorf("success vector for %s: %w", operation, err)
		}
		failure, err := readJSON[responseVector](prefix + ".failure.json")
		if err != nil {
			return err
		}
		if failure.Status >= 200 && failure.Status < 300 || failure.Body.OK || failure.Body.Trace == "" || failure.Body.Error == nil || !failure.Body.Error.Code.Valid() {
			return fmt.Errorf("failure vector for %s is not a typed failure envelope", operation)
		}
		switch failure.Body.Error.Retry {
		case "retriable", "retriable-after", "structural", "final":
		default:
			return fmt.Errorf("failure vector for %s has unknown retry class", operation)
		}
	}
	return nil
}

func verifyMoneyEncoding(name string, encoded json.RawMessage) error {
	var value any
	decoder := json.NewDecoder(strings.NewReader(string(encoded)))
	decoder.UseNumber()
	if err := decoder.Decode(&value); err != nil {
		return err
	}
	return walkMoney(name, value)
}

func walkMoney(name string, value any) error {
	switch typed := value.(type) {
	case map[string]any:
		for childName, child := range typed {
			if err := walkMoney(childName, child); err != nil {
				return err
			}
		}
	case []any:
		for _, child := range typed {
			if err := walkMoney(name, child); err != nil {
				return err
			}
		}
	case json.Number:
		if name == "amount" || name == "amounts" {
			return fmt.Errorf("%s is encoded as a JSON number", name)
		}
	}
	return nil
}

func readJSON[T any](path string) (T, error) {
	var result T
	encoded, err := os.ReadFile(path)
	if err != nil {
		return result, fmt.Errorf("read %s: %w", path, err)
	}
	if err := json.Unmarshal(encoded, &result); err != nil {
		return result, fmt.Errorf("decode %s: %w", path, err)
	}
	return result, nil
}

func pathMatches(template string, actual string) bool {
	templateParts := strings.Split(strings.Trim(template, "/"), "/")
	actualParts := strings.Split(strings.Trim(actual, "/"), "/")
	if len(templateParts) != len(actualParts) {
		return false
	}
	for index, expected := range templateParts {
		if strings.HasPrefix(expected, "{") && strings.HasSuffix(expected, "}") {
			if actualParts[index] == "" {
				return false
			}
			continue
		}
		if expected != actualParts[index] {
			return false
		}
	}
	return true
}

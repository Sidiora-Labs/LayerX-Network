package layerx

import (
	"context"
	"encoding/json"
	"net/url"
)

type Plane string

const (
	PlaneAgent Plane = "agent"
	PlaneHuman Plane = "human"
)

type IdempotencyKey struct{ value string }

func NewIdempotencyKey(value string) (IdempotencyKey, error) {
	if value == "" || len(value) > 255 {
		return IdempotencyKey{}, newSDKError(ErrorInvalidArgument, RetryNever)
	}
	for index := range value {
		if value[index] == 0 {
			return IdempotencyKey{}, newSDKError(ErrorInvalidArgument, RetryNever)
		}
	}
	return IdempotencyKey{value: value}, nil
}

func (key IdempotencyKey) String() string { return key.value }
func (key IdempotencyKey) valid() bool    { return key.value != "" }

type CallOptions struct {
	IdempotencyKey IdempotencyKey
	PathParameters map[string]string
	Query          url.Values
}

type TransportCall struct {
	Plane          Plane
	Operation      string
	Request        json.RawMessage
	IdempotencyKey IdempotencyKey
	PathParameters map[string]string
	Query          url.Values
}

type Transport interface {
	Call(context.Context, TransportCall) (json.RawMessage, error)
}

type TelemetryEvent struct {
	Plane     Plane
	Operation string
	Outcome   string
	Code      ErrorCode
}

type Telemetry func(TelemetryEvent)

type Client struct {
	transport Transport
	telemetry Telemetry
}

func NewClient(transport Transport, telemetry Telemetry) (*Client, error) {
	if transport == nil {
		return nil, newSDKError(ErrorInvalidArgument, RetryNever)
	}
	return &Client{transport: transport, telemetry: telemetry}, nil
}

func (client *Client) Agent(ctx context.Context, operation AgentOperation, request any, response any, options CallOptions) error {
	if !operation.Valid() {
		return newSDKError(ErrorInvalidArgument, RetryNever)
	}
	return client.call(ctx, PlaneAgent, string(operation), operation.RequiresIdempotency(), request, response, options)
}

func (client *Client) Human(ctx context.Context, operation HumanOperation, request any, response any, options CallOptions) error {
	if !operation.Valid() {
		return newSDKError(ErrorInvalidArgument, RetryNever)
	}
	return client.call(ctx, PlaneHuman, string(operation), operation.RequiresIdempotency(), request, response, options)
}

func (client *Client) call(ctx context.Context, plane Plane, operation string, requiresKey bool, request any, response any, options CallOptions) error {
	if client == nil || client.transport == nil || ctx == nil || response == nil {
		return newSDKError(ErrorInvalidArgument, RetryNever)
	}
	if requiresKey && !options.IdempotencyKey.valid() {
		return newSDKError(ErrorIdempotencyRequired, RetryNever)
	}
	encoded, err := json.Marshal(request)
	if err != nil {
		return newSDKError(ErrorInvalidArgument, RetryNever)
	}
	raw, err := client.transport.Call(ctx, TransportCall{
		Plane: plane, Operation: operation, Request: encoded,
		IdempotencyKey: options.IdempotencyKey,
		PathParameters: cloneStrings(options.PathParameters), Query: cloneValues(options.Query),
	})
	if err != nil {
		safe, ok := err.(*SDKError)
		if !ok {
			safe = transportError(ctx, err)
		}
		client.emit(TelemetryEvent{Plane: plane, Operation: operation, Outcome: "refused", Code: safe.Code})
		return safe
	}
	if err := json.Unmarshal(raw, response); err != nil {
		safe := newSDKError(ErrorDecodeFailure, RetryNever)
		client.emit(TelemetryEvent{Plane: plane, Operation: operation, Outcome: "refused", Code: safe.Code})
		return safe
	}
	client.emit(TelemetryEvent{Plane: plane, Operation: operation, Outcome: "completed"})
	return nil
}

func (client *Client) emit(event TelemetryEvent) {
	if client.telemetry != nil {
		client.telemetry(event)
	}
}

func cloneStrings(source map[string]string) map[string]string {
	copy := make(map[string]string, len(source))
	for key, value := range source {
		copy[key] = value
	}
	return copy
}

func cloneValues(source url.Values) url.Values {
	copy := make(url.Values, len(source))
	for key, values := range source {
		copy[key] = append([]string(nil), values...)
	}
	return copy
}

type SDKMetadata struct {
	Module          string
	Version         string
	AgentOperations int
	HumanOperations int
}

func PlatformSDKGo() SDKMetadata {
	return SDKMetadata{
		Module:          "github.com/Sidiora-Labs/LayerX-Network/platform/sdk/go",
		Version:         "0.1.0",
		AgentOperations: len(AllAgentOperations()),
		HumanOperations: len(AllHumanOperations()),
	}
}

func platform_sdk_go() SDKMetadata { return PlatformSDKGo() }

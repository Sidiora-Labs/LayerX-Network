package evmrpc

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"math/big"
	"sync"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/common/hexutil"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
)

type contextKey string

const tendermintTraceKey contextKey = "tendermintTrace"
const receiptTraceKey contextKey = "receiptTrace"

type TendermintTraces struct {
	mu     sync.Mutex
	Traces []TendermintTrace `json:"traces"`
}

func (tt *TendermintTraces) MarshalToJSON() (json.RawMessage, error) {
	if tt == nil {
		return nil, errors.New("Tendermint traces are unavailable")
	}
	tt.mu.Lock()
	defer tt.mu.Unlock()
	bz, err := json.Marshal(tt)
	return bz, err
}

type ReceiptTraces struct {
	mu     sync.Mutex
	Traces []RawResponseReceipt `json:"traces"`
}

func (rt *ReceiptTraces) MarshalToJSON() (json.RawMessage, error) {
	if rt == nil {
		return nil, errors.New("receipt traces are unavailable")
	}
	rt.mu.Lock()
	defer rt.mu.Unlock()
	bz, err := json.Marshal(rt)
	return bz, err
}

type RawResponseReceipt struct {
	BlockNumber       hexutil.Uint64  `json:"blockNumber"`
	ContractAddress   *common.Address `json:"contractAddress"`
	CumulativeGasUsed hexutil.Uint64  `json:"cumulativeGasUsed"`
	EffectiveGasPrice *hexutil.Big    `json:"effectiveGasPrice"`
	From              common.Address  `json:"from"`
	To                *common.Address `json:"to"`
	GasUsed           hexutil.Uint64  `json:"gasUsed"`
	Status            hexutil.Uint    `json:"status"`
	Type              hexutil.Uint    `json:"type"`
	TransactionHash   common.Hash     `json:"transactionHash"`
	TransactionIndex  hexutil.Uint64  `json:"transactionIndex"`
}

type TendermintTrace struct {
	Endpoint  string          `json:"endpoint"`
	Arguments []string        `json:"arguments"`
	Response  json.RawMessage `json:"response"`
}

func WithTendermintTraces(ctx context.Context, traces *TendermintTraces) context.Context {
	return context.WithValue(ctx, tendermintTraceKey, traces)
}

func TraceTendermintIfApplicable(ctx context.Context, endpoint string, arguments []string, response interface{}) error {
	existing, ok := ctx.Value(tendermintTraceKey).(*TendermintTraces)
	if !ok || existing == nil {
		return nil
	}
	encodedResponse, err := json.Marshal(response)
	if err != nil {
		return fmt.Errorf("encode Tendermint trace %s: %w", endpoint, err)
	}
	trace := TendermintTrace{
		Endpoint:  endpoint,
		Arguments: arguments,
		Response:  encodedResponse,
	}
	existing.mu.Lock()
	existing.Traces = append(existing.Traces, trace)
	existing.mu.Unlock()
	return nil
}

func TendermintTracesFromContext(ctx context.Context) *TendermintTraces {
	v := ctx.Value(tendermintTraceKey)
	if v == nil {
		return nil
	}
	traces, _ := v.(*TendermintTraces)
	return traces
}

func WithReceiptTraces(ctx context.Context, traces *ReceiptTraces) context.Context {
	return context.WithValue(ctx, receiptTraceKey, traces)
}

func TraceReceiptIfApplicable(ctx context.Context, receipt *types.Receipt) {
	if receipt == nil {
		return
	}
	rrr := &RawResponseReceipt{
		BlockNumber:       hexutil.Uint64(receipt.BlockNumber),
		CumulativeGasUsed: hexutil.Uint64(receipt.CumulativeGasUsed),
		EffectiveGasPrice: (*hexutil.Big)(new(big.Int).SetUint64(receipt.EffectiveGasPrice)),
		From:              common.HexToAddress(receipt.From),
		GasUsed:           hexutil.Uint64(receipt.GasUsed),
		Status:            hexutil.Uint(receipt.Status),
		Type:              hexutil.Uint(receipt.TxType),
		TransactionHash:   common.HexToHash(receipt.TxHashHex),
		TransactionIndex:  hexutil.Uint64(receipt.TransactionIndex),
	}
	if receipt.ContractAddress != "" {
		ca := common.HexToAddress(receipt.ContractAddress)
		rrr.ContractAddress = &ca
	}
	if receipt.To != "" {
		to := common.HexToAddress(receipt.To)
		rrr.To = &to
	}
	existing, ok := ctx.Value(receiptTraceKey).(*ReceiptTraces)
	if !ok || existing == nil {
		return
	}
	existing.mu.Lock()
	existing.Traces = append(existing.Traces, *rrr)
	existing.mu.Unlock()
}

func stringifyInt64Ptr(i *int64) string {
	if i == nil {
		return ""
	}
	return fmt.Sprintf("%d", *i)
}

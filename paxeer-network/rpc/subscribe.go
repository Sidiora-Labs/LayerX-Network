package evmrpc

import (
	"context"
	"errors"
	"fmt"
	"math"
	"math/big"
	"sync"
	"time"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/common/hexutil"
	ethtypes "github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/eth/filters"
	"github.com/ethereum/go-ethereum/rpc"
	"github.com/sidiora-labs/paxeer-network/consensus/rpc/coretypes"
	tmtypes "github.com/sidiora-labs/paxeer-network/consensus/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	evmtypes "github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/sdk/client"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/utils"
)

const SleepInterval = 5 * time.Second
const NewHeadsListenerBuffer = 10

type SubscriptionAPI struct {
	tmClient            client.LocalClient
	subscriptionManager *SubscriptionManager
	subscriptonConfig   *SubscriptionConfig

	logFetcher          *LogFetcher
	newHeadListenersMtx *sync.RWMutex
	newHeadListeners    map[rpc.ID]chan map[string]interface{}
	connectionType      ConnectionType
}

type SubscriptionConfig struct {
	subscriptionCapacity int
	newHeadLimit         uint64
}

func NewSubscriptionAPI(tmClient client.LocalClient, k *keeper.Keeper, ctxProvider func(int64) sdk.Context, logFetcher *LogFetcher, subscriptionConfig *SubscriptionConfig, filterConfig *FilterConfig, connectionType ConnectionType, blockHeaderNotifier *BlockHeaderNotifier) (*SubscriptionAPI, error) {
	if k == nil || ctxProvider == nil || logFetcher == nil || subscriptionConfig == nil || filterConfig == nil {
		return nil, errors.New("subscription API dependencies are not configured")
	}
	if blockHeaderNotifier == nil && tmClient == nil {
		return nil, errors.New("subscription API requires a consensus client or block-header notifier")
	}
	logFetcher.filterConfig = filterConfig
	api := &SubscriptionAPI{
		tmClient:            tmClient,
		subscriptonConfig:   subscriptionConfig,
		logFetcher:          logFetcher,
		newHeadListenersMtx: &sync.RWMutex{},
		newHeadListeners:    make(map[rpc.ID]chan map[string]interface{}),
		connectionType:      connectionType,
		// subscriptionManager is only constructed for the legacy
		// event-bus path below; under Autobahn the notifier feeds the
		// fan-out directly and the manager is unused.
	}
	if blockHeaderNotifier != nil {
		// Autobahn (and any future direct-channel) path. The producer
		// pushes one event per committed block; there is no Tendermint
		// event-bus subscription.
		go api.runNewHeadsFromNotifier(blockHeaderNotifier, k, ctxProvider)
	} else {
		// Legacy CometBFT path: subscribe to the Tendermint event bus.
		api.subscriptionManager = NewSubscriptionManager(tmClient)
		id, subCh, err := api.subscriptionManager.Subscribe(context.Background(), NewHeadQueryBuilder(), api.subscriptonConfig.subscriptionCapacity)
		if err != nil {
			return nil, fmt.Errorf("subscribe to consensus new-head events: %w", err)
		}
		go func() {
			defer recoverAndLog()
			defer func() {
				_ = api.subscriptionManager.Unsubscribe(context.Background(), id)
			}()
			for res := range subCh {
				eventHeader, ok := res.Data.(tmtypes.EventDataNewBlockHeader)
				if !ok {
					fmt.Printf("dropping malformed newHeads event of type %T\n", res.Data)
					continue
				}
				ctx := ctxProvider(eventHeader.Header.Height)
				baseFeePerGas := k.GetNextBaseFeePerGas(ctx).TruncateInt().BigInt()
				cp := ctx.ConsensusParams()
				if cp == nil || cp.Block == nil {
					fmt.Printf("dropping newHeads event at height %d: consensus block parameters are unavailable\n", eventHeader.Header.Height)
					continue
				}
				gasLimit := cp.Block.MaxGas
				ethHeader, err := encodeTmHeader(eventHeader, baseFeePerGas, gasLimit)
				if err != nil {
					fmt.Printf("error encoding new head event %#v due to %s\n", res.Data, err)
					continue
				}
				api.broadcastNewHead(ethHeader)
			}
		}()
	}
	return api, nil
}

func (a *SubscriptionAPI) runNewHeadsFromNotifier(notifier *BlockHeaderNotifier, k *keeper.Keeper, ctxProvider func(int64) sdk.Context) {
	defer recoverAndLog()
	for evt := range notifier.recv() {
		// Defend against a misbehaving producer. OnBlockCommitted's
		// contract requires non-nil header/response, but a single bad
		// event must not kill the fan-out goroutine for all subscribers.
		if evt.header == nil || evt.response == nil {
			fmt.Printf("dropping malformed newHeads event: header=%v response=%v\n", evt.header, evt.response)
			continue
		}
		ctx := ctxProvider(evt.header.Height)
		baseFeePerGas := pickHeadBaseFee(k.GetNextBaseFeePerGas, ctxProvider, evt.header.Height)
		// Source gasLimit from the active SDK ConsensusParams rather than
		// evt.response.ConsensusParamUpdates: the latter is only populated
		// on actual updates (nil for nearly every block). See block.go's
		// GetBlockByNumber for the same pattern + rationale.
		cp := ctx.ConsensusParams()
		if cp == nil || cp.Block == nil {
			fmt.Printf("dropping newHeads event at height %d: consensus block parameters are unavailable\n", evt.header.Height)
			continue
		}
		gasLimit := cp.Block.MaxGas
		ethHeader, err := encodeCommittedBlock(evt, baseFeePerGas, gasLimit)
		if err != nil {
			fmt.Printf("dropping invalid newHeads event at height %d: %v\n", evt.header.Height, err)
			continue
		}
		a.broadcastNewHead(ethHeader)
	}
}

// pickHeadBaseFee returns the baseFeePerGas to attach to the eth_newHeads
// notification for the block at `height`. Mirrors block.go's
// GetBlockByNumber: GetNextBaseFeePerGas(ctx_at_N) is the fee for N+1, so
// we call it on the *parent* ctx (height-1). Genesis (height 1) has no
// parent; return the configured default min fee instead.
//
// `getNextBaseFee` is a function pointer rather than a *keeper.Keeper
// method so tests can inject a fake without needing a full keeper.
func pickHeadBaseFee(getNextBaseFee func(sdk.Context) sdk.Dec, ctxProvider func(int64) sdk.Context, height int64) *big.Int {
	if height > 1 {
		return getNextBaseFee(ctxProvider(height - 1)).TruncateInt().BigInt()
	}
	return evmtypes.DefaultMinFeePerGas.TruncateInt().BigInt()
}

func (a *SubscriptionAPI) broadcastNewHead(ethHeader map[string]interface{}) {
	a.newHeadListenersMtx.Lock()
	defer a.newHeadListenersMtx.Unlock()
	toDelete := []rpc.ID{}
	for id, c := range a.newHeadListeners {
		if !handleListener(c, ethHeader) {
			toDelete = append(toDelete, id)
		}
	}
	for _, id := range toDelete {
		delete(a.newHeadListeners, id)
	}
}

func handleListener(c chan map[string]interface{}, ethHeader map[string]interface{}) bool {
	// if the channel is already closed, sending to it/closing it will panic
	defer func() { _ = recover() }()
	select {
	case c <- ethHeader:
		return true
	default:
		// this path is hit when the buffer is full, meaning that the subscriber is not consuming
		// fast enough
		close(c)
		return false
	}
}

func (a *SubscriptionAPI) NewHeads(ctx context.Context) (s *rpc.Subscription, err error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, "eth_newHeads", a.connectionType, startTime, err, recover())
	}()
	notifier, supported := rpc.NotifierFromContext(ctx)
	if !supported {
		return &rpc.Subscription{}, rpc.ErrNotificationsUnsupported
	}

	rpcSub := notifier.CreateSubscription()
	listener := make(chan map[string]interface{}, NewHeadsListenerBuffer)
	a.newHeadListenersMtx.Lock()
	defer a.newHeadListenersMtx.Unlock()
	if a.subscriptonConfig.newHeadLimit > 0 && uint64(len(a.newHeadListeners)) >= a.subscriptonConfig.newHeadLimit {
		return nil, errors.New("no new subscription can be created")
	}
	a.newHeadListeners[rpcSub.ID] = listener

	go func() {
		defer recoverAndLog()
	OUTER:
		for {
			select {
			case res, ok := <-listener:
				if !ok {
					break OUTER
				}
				if err := notifier.Notify(rpcSub.ID, res); err != nil {
					break OUTER
				}
			case <-rpcSub.Err():
				break OUTER
			}
		}
		a.newHeadListenersMtx.Lock()
		defer a.newHeadListenersMtx.Unlock()
		delete(a.newHeadListeners, rpcSub.ID)
		defer func() { _ = recover() }() // might have already been closed
		close(listener)
	}()

	return rpcSub, nil
}

func (a *SubscriptionAPI) Logs(ctx context.Context, filter *filters.FilterCriteria) (s *rpc.Subscription, _err error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, "eth_logs", a.connectionType, startTime, _err, recover())
	}()
	notifier, supported := rpc.NotifierFromContext(ctx)
	if !supported {
		return &rpc.Subscription{}, rpc.ErrNotificationsUnsupported
	}
	// create empty filter if filter does not exist
	if filter == nil {
		filter = &filters.FilterCriteria{}
	}
	// when fromBlock is 0 and toBlock is latest, adjust the filter
	// to unbounded filter
	if filter.FromBlock != nil && filter.FromBlock.Int64() == 0 &&
		filter.ToBlock != nil && filter.ToBlock.Int64() < 0 {
		latest := big.NewInt(a.logFetcher.ctxProvider(LatestCtxHeight).BlockHeight())
		unboundedFilter := &filters.FilterCriteria{
			FromBlock: latest, // set to latest block height
			ToBlock:   nil,    // set to nil to continue listening
			Addresses: filter.Addresses,
			Topics:    filter.Topics,
		}
		filter = unboundedFilter
	}

	rpcSub := notifier.CreateSubscription()

	// Track subscription metrics
	wpMetrics := GetGlobalMetrics()
	wpMetrics.RecordSubscriptionStart()

	if filter.BlockHash != nil {
		go func() {
			var err error
			defer recoverAndLog()
			defer wpMetrics.RecordSubscriptionEnd()
			logs, _, err := a.logFetcher.GetLogsByFilters(ctx, *filter, 0)
			if err != nil {
				wpMetrics.RecordSubscriptionError()
				_ = notifier.Notify(rpcSub.ID, err)
				return
			}
			for _, log := range logs {
				if err = notifier.Notify(rpcSub.ID, log); err != nil {
					return
				}
			}
		}()
		return rpcSub, nil
	}

	go func() {
		var err error
		defer recoverAndLog()
		defer wpMetrics.RecordSubscriptionEnd()
		begin := int64(0)
		for {
			var logs []*ethtypes.Log
			var lastToHeight int64
			logs, lastToHeight, err = a.logFetcher.GetLogsByFilters(ctx, *filter, begin)
			if err != nil {
				wpMetrics.RecordSubscriptionError()
				_ = notifier.Notify(rpcSub.ID, err)
				return
			}
			for _, log := range logs {
				if err = notifier.Notify(rpcSub.ID, log); err != nil {
					return
				}
			}
			if filter.ToBlock != nil && lastToHeight >= filter.ToBlock.Int64() {
				return
			}
			begin = lastToHeight
			filter.FromBlock = big.NewInt(lastToHeight + 1)
			timer := time.NewTimer(SleepInterval)
			select {
			case <-timer.C:
			case <-ctx.Done():
				timer.Stop()
				return
			}
		}
	}()

	return rpcSub, nil
}

const SubscriberPrefix = "evm.rpc."

type SubscriberID uint64

type SubInfo struct {
	Query          string
	SubscriptionCh <-chan coretypes.ResultEvent
}

type SubscriptionManager struct {
	subMu            sync.Mutex
	NextID           SubscriberID
	SubscriptionInfo map[SubscriberID]SubInfo
	tmClient         client.LocalClient
}

func NewSubscriptionManager(tmClient client.LocalClient) *SubscriptionManager {
	return &SubscriptionManager{
		subMu:            sync.Mutex{},
		NextID:           1,
		SubscriptionInfo: make(map[SubscriberID]SubInfo),
		tmClient:         tmClient,
	}
}

func (s *SubscriptionManager) Subscribe(ctx context.Context, q *QueryBuilder, limit int) (SubscriberID, <-chan coretypes.ResultEvent, error) {
	if s == nil || s.tmClient == nil || q == nil {
		return 0, nil, errors.New("subscription manager is not configured")
	}
	query := q.Build()
	s.subMu.Lock()
	defer s.subMu.Unlock()
	if s.NextID == SubscriberID(math.MaxUint64) {
		return 0, nil, errors.New("subscription identifier space exhausted")
	}
	id := s.NextID
	// ignore deprecation here since the new endpoint does not support polling
	//nolint:staticcheck
	res, err := s.tmClient.Subscribe(ctx, fmt.Sprintf("%s%d", SubscriberPrefix, id), query, limit)
	if err != nil {
		return 0, nil, err
	}
	s.SubscriptionInfo[id] = SubInfo{Query: query, SubscriptionCh: res}
	s.NextID++
	return id, res, nil
}

func (s *SubscriptionManager) Unsubscribe(ctx context.Context, id SubscriberID) error {
	if s == nil || s.tmClient == nil {
		return errors.New("subscription manager is not configured")
	}
	s.subMu.Lock()
	defer s.subMu.Unlock()
	info, ok := s.SubscriptionInfo[id]
	if !ok {
		return fmt.Errorf("subscription %d does not exist", id)
	}
	// ignore deprecation here since the new endpoint does not support polling
	//nolint:staticcheck
	err := s.tmClient.Unsubscribe(ctx, fmt.Sprintf("%s%d", SubscriberPrefix, id), info.Query)
	if err != nil {
		return err
	}
	delete(s.SubscriptionInfo, id)
	return nil
}

// encodeCommittedBlock rejects Autobahn notifications until the producer
// supplies the canonical parent, transaction, and receipt roots required by
// an Ethereum newHeads payload. Publishing zero roots makes the stream look
// cryptographically complete when it is not.
func encodeCommittedBlock(evt blockHeaderEvent, baseFee *big.Int, gasLimit int64) (map[string]interface{}, error) {
	if evt.header == nil || evt.response == nil {
		return nil, errors.New("committed head is missing header or finalize response")
	}
	return nil, fmt.Errorf("Autobahn committed head %d lacks canonical parent, transaction, and receipt roots", evt.header.Height)
}

func encodeTmHeader(
	header tmtypes.EventDataNewBlockHeader,
	baseFee *big.Int,
	gasLimit int64,
) (map[string]interface{}, error) {
	if header.Header.Height <= 0 {
		return nil, fmt.Errorf("new head has invalid height %d", header.Header.Height)
	}
	if header.Header.Time.Unix() < 0 {
		return nil, fmt.Errorf("new head %d timestamp predates Unix epoch", header.Header.Height)
	}
	if baseFee == nil || baseFee.Sign() < 0 {
		return nil, errors.New("new head has invalid base fee")
	}
	encodedGasLimit, err := encodeHeadGasLimit(gasLimit)
	if err != nil {
		return nil, err
	}
	blockHash := common.HexToHash(header.Header.Hash().String())
	number := big.NewInt(header.Header.Height)
	miner := common.HexToAddress(header.Header.ProposerAddress.String())
	gasWanted := uint64(0)
	lastHash := common.HexToHash(header.Header.LastBlockID.Hash.String())
	resultHash := common.HexToHash(header.Header.LastResultsHash.String())
	appHash := common.HexToHash(header.Header.AppHash.String())
	txHash := common.HexToHash(header.Header.DataHash.String())
	for _, txRes := range header.ResultFinalizeBlock.TxResults {
		if txRes == nil || txRes.GasUsed < 0 {
			return nil, errors.New("new head contains invalid transaction gas usage")
		}
		gasUsed := uint64(txRes.GasUsed)
		if math.MaxUint64-gasWanted < gasUsed {
			return nil, errors.New("new head transaction gas usage overflows uint64")
		}
		gasWanted += gasUsed
	}
	result := map[string]interface{}{
		"difficulty":            (*hexutil.Big)(utils.Big0), // inapplicable to Pax
		"extraData":             hexutil.Bytes{},            // inapplicable to Pax
		"gasLimit":              encodedGasLimit,
		"gasUsed":               hexutil.Uint64(gasWanted),
		"logsBloom":             ethtypes.Bloom{},          // inapplicable to Pax
		"miner":                 miner,
		"nonce":                 ethtypes.BlockNonce{}, // inapplicable to Pax
		"number":                (*hexutil.Big)(number),
		"parentHash":            lastHash,
		"receiptsRoot":          resultHash,
		"sha3Uncles":            common.Hash{}, // inapplicable to Pax
		"stateRoot":             appHash,
		"timestamp":             hexutil.Uint64(header.Header.Time.Unix()), //nolint:gosec
		"transactionsRoot":      txHash,
		"mixHash":               common.Hash{},     // inapplicable to Pax
		"excessBlobGas":         hexutil.Uint64(0), // inapplicable to Pax
		"parentBeaconBlockRoot": common.Hash{},     // inapplicable to Pax
		"hash":                  blockHash,
		"baseFeePerGas":         (*hexutil.Big)(baseFee),
		"withdrawalsRoot":       common.Hash{},     // inapplicable to Pax
		"blobGasUsed":           hexutil.Uint64(0), // inapplicable to Pax
	}
	return result, nil
}

func encodeHeadGasLimit(gasLimit int64) (hexutil.Uint64, error) {
	if gasLimit == -1 {
		return hexutil.Uint64(math.MaxUint64), nil
	}
	if gasLimit < 0 {
		return 0, fmt.Errorf("new head has invalid gas limit %d", gasLimit)
	}
	return hexutil.Uint64(gasLimit), nil
}

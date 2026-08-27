package app

import (
	"encoding/json"
	"errors"
	"fmt"
	"math/big"
	"strings"

	"github.com/ethereum/go-ethereum/common"
	ethtypes "github.com/ethereum/go-ethereum/core/types"
	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/artifacts/cw1155"
	evmkeeper "github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	evmtypes "github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	authsigning "github.com/sidiora-labs/paxeer-network/sdk/x/auth/signing"
	receiptstore "github.com/sidiora-labs/paxeer-network/storage/ledger_db/receipt"
	"github.com/sidiora-labs/paxeer-network/utils"
	wasmtypes "github.com/sidiora-labs/paxeer-network/wasm/x/wasm/types"
)

var ERC20ApprovalTopic = common.HexToHash("0x8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b925")
var ERC20TransferTopic = common.HexToHash("0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef")
var ERC721TransferTopic = common.HexToHash("0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef")
var ERC721ApprovalTopic = common.HexToHash("0x8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b925")
var ERC721ApproveAllTopic = common.HexToHash("0x17307eab39ab6107e8899845ad3d59bd9653f200f220920489ca2b5937696c31")
var ERC1155TransferSingleTopic = common.HexToHash("0xc3d58168c5ae7397731d063d5bbf3d657854427343f4c083240f7aacaa2d0f62")
var ERC1155TransferBatchTopic = common.HexToHash("0x4a39dc06d4c0dbc64b70af90fd698a233a518aa5d07e595d983b8c0526c8f7fb")
var ERC1155ApprovalForAllTopic = common.HexToHash("0x17307eab39ab6107e8899845ad3d59bd9653f200f220920489ca2b5937696c31")
var ERC1155URITopic = common.HexToHash("0x6bb7ff708619ba0610cba295a58592e0451dee2622938c8755667688daf3529b")
var EmptyHash = common.HexToHash("0x0")
var TrueHash = common.HexToHash("0x1")

var ErrSyntheticReceiptTranslation = errors.New("synthetic EVM receipt translation failed")

type AllowanceResponse struct {
	Allowance sdk.Int         `json:"allowance"`
	Expires   json.RawMessage `json:"expires"`
}

func getOwnerEventKey(contractAddr string, tokenID string) string {
	return fmt.Sprintf("%s-%s", contractAddr, tokenID)
}

func (app *App) AddCosmosEventsToEVMReceiptIfApplicable(ctx sdk.Context, tx sdk.Tx, checksum [32]byte, response sdk.DeliverTxHookInput) error {
	// hooks will only be called if DeliverTx is successful
	wasmEvents := GetEventsOfType(response, wasmtypes.WasmModuleEventType)
	if len(wasmEvents) == 0 {
		return nil
	}
	logs := []*ethtypes.Log{}
	// Note: txs with a very large number of WASM events may run out of gas due to
	// additional gas consumption from EVM receipt generation and event translation
	wasmToEvmEventGasLimit := app.EvmKeeper.GetDeliverTxHookWasmGasLimit(ctx.WithGasMeter(sdk.NewInfiniteGasMeter(1, 1)))
	wasmToEvmEventCtx := ctx.WithGasMeter(sdk.NewGasMeterWithMultiplier(ctx, wasmToEvmEventGasLimit))
	// unfortunately CW721 transfer events differ from ERC721 transfer events
	// in that CW721 include sender (which can be different than owner) whereas
	// ERC721 always include owner. The following logic refer to the owner
	// event emitted before the transfer and use that instead to populate the
	// synthetic ERC721 event.
	ownerEvents := GetEventsOfType(response, wasmtypes.EventTypeCW721PreTransferOwner)
	ownerEventsMap, err := indexCW721OwnerEvents(ownerEvents)
	if err != nil {
		return err
	}
	cw721TransferCounterMap := map[string]int{}
	for _, wasmEvent := range wasmEvents {
		contractAddr, found := GetAttributeValue(wasmEvent, wasmtypes.AttributeKeyContractAddr)
		if !found {
			continue
		}
		pointerAddr, _, exists := app.EvmKeeper.GetERC20CW20Pointer(wasmToEvmEventCtx, contractAddr)
		if exists {
			translated, err := app.translateCW20Event(wasmToEvmEventCtx, wasmEvent, pointerAddr, contractAddr)
			if err != nil {
				return err
			}
			for _, log := range translated {
				log.Index = uint(len(logs))
				logs = append(logs, log)
			}
			continue
		}
		// check if there is a ERC721 pointer to contract Addr
		pointerAddr, _, exists = app.EvmKeeper.GetERC721CW721Pointer(wasmToEvmEventCtx, contractAddr)
		if exists {
			translated, err := app.translateCW721Event(wasmToEvmEventCtx, wasmEvent, pointerAddr, contractAddr, ownerEventsMap, cw721TransferCounterMap)
			if err != nil {
				return err
			}
			for _, log := range translated {
				log.Index = uint(len(logs))
				logs = append(logs, log)
			}
			continue
		}
		// check if there is a ERC1155 pointer to contract Addr
		pointerAddr, _, exists = app.EvmKeeper.GetERC1155CW1155Pointer(wasmToEvmEventCtx, contractAddr)
		if exists {
			translated, err := app.translateCW1155Event(wasmToEvmEventCtx, wasmEvent, pointerAddr, contractAddr)
			if err != nil {
				return err
			}
			for _, log := range translated {
				log.Index = uint(len(logs))
				logs = append(logs, log)
			}
			continue
		}
	}
	if len(logs) == 0 {
		return nil
	}
	txHash := common.BytesToHash(checksum[:])
	if response.EvmTxInfo != nil {
		txHash = common.HexToHash(response.EvmTxInfo.TxHash)
	}
	var bloom ethtypes.Bloom
	if r, err := app.EvmKeeper.GetTransientReceipt(wasmToEvmEventCtx, txHash, uint64(ctx.TxIndex())); err == nil && r != nil { //nolint:gosec
		r.Logs = append(r.Logs, utils.Map(logs, evmkeeper.ConvertSyntheticEthLog)...)
		for i, l := range r.Logs {
			l.Index = uint32(i) //nolint:gosec
		}
		bloom = ethtypes.CreateBloom(&ethtypes.Receipt{Logs: evmkeeper.GetLogsForTx(r, 0)})
		r.LogsBloom = bloom[:]
		if err := app.EvmKeeper.SetTransientReceipt(wasmToEvmEventCtx, txHash, r); err != nil {
			return fmt.Errorf("persist synthetic EVM receipt: %w", err)
		}
	} else {
		if err != nil && !errors.Is(err, receiptstore.ErrNotFound) {
			return fmt.Errorf("load synthetic EVM receipt: %w", err)
		}
		bloom = ethtypes.CreateBloom(&ethtypes.Receipt{Logs: logs})
		receipt := &evmtypes.Receipt{
			TxType:           evmtypes.ShellEVMTxType,
			TxHashHex:        txHash.Hex(),
			GasUsed:          ctx.GasMeter().GasConsumed(),
			BlockNumber:      uint64(ctx.BlockHeight()), //nolint:gosec
			TransactionIndex: uint32(ctx.TxIndex()),     //nolint:gosec
			Logs:             utils.Map(logs, evmkeeper.ConvertSyntheticEthLog),
			LogsBloom:        bloom[:],
			Status:           uint32(ethtypes.ReceiptStatusSuccessful), // we don't create shell receipt for failed Cosmos tx since there is no event anyway
		}
		sigTx, ok := tx.(authsigning.SigVerifiableTx)
		if ok && len(sigTx.GetSigners()) > 0 {
			// use the first signer as the `from`
			receipt.From = app.EvmKeeper.GetEVMAddressOrDefault(wasmToEvmEventCtx, sigTx.GetSigners()[0]).Hex()
		}
		if err := app.EvmKeeper.SetTransientReceipt(wasmToEvmEventCtx, txHash, receipt); err != nil {
			return fmt.Errorf("persist synthetic EVM receipt: %w", err)
		}
	}
	if d, found := app.EvmKeeper.GetEVMTxDeferredInfo(ctx); found {
		app.EvmKeeper.AppendToEvmTxDeferredInfo(wasmToEvmEventCtx, bloom, txHash, d.Surplus)
	} else {
		app.EvmKeeper.AppendToEvmTxDeferredInfo(wasmToEvmEventCtx, bloom, txHash, sdk.ZeroInt())
	}
	return nil
}

func indexCW721OwnerEvents(ownerEvents []abci.Event) (map[string][]abci.Event, error) {
	ownerEventsMap := map[string][]abci.Event{}
	for _, ownerEvent := range ownerEvents {
		if len(ownerEvent.Attributes) != 3 {
			return nil, fmt.Errorf("%w: CW721 owner event must have exactly three attributes", ErrSyntheticReceiptTranslation)
		}
		contractAddr, contractFound := GetAttributeValue(ownerEvent, wasmtypes.AttributeKeyContractAddr)
		tokenID, tokenFound := GetAttributeValue(ownerEvent, wasmtypes.AttributeKeyTokenId)
		owner, ownerFound := GetAttributeValue(ownerEvent, wasmtypes.AttributeKeyOwner)
		if !contractFound || !tokenFound || !ownerFound || contractAddr == "" || tokenID == "" || owner == "" {
			return nil, fmt.Errorf("%w: malformed CW721 owner event", ErrSyntheticReceiptTranslation)
		}
		if _, err := sdk.AccAddressFromBech32(contractAddr); err != nil {
			return nil, fmt.Errorf("%w: invalid CW721 owner-event contract: %v", ErrSyntheticReceiptTranslation, err)
		}
		if err := validateUint256("CW721 owner-event token ID", safeBigIntFromString(tokenID)); err != nil {
			return nil, err
		}
		if _, err := sdk.AccAddressFromBech32(owner); err != nil {
			return nil, fmt.Errorf("%w: invalid CW721 owner-event owner: %v", ErrSyntheticReceiptTranslation, err)
		}
		ownerEventKey := getOwnerEventKey(contractAddr, tokenID)
		ownerEventsMap[ownerEventKey] = append(ownerEventsMap[ownerEventKey], ownerEvent)
	}
	return ownerEventsMap, nil
}

func (app *App) translateCW20Event(ctx sdk.Context, wasmEvent abci.Event, pointerAddr common.Address, contractAddr string) (res []*ethtypes.Log, err error) {
	actions, err := app.GetActionsFromWasmEvent(ctx, wasmEvent)
	if err != nil {
		return nil, err
	}
	for _, action := range actions {
		switch action.Type {
		case "mint", "burn", "send", "transfer", "transfer_from", "send_from", "burn_from":
			if action.Err != nil {
				return nil, action.Err
			}
			if err := validateUint256("CW20 amount", action.Amount); err != nil {
				return nil, err
			}
			if action.Type == "mint" {
				if action.To == EmptyHash {
					return nil, fmt.Errorf("%w: CW20 mint recipient is missing", ErrSyntheticReceiptTranslation)
				}
			} else if action.Type == "burn" || action.Type == "burn_from" {
				if action.From == EmptyHash {
					return nil, fmt.Errorf("%w: CW20 burn source is missing", ErrSyntheticReceiptTranslation)
				}
			} else if action.From == EmptyHash || action.To == EmptyHash {
				return nil, fmt.Errorf("%w: CW20 transfer endpoint is missing", ErrSyntheticReceiptTranslation)
			}
			res = append(res, &ethtypes.Log{
				Address: pointerAddr,
				Topics: []common.Hash{
					ERC20TransferTopic,
					action.From,
					action.To,
				},
				Data: common.BigToHash(action.Amount).Bytes(),
			})
		case "increase_allowance", "decrease_allowance":
			if action.Err != nil {
				return nil, action.Err
			}
			if action.Owner == EmptyHash || action.Spender == EmptyHash {
				return nil, fmt.Errorf("%w: CW20 allowance endpoint is missing", ErrSyntheticReceiptTranslation)
			}
			topics := []common.Hash{
				ERC20ApprovalTopic,
				action.Owner,
				action.Spender,
			}
			contract, err := sdk.AccAddressFromBech32(contractAddr)
			if err != nil {
				return nil, fmt.Errorf("%w: invalid CW20 contract address: %v", ErrSyntheticReceiptTranslation, err)
			}
			ret, err := app.WasmKeeper.QuerySmart(
				ctx,
				contract,
				[]byte(fmt.Sprintf(
					"{\"allowance\":{\"owner\":\"%s\",\"spender\":\"%s\"}}",
					app.EvmKeeper.GetPaxAddressOrDefault(ctx, common.BytesToAddress(action.Owner[:])).String(),
					app.EvmKeeper.GetPaxAddressOrDefault(ctx, common.BytesToAddress(action.Spender[:])).String())),
			)
			if err != nil {
				return nil, fmt.Errorf("%w: query CW20 allowance: %v", ErrSyntheticReceiptTranslation, err)
			}
			allowanceResponse := &AllowanceResponse{}
			if err := json.Unmarshal(ret, allowanceResponse); err != nil {
				return nil, fmt.Errorf("%w: decode CW20 allowance: %v", ErrSyntheticReceiptTranslation, err)
			}
			if allowanceResponse.Allowance.IsNil() || allowanceResponse.Allowance.IsNegative() {
				return nil, fmt.Errorf("%w: invalid CW20 allowance", ErrSyntheticReceiptTranslation)
			}
			res = append(res, &ethtypes.Log{
				Address: pointerAddr,
				Topics:  topics,
				Data:    common.BigToHash(allowanceResponse.Allowance.BigInt()).Bytes(),
			})
		}
	}
	return res, nil
}

func (app *App) translateCW721Event(ctx sdk.Context, wasmEvent abci.Event, pointerAddr common.Address, contractAddr string,
	ownerEventsMap map[string][]abci.Event, cw721TransferCounterMap map[string]int) (res []*ethtypes.Log, err error) {
	actions, err := app.GetActionsFromWasmEvent(ctx, wasmEvent)
	if err != nil {
		return nil, err
	}
	for _, action := range actions {
		switch action.Type {
		case "transfer_nft", "send_nft", "burn":
			if action.Err != nil {
				return nil, action.Err
			}
			if err := validateUint256("CW721 token ID", action.TokenId); err != nil {
				return nil, err
			}
			if action.Type != "burn" && action.Recipient == EmptyHash {
				return nil, fmt.Errorf("%w: CW721 transfer recipient is missing", ErrSyntheticReceiptTranslation)
			}
			ownerEventKey := getOwnerEventKey(contractAddr, action.TokenId.String())
			var currentCounter int
			if c, ok := cw721TransferCounterMap[ownerEventKey]; ok {
				currentCounter = c
			}
			cw721TransferCounterMap[ownerEventKey] = currentCounter + 1
			ownerEvents, ok := ownerEventsMap[ownerEventKey]
			if !ok || len(ownerEvents) <= currentCounter {
				return nil, fmt.Errorf("%w: missing CW721 owner event for %s", ErrSyntheticReceiptTranslation, ownerEventKey)
			}
			ownerPaxAddrStr, found := GetAttributeValue(ownerEvents[currentCounter], wasmtypes.AttributeKeyOwner)
			if !found {
				return nil, fmt.Errorf("%w: CW721 owner attribute is missing", ErrSyntheticReceiptTranslation)
			}
			ownerPaxAddr, err := sdk.AccAddressFromBech32(ownerPaxAddrStr)
			if err != nil {
				return nil, fmt.Errorf("%w: invalid CW721 owner: %v", ErrSyntheticReceiptTranslation, err)
			}
			ownerEvmAddr := app.EvmKeeper.GetEVMAddressOrDefault(ctx, ownerPaxAddr)
			sender := common.BytesToHash(ownerEvmAddr[:])
			res = append(res, &ethtypes.Log{
				Address: pointerAddr,
				Topics: []common.Hash{
					ERC721TransferTopic,
					sender,
					action.Recipient,
					common.BigToHash(action.TokenId),
				},
				Data: EmptyHash.Bytes(),
			})
		case "mint":
			if action.Err != nil {
				return nil, action.Err
			}
			if err := validateUint256("CW721 token ID", action.TokenId); err != nil {
				return nil, err
			}
			if action.Owner == EmptyHash {
				return nil, fmt.Errorf("%w: CW721 mint owner is missing", ErrSyntheticReceiptTranslation)
			}
			res = append(res, &ethtypes.Log{
				Address: pointerAddr,
				Topics: []common.Hash{
					ERC721TransferTopic,
					EmptyHash,
					action.Owner,
					common.BigToHash(action.TokenId),
				},
				Data: EmptyHash.Bytes(),
			})
		case "approve":
			if action.Err != nil {
				return nil, action.Err
			}
			if err := validateUint256("CW721 token ID", action.TokenId); err != nil {
				return nil, err
			}
			if action.Sender == EmptyHash || action.Spender == EmptyHash {
				return nil, fmt.Errorf("%w: CW721 approval endpoint is missing", ErrSyntheticReceiptTranslation)
			}
			res = append(res, &ethtypes.Log{
				Address: pointerAddr,
				Topics: []common.Hash{
					ERC721ApprovalTopic,
					action.Sender,
					action.Spender,
					common.BigToHash(action.TokenId),
				},
				Data: EmptyHash.Bytes(),
			})
		case "revoke":
			if action.Err != nil {
				return nil, action.Err
			}
			if err := validateUint256("CW721 token ID", action.TokenId); err != nil {
				return nil, err
			}
			if action.Sender == EmptyHash {
				return nil, fmt.Errorf("%w: CW721 revoke owner is missing", ErrSyntheticReceiptTranslation)
			}
			res = append(res, &ethtypes.Log{
				Address: pointerAddr,
				Topics: []common.Hash{
					ERC721ApprovalTopic,
					action.Sender,
					EmptyHash,
					common.BigToHash(action.TokenId),
				},
				Data: EmptyHash.Bytes(),
			})
		case "approve_all":
			if action.Err != nil {
				return nil, action.Err
			}
			if action.Sender == EmptyHash || action.Operator == EmptyHash {
				return nil, fmt.Errorf("%w: CW721 approval-for-all endpoint is missing", ErrSyntheticReceiptTranslation)
			}
			res = append(res, &ethtypes.Log{
				Address: pointerAddr,
				Topics: []common.Hash{
					ERC721ApproveAllTopic,
					action.Sender,
					action.Operator,
				},
				Data: TrueHash.Bytes(),
			})
		case "revoke_all":
			if action.Err != nil {
				return nil, action.Err
			}
			if action.Sender == EmptyHash || action.Operator == EmptyHash {
				return nil, fmt.Errorf("%w: CW721 revoke-all endpoint is missing", ErrSyntheticReceiptTranslation)
			}
			res = append(res, &ethtypes.Log{
				Address: pointerAddr,
				Topics: []common.Hash{
					ERC721ApproveAllTopic,
					action.Sender,
					action.Operator,
				},
				Data: EmptyHash.Bytes(),
			})
		}
	}
	return res, nil
}

func (app *App) translateCW1155Event(ctx sdk.Context, wasmEvent abci.Event, pointerAddr common.Address, contractAddr string) (res []*ethtypes.Log, err error) {
	actions, err := app.GetActionsFromWasmEvent(ctx, wasmEvent)
	if err != nil {
		return nil, err
	}
	for _, action := range actions {
		switch action.Type {
		case "transfer_single", "mint_single", "burn_single":
			if action.Err != nil {
				return nil, action.Err
			}
			fromHash := EmptyHash
			toHash := EmptyHash
			if action.Type != "mint_single" {
				fromHash = action.Owner
			}
			if action.Type != "burn_single" {
				toHash = action.Recipient
			}
			if err := validateUint256("CW1155 token ID", action.TokenId); err != nil {
				return nil, err
			}
			if err := validateUint256("CW1155 amount", action.Amount); err != nil {
				return nil, err
			}
			if action.Sender == EmptyHash || (action.Type != "mint_single" && action.Owner == EmptyHash) || (action.Type != "burn_single" && action.Recipient == EmptyHash) {
				return nil, fmt.Errorf("%w: CW1155 transfer endpoint is missing", ErrSyntheticReceiptTranslation)
			}
			dataHash1 := common.BigToHash(action.TokenId).Bytes()
			dataHash2 := common.BigToHash(action.Amount).Bytes()
			res = append(res, &ethtypes.Log{
				Address: pointerAddr,
				Topics: []common.Hash{
					ERC1155TransferSingleTopic,
					action.Sender,
					fromHash,
					toHash,
				},
				Data: append(dataHash1, dataHash2...),
			})
		case "transfer_batch", "mint_batch", "burn_batch":
			if action.Err != nil {
				return nil, action.Err
			}
			fromHash := EmptyHash
			toHash := EmptyHash
			if action.Type != "mint_batch" {
				fromHash = action.Owner
			}
			if action.Type != "burn_batch" {
				toHash = action.Recipient
			}
			if len(action.TokenIds) == 0 || len(action.TokenIds) != len(action.Amounts) {
				return nil, fmt.Errorf("%w: CW1155 batch token and amount lengths differ", ErrSyntheticReceiptTranslation)
			}
			for _, tokenID := range action.TokenIds {
				if err := validateUint256("CW1155 token ID", tokenID); err != nil {
					return nil, err
				}
			}
			for _, amount := range action.Amounts {
				if err := validateUint256("CW1155 amount", amount); err != nil {
					return nil, err
				}
			}
			if action.Sender == EmptyHash || (action.Type != "mint_batch" && action.Owner == EmptyHash) || (action.Type != "burn_batch" && action.Recipient == EmptyHash) {
				return nil, fmt.Errorf("%w: CW1155 batch endpoint is missing", ErrSyntheticReceiptTranslation)
			}
			dataArgs := cw1155.GetParsedABI().Events["TransferBatch"].Inputs.NonIndexed()
			value, err := dataArgs.Pack(action.TokenIds, action.Amounts)
			if err != nil {
				return nil, fmt.Errorf("%w: encode CW1155 batch: %v", ErrSyntheticReceiptTranslation, err)
			}
			res = append(res, &ethtypes.Log{
				Address: pointerAddr,
				Topics: []common.Hash{
					ERC1155TransferBatchTopic,
					action.Sender,
					fromHash,
					toHash,
				},
				Data: value,
			})
		case "approve_all":
			if action.Err != nil {
				return nil, action.Err
			}
			if action.Sender == EmptyHash || action.Operator == EmptyHash {
				return nil, fmt.Errorf("%w: CW1155 approval-for-all endpoint is missing", ErrSyntheticReceiptTranslation)
			}
			res = append(res, &ethtypes.Log{
				Address: pointerAddr,
				Topics: []common.Hash{
					ERC1155ApprovalForAllTopic,
					action.Sender,
					action.Operator,
				},
				Data: TrueHash.Bytes(),
			})
		case "revoke_all":
			if action.Err != nil {
				return nil, action.Err
			}
			if action.Sender == EmptyHash || action.Operator == EmptyHash {
				return nil, fmt.Errorf("%w: CW1155 revoke-all endpoint is missing", ErrSyntheticReceiptTranslation)
			}
			res = append(res, &ethtypes.Log{
				Address: pointerAddr,
				Topics: []common.Hash{
					ERC1155ApprovalForAllTopic,
					action.Sender,
					action.Operator,
				},
				Data: EmptyHash.Bytes(),
			})
		}
	}
	return res, nil
}

func (app *App) GetEvmAddressHash(ctx sdk.Context, addrStr string) (common.Hash, error) {
	paxAddr, err := sdk.AccAddressFromBech32(addrStr)
	if err != nil {
		return common.Hash{}, fmt.Errorf("%w: invalid Pax address: %v", ErrSyntheticReceiptTranslation, err)
	}
	evmAddr := app.EvmKeeper.GetEVMAddressOrDefault(ctx, paxAddr)
	return common.BytesToHash(evmAddr[:]), nil
}

func GetEventsOfType(rdtx sdk.DeliverTxHookInput, ty string) (res []abci.Event) {
	for _, event := range rdtx.Events {
		if event.Type == ty {
			res = append(res, event)
		}
	}
	return
}

func GetAttributeValue(event abci.Event, attribute string) (string, bool) {
	for _, attr := range event.Attributes {
		if string(attr.Key) == attribute {
			return string(attr.Value), true
		}
	}
	return "", false
}

func (app *App) GetActionsFromWasmEvent(ctx sdk.Context, event abci.Event) ([]*Action, error) {
	actions := []*Action{}
	for _, attr := range event.Attributes {
		key := string(attr.Key)
		value := string(attr.Value)
		if key == "action" {
			actions = append(actions, &Action{Type: value})
			continue
		}
		if len(actions) == 0 {
			continue
		}
		curAction := actions[len(actions)-1]
		var parseErr error
		switch key {
		case "amount":
			curAction.Amount = safeBigIntFromString(value)
		case "amounts":
			curAction.Amounts = parseBigInts(value)
		case "token_id":
			curAction.TokenId = safeBigIntFromString(value)
		case "token_ids":
			curAction.TokenIds = parseBigInts(value)
		case "sender":
			curAction.Sender, parseErr = app.GetEvmAddressHash(ctx, value)
		case "recipient":
			curAction.Recipient, parseErr = app.GetEvmAddressHash(ctx, value)
		case "spender":
			curAction.Spender, parseErr = app.GetEvmAddressHash(ctx, value)
		case "operator":
			curAction.Operator, parseErr = app.GetEvmAddressHash(ctx, value)
		case "owner":
			curAction.Owner, parseErr = app.GetEvmAddressHash(ctx, value)
		case "from":
			curAction.From, parseErr = app.GetEvmAddressHash(ctx, value)
		case "to":
			curAction.To, parseErr = app.GetEvmAddressHash(ctx, value)
		}
		if parseErr != nil && curAction.Err == nil {
			curAction.Err = parseErr
		}
	}
	return actions, nil
}

func parseBigInts(value string) []*big.Int {
	return utils.Map(strings.Split(value, ","), safeBigIntFromString)
}

func validateUint256(field string, value *big.Int) error {
	if value == nil || value.Sign() < 0 || value.BitLen() > 256 {
		return fmt.Errorf("%w: invalid %s", ErrSyntheticReceiptTranslation, field)
	}
	return nil
}

func safeBigIntFromString(s string) *big.Int {
	sdkInt, ok := sdk.NewIntFromString(s)
	if !ok {
		return nil
	}
	return sdkInt.BigInt()
}

type Action struct {
	Type      string
	Amount    *big.Int
	Amounts   []*big.Int
	TokenId   *big.Int
	TokenIds  []*big.Int
	Sender    common.Hash
	Recipient common.Hash
	Spender   common.Hash
	Operator  common.Hash
	Owner     common.Hash
	From      common.Hash
	To        common.Hash
	Err       error
}

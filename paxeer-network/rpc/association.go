package evmrpc

import (
	"context"
	"encoding/hex"
	"errors"
	"fmt"
	"math"
	"strings"
	"time"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types/ethtx"
	"github.com/sidiora-labs/paxeer-network/sdk/client"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
)

type AssociationAPI struct {
	tmClient         client.LocalClient
	keeper           *keeper.Keeper
	ctxProvider      func(int64) sdk.Context
	txConfigProvider func(int64) client.TxConfig
	sendAPI          *SendAPI
	connectionType   ConnectionType
	watermarks       *WatermarkManager
}

func NewAssociationAPI(
	tmClient client.LocalClient,
	k *keeper.Keeper,
	ctxProvider func(int64) sdk.Context,
	txConfigProvider func(int64) client.TxConfig,
	sendAPI *SendAPI,
	connectionType ConnectionType,
	watermarks *WatermarkManager,
) *AssociationAPI {
	return &AssociationAPI{
		tmClient:         tmClient,
		keeper:           k,
		ctxProvider:      ctxProvider,
		txConfigProvider: txConfigProvider,
		sendAPI:          sendAPI,
		connectionType:   connectionType,
		watermarks:       watermarks,
	}
}

type AssociateRequest struct {
	R             string `json:"r"`
	S             string `json:"s"`
	V             string `json:"v"`
	CustomMessage string `json:"custom_message"`
}

func (t *AssociationAPI) Associate(ctx context.Context, req *AssociateRequest) (returnErr error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, "pax_associate", t.connectionType, startTime, returnErr, recover())
	}()
	rBytes, err := decodeHexString(req.R)
	if err != nil {
		return err
	}
	sBytes, err := decodeHexString(req.S)
	if err != nil {
		return err
	}
	vBytes, err := decodeHexString(req.V)
	if err != nil {
		return err
	}

	associateTx := ethtx.AssociateTx{
		V:             vBytes,
		R:             rBytes,
		S:             sBytes,
		CustomMessage: req.CustomMessage,
	}

	msg, err := types.NewMsgEVMTransaction(&associateTx)
	if err != nil {
		return err
	}
	txBuilder := t.sendAPI.txConfigProvider(LatestCtxHeight).NewTxBuilder()
	if err = txBuilder.SetMsgs(msg); err != nil {
		return err
	}
	txbz, encodeErr := t.sendAPI.txConfigProvider(LatestCtxHeight).TxEncoder()(txBuilder.GetTx())
	if encodeErr != nil {
		return encodeErr
	}

	res, broadcastError := t.tmClient.BroadcastTx(ctx, txbz)
	if broadcastError != nil {
		err = broadcastError
	} else if res == nil {
		err = errors.New("missing broadcast response")
	} else if res.Code != 0 {
		err = sdkerrors.ABCIError(sdkerrors.RootCodespace, res.Code, "")
	}

	return err
}

func (t *AssociationAPI) GetPaxAddress(ctx context.Context, ethAddress common.Address) (result string, returnErr error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, "pax_getPaxAddress", t.connectionType, startTime, returnErr, recover())
	}()
	paxAddress, found := t.keeper.GetPaxAddress(t.ctxProvider(LatestCtxHeight), ethAddress)
	if !found {
		return "", fmt.Errorf("failed to find Pax address for %s", ethAddress.Hex())
	}

	return paxAddress.String(), nil
}

func (t *AssociationAPI) GetEVMAddress(ctx context.Context, paxAddress string) (result string, returnErr error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, "pax_getEVMAddress", t.connectionType, startTime, returnErr, recover())
	}()
	paxAddr, err := sdk.AccAddressFromBech32(paxAddress)
	if err != nil {
		return "", err
	}
	ethAddress, found := t.keeper.GetEVMAddress(t.ctxProvider(LatestCtxHeight), paxAddr)
	if !found {
		return "", fmt.Errorf("failed to find EVM address for %s", paxAddress)
	}

	return ethAddress.Hex(), nil
}

func decodeHexString(hexString string) ([]byte, error) {
	trimmed := strings.TrimPrefix(hexString, "0x")
	if len(trimmed)%2 != 0 {
		trimmed = "0" + trimmed
	}
	return hex.DecodeString(trimmed)
}

func (t *AssociationAPI) GetCosmosTx(ctx context.Context, ethHash common.Hash) (result string, returnErr error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, "pax_getCosmosTx", t.connectionType, startTime, returnErr, recover())
	}()
	receipt, err := t.keeper.GetReceipt(t.ctxProvider(LatestCtxHeight), ethHash)
	if err != nil {
		return "", err
	}
	if receipt.BlockNumber > math.MaxInt64 {
		return "", fmt.Errorf("invalid block number: %d", receipt.BlockNumber)
	}
	height := int64(receipt.BlockNumber) //nolint:gosec
	block, err := blockByNumberRespectingWatermarks(ctx, t.tmClient, t.watermarks, &height, 1)
	if err != nil {
		return "", err
	}
	if int(receipt.TransactionIndex) >= len(block.Block.Txs) {
		return "", fmt.Errorf("transaction index %d out of range (block has %d txs)", receipt.TransactionIndex, len(block.Block.Txs))
	}
	return fmt.Sprintf("%X", block.Block.Txs[receipt.TransactionIndex].Hash()), nil
}

func (t *AssociationAPI) GetEvmTx(ctx context.Context, cosmosHash string) (result string, returnErr error) {
	startTime := time.Now()
	defer func() {
		recordMetricsWithError(ctx, "pax_getEvmTx", t.connectionType, startTime, returnErr, recover())
	}()
	hashBytes, err := hex.DecodeString(cosmosHash)
	if err != nil {
		return "", fmt.Errorf("failed to decode cosmosHash: %w", err)
	}

	txResponse, err := t.tmClient.Tx(ctx, hashBytes, false)
	if err != nil {
		return "", err
	}
	if txResponse.TxResult.EvmTxInfo == nil {
		return "", fmt.Errorf("transaction not found")
	}

	return txResponse.TxResult.EvmTxInfo.TxHash, nil
}

package custodyproof

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/ethereum/go-ethereum/crypto"
	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	"github.com/sidiora-labs/paxeer-network/consensus/crypto/merkle"
	"github.com/sidiora-labs/paxeer-network/consensus/light"
	"github.com/sidiora-labs/paxeer-network/consensus/rpc/coretypes"
	"github.com/sidiora-labs/paxeer-network/consensus/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/config"
	"github.com/sidiora-labs/paxeer-network/sdk/store/rootmulti"
	storetypes "github.com/sidiora-labs/paxeer-network/sdk/store/types"
)

const (
	Version         = "paxeer-custody-state-v2"
	MaxInputBytes   = 128 * 1024 * 1024
	MaxHistory      = 8192
	MaxValidators   = 1000
	MaxRuntimeBytes = 512 * 1024
	MaxProofBytes   = 1024 * 1024
	TrustingPeriod  = 24 * time.Hour
	MaxClockDrift   = time.Minute
)

type Expected struct {
	GenesisSHA256 string `json:"genesis_sha256"`
	CometChainID  string `json:"comet_chain_id"`
	ChainID       uint64 `json:"chain_id"`
	Vault         string `json:"vault"`
	RuntimeSHA256 string `json:"runtime_sha256"`
	Confirmations uint64 `json:"confirmations"`
	DepositID     string `json:"deposit_id,omitempty"`
	DepositSlot   string `json:"deposit_slot,omitempty"`
}

type LightBlock struct {
	Commit     coretypes.ResultCommit       `json:"commit"`
	Validators []coretypes.ResultValidators `json:"validators"`
}

type StatePoint struct {
	Height   int64               `json:"height"`
	Code     abci.ResponseQuery  `json:"code"`
	CodeHash abci.ResponseQuery  `json:"code_hash"`
	Deposit  *abci.ResponseQuery `json:"deposit,omitempty"`
}

type Bundle struct {
	Version         string       `json:"version"`
	Genesis         []byte       `json:"genesis"`
	History         []LightBlock `json:"history"`
	StateHeight     int64        `json:"state_height"`
	FinalizedHeight int64        `json:"finalized_height"`
	State           []StatePoint `json:"state"`
}

type Request struct {
	Operation string   `json:"operation,omitempty"`
	Expected  Expected `json:"expected"`
	Bundle    Bundle   `json:"bundle"`
}

type Result struct {
	Version             string `json:"version"`
	StateHeight         int64  `json:"state_height"`
	StateHeaderHash     string `json:"state_header_hash"`
	ApplicationRoot     string `json:"application_root"`
	FinalizedHeight     int64  `json:"finalized_height"`
	FinalizedHeaderHash string `json:"finalized_header_hash"`
	RuntimeSHA256       string `json:"runtime_sha256"`
	ProofSHA256         string `json:"proof_sha256"`
	DepositID           string `json:"deposit_id,omitempty"`
	Runtime             []byte `json:"runtime"`
}

func fixedHex(value string, length int) ([]byte, error) {
	if len(value) != length*2+2 || !strings.HasPrefix(value, "0x") {
		return nil, errors.New("hex length or prefix")
	}
	decoded, err := hex.DecodeString(value[2:])
	if err != nil || bytes.Equal(decoded, make([]byte, length)) {
		return nil, errors.New("invalid or zero identity")
	}
	return decoded, nil
}

func hexBytes(value []byte) string { return "0x" + hex.EncodeToString(value) }

func validatorSet(pages []coretypes.ResultValidators, height int64) (*types.ValidatorSet, error) {
	if len(pages) == 0 || len(pages) > (MaxValidators+99)/100 {
		return nil, errors.New("validator page bound")
	}
	var validators []*types.Validator
	seen := make(map[string]bool)
	var totalPower int64
	total := pages[0].Total
	if total <= 0 || total > MaxValidators || len(pages) != (total+99)/100 {
		return nil, errors.New("validator total bound")
	}
	for index, page := range pages {
		count := min(100, total-index*100)
		if page.BlockHeight != height || page.Total != total || page.Count != count || len(page.Validators) != count {
			return nil, errors.New("validator page identity")
		}
		for _, validator := range page.Validators {
			if validator == nil || validator.PubKey == nil || validator.ValidateBasic() != nil ||
				validator.VotingPower <= 0 || validator.VotingPower > types.MaxTotalVotingPower-totalPower ||
				!bytes.Equal(validator.Address, validator.PubKey.Address()) || seen[string(validator.Address)] {
				return nil, errors.New("invalid or duplicate validator")
			}
			totalPower += validator.VotingPower
			seen[string(validator.Address)] = true
			validators = append(validators, validator)
		}
	}
	return types.NewValidatorSet(validators), nil
}

func genesisValidators(genesisBytes []byte, expected Expected, now time.Time) (*types.GenesisDoc, *types.ValidatorSet, error) {
	genesisHash, err := fixedHex(expected.GenesisSHA256, 32)
	if err != nil {
		return nil, nil, err
	}
	if expected.ChainID != 125 || config.GetEVMChainID(expected.CometChainID).Uint64() != expected.ChainID ||
		len(genesisBytes) == 0 || len(genesisBytes) > 32*1024*1024 {
		return nil, nil, errors.New("genesis identity bounds")
	}
	digest := sha256.Sum256(genesisBytes)
	if !bytes.Equal(digest[:], genesisHash) {
		return nil, nil, errors.New("genesis identity")
	}
	var genesis types.GenesisDoc
	if err := json.Unmarshal(genesisBytes, &genesis); err != nil {
		return nil, nil, err
	}
	if genesis.ChainID != expected.CometChainID || genesis.InitialHeight != 1 || genesis.GenesisTime.IsZero() ||
		genesis.GenesisTime.After(now.Add(MaxClockDrift)) || len(genesis.Validators) == 0 || len(genesis.Validators) > MaxValidators {
		return nil, nil, errors.New("genesis chain or validator identity")
	}
	validators := make([]*types.Validator, 0, len(genesis.Validators))
	for _, validator := range genesis.Validators {
		validators = append(validators, &types.Validator{Address: validator.Address,
			PubKey: validator.PubKey, VotingPower: validator.Power})
	}
	var pages []coretypes.ResultValidators
	for start := 0; start < len(validators); start += 100 {
		end := min(start+100, len(validators))
		pages = append(pages, coretypes.ResultValidators{BlockHeight: 1, Count: end - start,
			Total: len(validators), Validators: validators[start:end]})
	}
	initial, err := validatorSet(pages, 1)
	return &genesis, initial, err
}

func verifyEntry(previous *types.SignedHeader, entry *LightBlock, genesis *types.GenesisDoc,
	initial *types.ValidatorSet, now time.Time) error {
	header := &entry.Commit.SignedHeader
	if !entry.Commit.CanonicalCommit || header.ValidateBasic(genesis.ChainID) != nil || len(header.AppHash) != 32 ||
		!header.Time.Before(now.Add(MaxClockDrift)) {
		return errors.New("signed header identity")
	}
	validators, err := validatorSet(entry.Validators, header.Height)
	if err != nil {
		return err
	}
	if previous == nil {
		if header.Height != 1 || !bytes.Equal(initial.Hash(), validators.Hash()) ||
			!bytes.Equal(initial.Hash(), header.ValidatorsHash) || header.Time.Before(genesis.GenesisTime) {
			return errors.New("initial header genesis binding")
		}
	} else {
		if previous.ValidateBasic(genesis.ChainID) != nil || previous.Height >= int64(^uint64(0)>>1) ||
			header.Height != previous.Height+1 || !header.Time.After(previous.Time) ||
			!header.LastBlockID.Equals(previous.Commit.BlockID) ||
			!bytes.Equal(header.ValidatorsHash, previous.NextValidatorsHash) ||
			!bytes.Equal(header.ValidatorsHash, validators.Hash()) {
			return errors.New("historical header transition")
		}
	}
	if err := validators.VerifyCommitLightAllSignatures(genesis.ChainID, header.Commit.BlockID,
		header.Height, header.Commit); err != nil {
		return fmt.Errorf("header quorum: %w", err)
	}
	return nil
}

func verifyHistory(bundle *Bundle, expected Expected, now time.Time) ([]*types.SignedHeader, error) {
	if bundle.Version != Version || len(bundle.History) == 0 || len(bundle.History) > MaxHistory ||
		bundle.FinalizedHeight >= MaxHistory || int64(len(bundle.History)) != bundle.FinalizedHeight+1 {
		return nil, errors.New("individual history message bound")
	}
	genesis, initial, err := genesisValidators(bundle.Genesis, expected, now)
	if err != nil {
		return nil, err
	}
	headers := make([]*types.SignedHeader, 0, len(bundle.History))
	var previous *types.SignedHeader
	for index := range bundle.History {
		entry := &bundle.History[index]
		if err := verifyEntry(previous, entry, genesis, initial, now); err != nil {
			return nil, err
		}
		previous = &entry.Commit.SignedHeader
		headers = append(headers, previous)
	}
	return headers, nil
}

func membership(response *abci.ResponseQuery, height int64, root, key []byte, maxValue int) error {
	if response == nil || response.Code != 0 || response.Height != height ||
		!bytes.Equal(response.Key, key) || len(response.Value) == 0 || len(response.Value) > maxValue ||
		response.ProofOps == nil || len(response.ProofOps.Ops) != 2 {
		return errors.New("state query identity or missing proof")
	}
	ops := response.ProofOps.Ops
	if ops[0].Type != storetypes.ProofOpIAVLCommitment || !bytes.Equal(ops[0].Key, key) ||
		ops[1].Type != storetypes.ProofOpSimpleMerkleCommitment || !bytes.Equal(ops[1].Key, []byte("evm")) {
		return errors.New("state proof store or key")
	}
	for _, op := range ops {
		if len(op.Data) == 0 || len(op.Data) > MaxProofBytes {
			return errors.New("state proof size")
		}
	}
	path := merkle.KeyPath{}.AppendKey([]byte("evm"), merkle.KeyEncodingURL).AppendKey(key, merkle.KeyEncodingURL)
	if err := rootmulti.DefaultProofRuntime().VerifyValue(response.ProofOps, root, path.String(), response.Value); err != nil {
		return fmt.Errorf("application-root membership: %w", err)
	}
	return nil
}

func Verify(request *Request, now time.Time) (*Result, error) {
	if request == nil {
		return nil, errors.New("missing proof request")
	}
	headers, err := verifyHistory(&request.Bundle, request.Expected, now)
	if err != nil {
		return nil, err
	}
	return verifyState(request, now, func(height int64) (*types.SignedHeader, error) {
		if height <= 0 || height > int64(len(headers)) {
			return nil, errors.New("unverified state height")
		}
		return headers[height-1], nil
	})
}

func verifyState(request *Request, now time.Time, lookup func(int64) (*types.SignedHeader, error)) (*Result, error) {
	if request == nil || request.Expected.ChainID != 125 || request.Expected.CometChainID == "" || now.IsZero() {
		return nil, errors.New("Paxeer proof identity")
	}
	expected, bundle := request.Expected, &request.Bundle
	if config.GetEVMChainID(expected.CometChainID).Uint64() != expected.ChainID {
		return nil, errors.New("Comet to EVM chain identity")
	}
	if bundle.Version != Version || bundle.StateHeight < 2 || bundle.FinalizedHeight < bundle.StateHeight ||
		bundle.FinalizedHeight == int64(^uint64(0)>>1) || expected.Confirmations == 0 || expected.Confirmations >= MaxHistory ||
		bundle.FinalizedHeight-bundle.StateHeight >= MaxHistory ||
		uint64(bundle.FinalizedHeight-bundle.StateHeight+1) < expected.Confirmations {
		return nil, errors.New("state finality window")
	}
	stateHeader, err := lookup(bundle.StateHeight + 1)
	if err != nil {
		return nil, err
	}
	finalHeader, err := lookup(bundle.FinalizedHeight + 1)
	if err != nil {
		return nil, err
	}
	if light.HeaderExpired(finalHeader, TrustingPeriod, now) || !finalHeader.Time.Before(now.Add(MaxClockDrift)) {
		return nil, errors.New("live head freshness")
	}
	vault, err := fixedHex(expected.Vault, 20)
	if err != nil {
		return nil, err
	}
	runtimeHash, err := fixedHex(expected.RuntimeSHA256, 32)
	if err != nil {
		return nil, err
	}
	var depositKey []byte
	if expected.DepositID != "" {
		depositID, err := fixedHex(expected.DepositID, 32)
		if err != nil {
			return nil, err
		}
		if len(expected.DepositSlot) != 66 || !strings.HasPrefix(expected.DepositSlot, "0x") {
			return nil, errors.New("deposit storage slot")
		}
		slot, err := hex.DecodeString(expected.DepositSlot[2:])
		if err != nil {
			return nil, err
		}
		depositKey = append(append([]byte{3}, vault...), crypto.Keccak256(depositID, slot)...)
	} else if expected.DepositSlot != "" {
		return nil, errors.New("unexpected deposit slot")
	}
	points := []int64{bundle.StateHeight}
	if bundle.FinalizedHeight != bundle.StateHeight {
		points = append(points, bundle.FinalizedHeight)
	}
	if len(bundle.State) != len(points) {
		return nil, errors.New("state point count")
	}
	var runtime []byte
	for index, height := range points {
		point := &bundle.State[index]
		if point.Height != height {
			return nil, errors.New("state point height")
		}
		header, err := lookup(height + 1)
		if err != nil {
			return nil, err
		}
		root := header.AppHash
		if err := membership(&point.Code, height, root, append([]byte{7}, vault...), MaxRuntimeBytes); err != nil {
			return nil, err
		}
		if err := membership(&point.CodeHash, height, root, append([]byte{8}, vault...), 32); err != nil {
			return nil, err
		}
		digest := sha256.Sum256(point.Code.Value)
		if !bytes.Equal(digest[:], runtimeHash) || !bytes.Equal(crypto.Keccak256(point.Code.Value), point.CodeHash.Value) {
			return nil, errors.New("authenticated vault runtime")
		}
		if depositKey != nil {
			if err := membership(point.Deposit, height, root, depositKey, 32); err != nil {
				return nil, err
			}
			one := make([]byte, 32)
			one[31] = 1
			if !bytes.Equal(point.Deposit.Value, one) {
				return nil, errors.New("deposit membership value")
			}
		} else if point.Deposit != nil {
			return nil, errors.New("unexpected deposit proof")
		}
		runtime = point.Code.Value
	}
	canonical, err := json.Marshal(bundle)
	if err != nil {
		return nil, err
	}
	digest := sha256.Sum256(canonical)
	return &Result{Version: Version, StateHeight: bundle.StateHeight,
		StateHeaderHash: hexBytes(stateHeader.Hash()),
		ApplicationRoot: hexBytes(stateHeader.AppHash),
		FinalizedHeight: bundle.FinalizedHeight, FinalizedHeaderHash: hexBytes(finalHeader.Hash()),
		RuntimeSHA256: hexBytes(runtimeHash), ProofSHA256: hexBytes(digest[:]), DepositID: expected.DepositID,
		Runtime: runtime}, nil
}

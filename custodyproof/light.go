package custodyproof

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"errors"
	"fmt"
	"math/big"
	"time"

	ics23 "github.com/confio/ics23/go"

	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	"github.com/sidiora-labs/paxeer-network/consensus/crypto"
	"github.com/sidiora-labs/paxeer-network/consensus/crypto/ed25519"
	tmmath "github.com/sidiora-labs/paxeer-network/consensus/libs/math"
	"github.com/sidiora-labs/paxeer-network/consensus/libs/utils"
	tmcrypto "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/crypto"
	"github.com/sidiora-labs/paxeer-network/consensus/rpc/coretypes"
	"github.com/sidiora-labs/paxeer-network/consensus/types"
	"github.com/sidiora-labs/paxeer-network/consensus/version"
	"github.com/sidiora-labs/paxeer-network/modules/evm/config"
	custodytypes "github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	storetypes "github.com/sidiora-labs/paxeer-network/sdk/store/types"
)

const (
	LightProfileMagic    = "LXBC3"
	LightCreditMagic     = "LXDC3"
	LightBundleMagic     = "LXLB1"
	LightProfileBytes    = 207
	LightCreditHeadBytes = 363
	LightProtocol        = 3
	LightProofKind       = 2
	LightEVMChainID      = 125
	LightMaxIAVLSteps    = 64
	LightMaxStoreSteps   = 32
	LightReserveAccount  = "system:paxeer-reserve"
	LightAccountDomain   = "LX:ACCOUNT:v1"
	LightNullifierDomain = "LX:DEPOSIT:NULLIFIER:v1"
)

var lightTrustLevel = tmmath.Fraction{Numerator: 1, Denominator: 3}

// lightMaxTimestampSeconds is 9999-12-31T23:59:59Z, the last second a
// protobuf Timestamp can carry; vote sign bytes cannot encode a later one.
const lightMaxTimestampSeconds = 253402300799

// LightProfile is the decoded LXBC3 genesis bridge profile.
type LightProfile struct {
	EVMChainID     uint64
	Custody        [20]byte
	ModuleIdentity [32]byte
	TrustedHash    [32]byte
	AssetID        [32]byte
	ReserveAccount [32]byte
	TrustedHeight  uint64
	CometChainID   string
	NetworkID      uint32
}

// LightEvidence is what a Paxeer node returns for one deposit: the signed
// header at height N, the validator set of height N, the store proof of the
// deposit record at height N-1 and, for a non-adjacent update across a
// validator-set change, the validator set the verifier currently trusts.
type LightEvidence struct {
	Header     *types.SignedHeader
	Validators *types.ValidatorSet
	Trusted    *types.ValidatorSet
	Deposit    *abci.ResponseQuery
}

// LightTrust is the verifier's trust state.
type LightTrust struct {
	Height             int64
	HeaderHash         []byte
	NextValidatorsHash []byte
	Time               time.Time
}

// LightCredit is a decoded and verified LXDC3 payload.
type LightCredit struct {
	ProfileHash    [32]byte
	NetworkID      uint32
	DepositID      [32]byte
	AssetID        [32]byte
	Beneficiary    [32]byte
	OwnerKey       [32]byte
	Payer          [20]byte
	Amount         *big.Int
	Nonce          uint64
	StateHeight    uint64
	HeaderHash     [32]byte
	AppHash        [32]byte
	HeaderHeight   uint64
	ValidatorsHash [32]byte
	BundleHash     [32]byte
	Nullifier      [32]byte
	Evidence       LightEvidence
	Trust          LightTrust
}

// LightAccountID derives a LayerX account identifier from its name.
func LightAccountID(name string) [32]byte {
	var length [4]byte
	binary.BigEndian.PutUint32(length[:], uint32(len(name)))
	return sha256.Sum256(append(append([]byte(LightAccountDomain), length[:]...), name...))
}

// LightNullifier is the deposit nullifier a credit consumes.
func LightNullifier(depositID [32]byte) [32]byte {
	return sha256.Sum256(append([]byte(LightNullifierDomain), depositID[:]...))
}

// ValidatorSetFromPages builds the validator set of one height from every
// RPC page, in RPC order, under the reference verifier's bounds.
func ValidatorSetFromPages(pages []coretypes.ResultValidators, height int64) (*types.ValidatorSet, error) {
	return validatorSet(pages, height)
}

func lightChainID(chainID string) error {
	if len(chainID) == 0 || len(chainID) > 32 || len(chainID) > types.MaxChainIDLen {
		return errors.New("comet chain id length")
	}
	for _, character := range []byte(chainID) {
		if character < 0x21 || character > 0x7e {
			return errors.New("comet chain id is not printable ASCII")
		}
	}
	if config.GetEVMChainID(chainID).Uint64() != LightEVMChainID {
		return errors.New("comet to EVM chain identity")
	}
	return nil
}

// BuildLightProfile serializes the 207-byte LXBC3 profile from the signed
// header at the trusted height.
func BuildLightProfile(trusted *types.SignedHeader, assetID [32]byte, networkID uint32) ([]byte, error) {
	if trusted == nil || trusted.Header == nil || trusted.Commit == nil || networkID == 0 || assetID == ([32]byte{}) {
		return nil, errors.New("light profile identity")
	}
	if err := lightChainID(trusted.ChainID); err != nil {
		return nil, err
	}
	if err := trusted.ValidateBasic(trusted.ChainID); err != nil {
		return nil, fmt.Errorf("trusted header: %w", err)
	}
	if trusted.Height < 1 || len(trusted.NextValidatorsHash) != 32 {
		return nil, errors.New("trusted header height or validator hash")
	}
	address, err := custodytypes.ParseAddress(custodytypes.CustodyAddress)
	if err != nil {
		return nil, err
	}
	module, reserve := ModuleIdentity(), LightAccountID(LightReserveAccount)
	out := make([]byte, 0, LightProfileBytes)
	out = append(out, LightProfileMagic...)
	out = binary.BigEndian.AppendUint64(out, LightEVMChainID)
	out = append(out, address.Bytes()...)
	out = append(out, module[:]...)
	out = append(out, trusted.NextValidatorsHash...)
	out = append(out, assetID[:]...)
	out = append(out, reserve[:]...)
	out = binary.BigEndian.AppendUint64(out, uint64(trusted.Height))
	chain := make([]byte, 32)
	copy(chain, trusted.ChainID)
	out = append(out, chain...)
	out = binary.BigEndian.AppendUint32(out, networkID)
	out = binary.BigEndian.AppendUint16(out, LightProtocol)
	if len(out) != LightProfileBytes {
		return nil, errors.New("light profile size")
	}
	if _, err := DecodeLightProfile(out); err != nil {
		return nil, err
	}
	return out, nil
}

// DecodeLightProfile parses and checks an LXBC3 profile.
func DecodeLightProfile(profile []byte) (*LightProfile, error) {
	if len(profile) != LightProfileBytes || string(profile[:5]) != LightProfileMagic {
		return nil, errors.New("light profile size or magic")
	}
	out := &LightProfile{EVMChainID: binary.BigEndian.Uint64(profile[5:13]),
		TrustedHeight: binary.BigEndian.Uint64(profile[161:169]),
		NetworkID:     binary.BigEndian.Uint32(profile[201:205])}
	copy(out.Custody[:], profile[13:33])
	copy(out.ModuleIdentity[:], profile[33:65])
	copy(out.TrustedHash[:], profile[65:97])
	copy(out.AssetID[:], profile[97:129])
	copy(out.ReserveAccount[:], profile[129:161])
	chain := profile[169:201]
	length := bytes.IndexByte(chain, 0)
	if length < 0 {
		length = len(chain)
	}
	if !bytes.Equal(chain[length:], make([]byte, 32-length)) {
		return nil, errors.New("light profile chain id padding")
	}
	out.CometChainID = string(chain[:length])
	if err := lightChainID(out.CometChainID); err != nil {
		return nil, err
	}
	address, err := custodytypes.ParseAddress(custodytypes.CustodyAddress)
	if err != nil {
		return nil, err
	}
	if out.EVMChainID != LightEVMChainID || !bytes.Equal(out.Custody[:], address.Bytes()) ||
		out.ModuleIdentity != ModuleIdentity() || out.ReserveAccount != LightAccountID(LightReserveAccount) ||
		out.TrustedHash == ([32]byte{}) || out.AssetID == ([32]byte{}) || out.TrustedHeight < 1 ||
		out.TrustedHeight > uint64(^uint64(0)>>1) || out.NetworkID == 0 ||
		binary.BigEndian.Uint16(profile[205:207]) != LightProtocol {
		return nil, errors.New("light profile identity")
	}
	return out, nil
}

// TrustFromProfile seeds a verifier's trust state from its profile.
func TrustFromProfile(profile *LightProfile) LightTrust {
	return LightTrust{Height: int64(profile.TrustedHeight), NextValidatorsHash: append([]byte(nil), profile.TrustedHash[:]...)}
}

type lightWriter struct{ bytes.Buffer }

func (w *lightWriter) u8(value int)     { w.WriteByte(byte(value)) }
func (w *lightWriter) u16(value int)    { w.Write(binary.BigEndian.AppendUint16(nil, uint16(value))) }
func (w *lightWriter) u32(value uint32) { w.Write(binary.BigEndian.AppendUint32(nil, value)) }
func (w *lightWriter) u64(value uint64) { w.Write(binary.BigEndian.AppendUint64(nil, value)) }

func (w *lightWriter) hash8(value []byte) error {
	if len(value) != 0 && len(value) != 32 {
		return errors.New("header hash length")
	}
	w.u8(len(value))
	w.Write(value)
	return nil
}

func (w *lightWriter) short(value []byte) error {
	if len(value) > 255 {
		return errors.New("proof operand length")
	}
	w.u8(len(value))
	w.Write(value)
	return nil
}

func (w *lightWriter) validators(set *types.ValidatorSet) error {
	if set == nil {
		w.u16(0)
		return nil
	}
	if len(set.Validators) == 0 || len(set.Validators) > MaxValidators {
		return errors.New("validator count")
	}
	w.u16(len(set.Validators))
	for _, validator := range set.Validators {
		if validator == nil || validator.VotingPower <= 0 || len(validator.PubKey.Bytes()) != 32 ||
			!bytes.Equal(validator.Address, validator.PubKey.Address()) {
			return errors.New("validator identity")
		}
		w.Write(validator.PubKey.Bytes())
		w.u64(uint64(validator.VotingPower))
	}
	return nil
}

func lightExistence(op []byte, spec *ics23.ProofSpec, steps int) (*ics23.ExistenceProof, error) {
	var commitment ics23.CommitmentProof
	if err := commitment.Unmarshal(op); err != nil {
		return nil, err
	}
	exist := commitment.GetExist()
	if exist == nil || exist.Leaf == nil {
		return nil, errors.New("proof op is not an existence proof")
	}
	reencoded, err := (&ics23.CommitmentProof{Proof: &ics23.CommitmentProof_Exist{Exist: exist}}).Marshal()
	if err != nil || !bytes.Equal(reencoded, op) {
		return nil, errors.New("proof op is not a canonical existence proof")
	}
	leaf := exist.Leaf
	if leaf.Hash != ics23.HashOp_SHA256 || leaf.PrehashKey != ics23.HashOp_NO_HASH ||
		leaf.PrehashValue != ics23.HashOp_SHA256 || leaf.Length != ics23.LengthOp_VAR_PROTO ||
		leaf.Hash != spec.LeafSpec.Hash || leaf.PrehashKey != spec.LeafSpec.PrehashKey ||
		leaf.PrehashValue != spec.LeafSpec.PrehashValue || leaf.Length != spec.LeafSpec.Length ||
		!bytes.HasPrefix(leaf.Prefix, spec.LeafSpec.Prefix) || len(leaf.Prefix) > 255 {
		return nil, errors.New("leaf op shape")
	}
	if len(exist.Path) > steps {
		return nil, errors.New("proof step count")
	}
	for _, inner := range exist.Path {
		if inner == nil || inner.Hash != ics23.HashOp_SHA256 || len(inner.Prefix) > 255 || len(inner.Suffix) > 255 {
			return nil, errors.New("inner op shape")
		}
	}
	return exist, nil
}

func (w *lightWriter) path(exist *ics23.ExistenceProof) error {
	if err := w.short(exist.Leaf.Prefix); err != nil {
		return err
	}
	w.u8(len(exist.Path))
	for _, inner := range exist.Path {
		if err := errors.Join(w.short(inner.Prefix), w.short(inner.Suffix)); err != nil {
			return err
		}
	}
	return nil
}

// EncodeLightBundle serializes LXLB1. It refuses evidence whose shape the
// binary format cannot carry or whose implicit fields do not hold; it does not
// verify signatures or proofs, which BuildLightCredit does before calling it.
func EncodeLightBundle(evidence *LightEvidence) ([]byte, error) {
	if evidence == nil || evidence.Header == nil || evidence.Header.Header == nil || evidence.Header.Commit == nil ||
		evidence.Validators == nil || evidence.Deposit == nil {
		return nil, errors.New("missing light evidence")
	}
	header, commit := evidence.Header.Header, evidence.Header.Commit
	if header.Height < 1 || header.Time.Unix() <= 0 || header.Time.Unix() > lightMaxTimestampSeconds || len(header.ProposerAddress) != 20 || len(header.AppHash) != 32 ||
		len(header.ValidatorsHash) != 32 || len(header.NextValidatorsHash) != 32 {
		return nil, errors.New("header bounds")
	}
	if commit.Height != header.Height || commit.Round < 0 || !bytes.Equal(commit.BlockID.Hash, header.Hash()) ||
		len(commit.BlockID.PartSetHeader.Hash) != 32 || commit.BlockID.PartSetHeader.Total == 0 ||
		commit.BlockID.PartSetHeader.Total > types.MaxBlockPartsCount {
		return nil, errors.New("commit does not bind the header")
	}
	if len(commit.Signatures) != len(evidence.Validators.Validators) {
		return nil, errors.New("commit signature count")
	}
	w := &lightWriter{}
	w.WriteString(LightBundleMagic)
	w.u64(header.Version.Block)
	w.u64(header.Version.App)
	w.u64(uint64(header.Height))
	w.u64(uint64(header.Time.Unix()))
	w.u32(uint32(header.Time.Nanosecond()))
	if err := w.hash8(header.LastBlockID.Hash); err != nil {
		return nil, err
	}
	w.u32(header.LastBlockID.PartSetHeader.Total)
	if err := w.hash8(header.LastBlockID.PartSetHeader.Hash); err != nil {
		return nil, err
	}
	for _, hash := range [][]byte{header.LastCommitHash, header.DataHash, header.ValidatorsHash, header.NextValidatorsHash,
		header.ConsensusHash, header.AppHash, header.LastResultsHash, header.EvidenceHash} {
		if err := w.hash8(hash); err != nil {
			return nil, err
		}
	}
	w.u8(len(header.ProposerAddress))
	w.Write(header.ProposerAddress)
	w.u32(uint32(commit.Round))
	w.u32(commit.BlockID.PartSetHeader.Total)
	w.Write(commit.BlockID.PartSetHeader.Hash)
	if err := w.validators(evidence.Validators); err != nil {
		return nil, err
	}
	for index, signature := range commit.Signatures {
		switch signature.BlockIDFlag {
		case types.BlockIDFlagAbsent:
			if len(signature.ValidatorAddress) != 0 || signature.Signature.IsPresent() || !signature.Timestamp.IsZero() {
				return nil, errors.New("absent commit signature is not empty")
			}
			w.u8(int(signature.BlockIDFlag))
			continue
		case types.BlockIDFlagCommit, types.BlockIDFlagNil:
		default:
			return nil, errors.New("commit signature flag")
		}
		raw, present := signature.Signature.Get()
		if !present || len(raw.Bytes()) != 64 ||
			!bytes.Equal(signature.ValidatorAddress, evidence.Validators.Validators[index].Address) {
			return nil, errors.New("commit signature is not aligned with its validator")
		}
		w.u8(int(signature.BlockIDFlag))
		w.u64(uint64(signature.Timestamp.Unix()))
		w.u32(uint32(signature.Timestamp.Nanosecond()))
		w.Write(raw.Bytes())
	}
	if evidence.Trusted != nil && len(evidence.Trusted.Validators) == 0 {
		return nil, errors.New("empty trusted validator set")
	}
	if err := w.validators(evidence.Trusted); err != nil {
		return nil, err
	}
	response := evidence.Deposit
	if response.ProofOps == nil || len(response.ProofOps.Ops) != 2 || len(response.Key) != 33 ||
		response.Key[0] != custodytypes.DepositPrefix[0] || len(response.Value) == 0 || len(response.Value) > MaxRecordBytes {
		return nil, errors.New("deposit query shape")
	}
	iavl, err := lightExistence(response.ProofOps.Ops[0].Data, ics23.IavlSpec, LightMaxIAVLSteps)
	if err != nil {
		return nil, fmt.Errorf("iavl proof: %w", err)
	}
	store, err := lightExistence(response.ProofOps.Ops[1].Data, ics23.TendermintSpec, LightMaxStoreSteps)
	if err != nil {
		return nil, fmt.Errorf("store proof: %w", err)
	}
	root, err := iavl.Calculate()
	if err != nil {
		return nil, err
	}
	if !bytes.Equal(iavl.Key, response.Key) || !bytes.Equal(iavl.Value, response.Value) ||
		string(store.Key) != custodytypes.StoreKey || !bytes.Equal(store.Value, root) {
		return nil, errors.New("proof operands do not chain")
	}
	w.u16(len(iavl.Key))
	w.Write(iavl.Key)
	w.u16(len(iavl.Value))
	w.Write(iavl.Value)
	if err := w.path(iavl); err != nil {
		return nil, err
	}
	if err := w.short(store.Key); err != nil {
		return nil, err
	}
	if err := w.path(store); err != nil {
		return nil, err
	}
	return w.Bytes(), nil
}

type lightDeposit struct {
	id, asset, beneficiary [32]byte
	payer                  [20]byte
	amount                 *big.Int
	nonce                  uint64
}

func lightRecord(value []byte, assetID [32]byte, stateHeight int64) (*lightDeposit, error) {
	var deposit custodytypes.Deposit
	if err := deposit.Unmarshal(value); err != nil || deposit.Validate() != nil ||
		deposit.AssetId != custodytypes.Hash32(assetID) || deposit.Height <= 0 || deposit.Height > stateHeight {
		return nil, errors.New("authenticated custody deposit")
	}
	canonical, err := deposit.Marshal()
	if err != nil || !bytes.Equal(canonical, value) {
		return nil, errors.New("custody deposit encoding")
	}
	out := &lightDeposit{asset: assetID, nonce: deposit.Nonce}
	if out.id, err = custodytypes.ParseNonZeroHash32(deposit.DepositId); err != nil {
		return nil, err
	}
	depositor, err := custodytypes.ParseAddress(deposit.Depositor)
	if err != nil {
		return nil, err
	}
	out.payer = depositor
	if out.beneficiary, err = custodytypes.ParseNonZeroHash32(deposit.Beneficiary); err != nil {
		return nil, err
	}
	amount, err := custodytypes.ParseAmount(deposit.Amount)
	if err != nil {
		return nil, err
	}
	out.amount = amount.BigInt()
	if out.amount.Sign() <= 0 || out.amount.BitLen() > 128 || out.nonce == 0 {
		return nil, errors.New("custody deposit amount or nonce")
	}
	if custodytypes.DepositID(new(big.Int).SetUint64(LightEVMChainID), depositor, assetID, out.beneficiary,
		out.amount, out.nonce) != out.id {
		return nil, errors.New("deposit identifier preimage")
	}
	return out, nil
}

// verifyLightEvidence is the reference check: the existing Go light-client
// and store-proof code decides whether evidence is acceptable under a trust
// state. It returns the deposit and the trust state after the update.
func verifyLightEvidence(profile *LightProfile, trust LightTrust, evidence *LightEvidence) (*lightDeposit, LightTrust, error) {
	none := LightTrust{}
	if evidence == nil || evidence.Header == nil || evidence.Header.Header == nil || evidence.Header.Commit == nil ||
		evidence.Validators == nil || evidence.Deposit == nil {
		return nil, none, errors.New("missing light evidence")
	}
	signed := evidence.Header
	if err := signed.ValidateBasic(profile.CometChainID); err != nil {
		return nil, none, fmt.Errorf("signed header: %w", err)
	}
	if len(signed.AppHash) != 32 || signed.Height < 2 {
		return nil, none, errors.New("signed header bounds")
	}
	if err := evidence.Validators.ValidateBasic(); err != nil {
		return nil, none, err
	}
	if !bytes.Equal(evidence.Validators.Hash(), signed.ValidatorsHash) {
		return nil, none, errors.New("validator set does not hash to the header's validators hash")
	}
	if err := evidence.Validators.VerifyCommitLightAllSignatures(profile.CometChainID, signed.Commit.BlockID,
		signed.Height, signed.Commit); err != nil {
		return nil, none, fmt.Errorf("header quorum: %w", err)
	}
	if trust.Height < 1 || len(trust.NextValidatorsHash) != 32 {
		return nil, none, errors.New("trust state")
	}
	next := LightTrust{Height: signed.Height, HeaderHash: signed.Hash(), NextValidatorsHash: signed.NextValidatorsHash, Time: signed.Time}
	switch {
	case signed.Height < trust.Height:
		return nil, none, errors.New("header is below the trusted height")
	case signed.Height == trust.Height:
		if len(trust.HeaderHash) != 32 || !bytes.Equal(trust.HeaderHash, signed.Hash()) {
			return nil, none, errors.New("header at the trusted height is not the trusted header")
		}
		if evidence.Trusted != nil {
			return nil, none, errors.New("unneeded trusted validator set")
		}
		next = trust
	case bytes.Equal(signed.ValidatorsHash, trust.NextValidatorsHash):
		if evidence.Trusted != nil {
			return nil, none, errors.New("unneeded trusted validator set")
		}
	case signed.Height == trust.Height+1:
		return nil, none, errors.New("adjacent header does not carry the trusted validator set")
	default:
		if evidence.Trusted == nil || evidence.Trusted.ValidateBasic() != nil ||
			!bytes.Equal(evidence.Trusted.Hash(), trust.NextValidatorsHash) {
			return nil, none, errors.New("trusted validator set does not hash to the trusted hash")
		}
		if err := evidence.Trusted.VerifyCommitLightTrustingAllSignatures(profile.CometChainID, signed.Commit,
			lightTrustLevel); err != nil {
			return nil, none, fmt.Errorf("trusted overlap: %w", err)
		}
	}
	stateHeight := signed.Height - 1
	if err := membership(evidence.Deposit, stateHeight, signed.AppHash, evidence.Deposit.Key, MaxRecordBytes); err != nil {
		return nil, none, err
	}
	deposit, err := lightRecord(evidence.Deposit.Value, profile.AssetID, stateHeight)
	if err != nil {
		return nil, none, err
	}
	if !bytes.Equal(evidence.Deposit.Key, custodytypes.DepositKey(deposit.id)) {
		return nil, none, errors.New("deposit record is not stored under its identifier")
	}
	return deposit, next, nil
}

// BuildLightCredit returns head||bundle for evidence the reference verifier
// accepts under the profile's own trust state.
func BuildLightCredit(profileBytes []byte, ownerKey [32]byte, networkID uint32, evidence *LightEvidence) ([]byte, error) {
	profile, err := DecodeLightProfile(profileBytes)
	if err != nil {
		return nil, err
	}
	if profile.NetworkID != networkID || ownerKey == ([32]byte{}) {
		return nil, errors.New("credit network or owner key")
	}
	if _, err := ed25519.PublicKeyFromBytes(ownerKey[:]); err != nil {
		return nil, fmt.Errorf("owner key: %w", err)
	}
	if evidence != nil && evidence.Header != nil && evidence.Header.Header != nil &&
		evidence.Header.Height <= int64(profile.TrustedHeight) {
		return nil, errors.New("credit header is not above the profile's trusted height")
	}
	deposit, _, err := verifyLightEvidence(profile, TrustFromProfile(profile), evidence)
	if err != nil {
		return nil, err
	}
	bundle, err := EncodeLightBundle(evidence)
	if err != nil {
		return nil, err
	}
	payload := append(lightHead(profileBytes, profile, ownerKey, deposit, evidence.Header, bundle), bundle...)
	if _, err := VerifyLightCredit(profileBytes, payload, nil); err != nil {
		return nil, fmt.Errorf("serialized credit does not verify: %w", err)
	}
	return payload, nil
}

func lightHead(profileBytes []byte, profile *LightProfile, ownerKey [32]byte, deposit *lightDeposit,
	signed *types.SignedHeader, bundle []byte) []byte {
	profileHash, bundleHash := sha256.Sum256(profileBytes), sha256.Sum256(bundle)
	out := make([]byte, 0, LightCreditHeadBytes+len(bundle))
	out = append(out, LightCreditMagic...)
	out = append(out, profileHash[:]...)
	out = binary.BigEndian.AppendUint32(out, profile.NetworkID)
	out = binary.BigEndian.AppendUint16(out, LightProtocol)
	out = append(out, deposit.id[:]...)
	out = append(out, deposit.asset[:]...)
	out = append(out, deposit.beneficiary[:]...)
	out = append(out, ownerKey[:]...)
	out = append(out, deposit.payer[:]...)
	out = append(out, deposit.amount.FillBytes(make([]byte, 16))...)
	out = binary.BigEndian.AppendUint64(out, deposit.nonce)
	out = binary.BigEndian.AppendUint64(out, uint64(signed.Height-1))
	out = append(out, signed.Hash()...)
	out = append(out, signed.AppHash...)
	out = binary.BigEndian.AppendUint64(out, uint64(signed.Height))
	out = append(out, signed.ValidatorsHash...)
	out = append(out, bundleHash[:]...)
	out = binary.BigEndian.AppendUint32(out, LightProofKind)
	return out
}

type lightReader struct {
	data []byte
	err  error
}

func (r *lightReader) take(length int) []byte {
	if r.err != nil || length < 0 || length > len(r.data) {
		r.err = errors.New("truncated light bundle")
		return make([]byte, max(length, 0))
	}
	out := r.data[:length]
	r.data = r.data[length:]
	return out
}

func (r *lightReader) u8() int     { return int(r.take(1)[0]) }
func (r *lightReader) u16() int    { return int(binary.BigEndian.Uint16(r.take(2))) }
func (r *lightReader) u32() uint32 { return binary.BigEndian.Uint32(r.take(4)) }
func (r *lightReader) u64() uint64 { return binary.BigEndian.Uint64(r.take(8)) }

func (r *lightReader) hash8() []byte {
	length := r.u8()
	if length != 0 && length != 32 {
		r.err = errors.New("header hash length")
		return nil
	}
	if length == 0 {
		return nil
	}
	return append([]byte(nil), r.take(length)...)
}

func (r *lightReader) short() []byte { return append([]byte(nil), r.take(r.u8())...) }

func (r *lightReader) time() time.Time {
	seconds, nanos := int64(r.u64()), r.u32()
	if seconds <= 0 || seconds > lightMaxTimestampSeconds || nanos >= 1_000_000_000 {
		r.err = errors.New("timestamp bounds")
		return time.Time{}
	}
	return time.Unix(seconds, int64(nanos)).UTC()
}

func (r *lightReader) validators(required bool) *types.ValidatorSet {
	count := r.u16()
	if count > MaxValidators || (required && count == 0) {
		r.err = errors.New("validator count")
	}
	if r.err != nil || count == 0 {
		return nil
	}
	validators := make([]*types.Validator, 0, count)
	var total uint64
	for range count {
		key, err := ed25519.PublicKeyFromBytes(r.take(32))
		power := r.u64()
		if err != nil || power == 0 || power > uint64(types.MaxTotalVotingPower)-total {
			r.err = errors.New("validator key or power")
			return nil
		}
		total += power
		validators = append(validators, types.NewValidator(key, int64(power)))
	}
	if r.err != nil {
		return nil
	}
	ordered := make([]*types.Validator, len(validators))
	copy(ordered, validators)
	set := types.NewValidatorSet(validators)
	for index, validator := range set.Validators {
		if !bytes.Equal(validator.Address, ordered[index].Address) {
			r.err = errors.New("validators are not in validator-set order")
			return nil
		}
	}
	return set
}

func (r *lightReader) path(spec *ics23.ProofSpec, steps int) (*ics23.LeafOp, []*ics23.InnerOp) {
	leaf := &ics23.LeafOp{Hash: spec.LeafSpec.Hash, PrehashKey: spec.LeafSpec.PrehashKey,
		PrehashValue: spec.LeafSpec.PrehashValue, Length: spec.LeafSpec.Length, Prefix: r.short()}
	count := r.u8()
	if count > steps {
		r.err = errors.New("proof step count")
		return nil, nil
	}
	var path []*ics23.InnerOp
	for range count {
		prefix := r.short()
		path = append(path, &ics23.InnerOp{Hash: ics23.HashOp_SHA256, Prefix: prefix, Suffix: r.short()})
	}
	return leaf, path
}

// DecodeLightBundle parses LXLB1 back into the node types it was built from.
// The chain id is not carried by the bundle and comes from the profile.
func DecodeLightBundle(bundle []byte, chainID string) (*LightEvidence, error) {
	if len(bundle) < 5 || string(bundle[:5]) != LightBundleMagic {
		return nil, errors.New("light bundle magic")
	}
	r := &lightReader{data: bundle[5:]}
	header := &types.Header{ChainID: chainID}
	header.Version = version.Consensus{Block: r.u64(), App: r.u64()}
	height := r.u64()
	if height < 1 || height > uint64(^uint64(0)>>1) {
		return nil, errors.New("header height")
	}
	header.Height = int64(height)
	header.Time = r.time()
	header.LastBlockID.Hash = r.hash8()
	header.LastBlockID.PartSetHeader.Total = r.u32()
	header.LastBlockID.PartSetHeader.Hash = r.hash8()
	header.LastCommitHash, header.DataHash = r.hash8(), r.hash8()
	header.ValidatorsHash, header.NextValidatorsHash = r.hash8(), r.hash8()
	header.ConsensusHash, header.AppHash = r.hash8(), r.hash8()
	header.LastResultsHash, header.EvidenceHash = r.hash8(), r.hash8()
	if r.u8() != 20 {
		return nil, errors.New("proposer address length")
	}
	header.ProposerAddress = append([]byte(nil), r.take(20)...)
	round := r.u32()
	if round > uint32(^uint32(0)>>1) {
		return nil, errors.New("commit round")
	}
	commit := &types.Commit{Height: header.Height, Round: int32(round)}
	commit.BlockID.PartSetHeader.Total = r.u32()
	commit.BlockID.PartSetHeader.Hash = append([]byte(nil), r.take(32)...)
	if r.err != nil {
		return nil, r.err
	}
	if commit.BlockID.PartSetHeader.Total == 0 || commit.BlockID.PartSetHeader.Total > types.MaxBlockPartsCount {
		return nil, errors.New("commit part set total")
	}
	commit.BlockID.Hash = header.Hash()
	validators := r.validators(true)
	if r.err != nil {
		return nil, r.err
	}
	for _, validator := range validators.Validators {
		flag := types.BlockIDFlag(r.u8())
		signature := types.CommitSig{BlockIDFlag: flag}
		switch flag {
		case types.BlockIDFlagAbsent:
		case types.BlockIDFlagCommit, types.BlockIDFlagNil:
			signature.ValidatorAddress = validator.Address
			signature.Timestamp = r.time()
			raw, err := crypto.SigFromBytes(r.take(64))
			if err != nil {
				return nil, err
			}
			signature.Signature = utils.Some(raw)
		default:
			return nil, errors.New("commit signature flag")
		}
		commit.Signatures = append(commit.Signatures, signature)
	}
	trusted := r.validators(false)
	key := append([]byte(nil), r.take(r.u16())...)
	valueLength := r.u16()
	if valueLength == 0 || valueLength > MaxRecordBytes || len(key) != 33 {
		return nil, errors.New("deposit key or value length")
	}
	value := append([]byte(nil), r.take(valueLength)...)
	iavl := &ics23.ExistenceProof{Key: key, Value: value}
	iavl.Leaf, iavl.Path = r.path(ics23.IavlSpec, LightMaxIAVLSteps)
	store := &ics23.ExistenceProof{Key: r.short()}
	store.Leaf, store.Path = r.path(ics23.TendermintSpec, LightMaxStoreSteps)
	if r.err != nil {
		return nil, r.err
	}
	if len(r.data) != 0 {
		return nil, errors.New("trailing light bundle bytes")
	}
	var err error
	if store.Value, err = iavl.Calculate(); err != nil {
		return nil, err
	}
	response := &abci.ResponseQuery{Key: key, Value: value, Height: header.Height - 1}
	response.ProofOps, err = lightProofOps(iavl, store)
	if err != nil {
		return nil, err
	}
	return &LightEvidence{Header: &types.SignedHeader{Header: header, Commit: commit}, Validators: validators,
		Trusted: trusted, Deposit: response}, nil
}

// VerifyLightCredit decodes an LXDC3 payload and accepts it only when the
// reference verifier accepts the evidence it carries and every head field is
// the one that evidence authenticates. A nil trust state is the profile's.
func VerifyLightCredit(profileBytes, payload []byte, trust *LightTrust) (*LightCredit, error) {
	profile, err := DecodeLightProfile(profileBytes)
	if err != nil {
		return nil, err
	}
	if len(payload) <= LightCreditHeadBytes || len(payload) > MaxInputBytes || string(payload[:5]) != LightCreditMagic {
		return nil, errors.New("light credit size or magic")
	}
	head, bundle := payload[:LightCreditHeadBytes], payload[LightCreditHeadBytes:]
	evidence, err := DecodeLightBundle(bundle, profile.CometChainID)
	if err != nil {
		return nil, err
	}
	state := TrustFromProfile(profile)
	if trust != nil {
		state = *trust
	}
	deposit, next, err := verifyLightEvidence(profile, state, evidence)
	if err != nil {
		return nil, err
	}
	reencoded, err := EncodeLightBundle(evidence)
	if err != nil || !bytes.Equal(reencoded, bundle) {
		return nil, errors.New("light bundle is not canonical")
	}
	credit := &LightCredit{NetworkID: profile.NetworkID, Amount: deposit.amount, Evidence: *evidence, Trust: next}
	copy(credit.OwnerKey[:], head[139:171])
	if credit.OwnerKey == ([32]byte{}) {
		return nil, errors.New("credit owner key")
	}
	if _, err := ed25519.PublicKeyFromBytes(credit.OwnerKey[:]); err != nil {
		return nil, fmt.Errorf("owner key: %w", err)
	}
	if !bytes.Equal(head, lightHead(profileBytes, profile, credit.OwnerKey, deposit, evidence.Header, bundle)) {
		return nil, errors.New("credit head does not match its evidence")
	}
	credit.ProfileHash, credit.BundleHash = sha256.Sum256(profileBytes), sha256.Sum256(bundle)
	credit.DepositID, credit.AssetID, credit.Beneficiary = deposit.id, deposit.asset, deposit.beneficiary
	credit.Payer, credit.Nonce = deposit.payer, deposit.nonce
	credit.StateHeight, credit.HeaderHeight = uint64(evidence.Header.Height-1), uint64(evidence.Header.Height)
	copy(credit.HeaderHash[:], evidence.Header.Hash())
	copy(credit.AppHash[:], evidence.Header.AppHash)
	copy(credit.ValidatorsHash[:], evidence.Header.ValidatorsHash)
	credit.Nullifier = LightNullifier(deposit.id)
	return credit, nil
}

func lightProofOps(iavl, store *ics23.ExistenceProof) (*tmcrypto.ProofOps, error) {
	ops := make([]tmcrypto.ProofOp, 0, 2)
	for index, exist := range []*ics23.ExistenceProof{iavl, store} {
		data, err := (&ics23.CommitmentProof{Proof: &ics23.CommitmentProof_Exist{Exist: exist}}).Marshal()
		if err != nil {
			return nil, err
		}
		kind := storetypes.ProofOpIAVLCommitment
		if index == 1 {
			kind = storetypes.ProofOpSimpleMerkleCommitment
		}
		ops = append(ops, tmcrypto.ProofOp{Type: kind, Key: exist.Key, Data: data})
	}
	return &tmcrypto.ProofOps{Ops: ops}, nil
}

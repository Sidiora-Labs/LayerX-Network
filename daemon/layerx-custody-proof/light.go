package main

import (
	"bytes"
	"context"
	"encoding/hex"
	"errors"
	"flag"
	"fmt"
	"os"
	"strings"
	"time"

	rpcclient "github.com/sidiora-labs/paxeer-network/consensus/rpc/client"
	rpchttp "github.com/sidiora-labs/paxeer-network/consensus/rpc/client/http"
	"github.com/sidiora-labs/paxeer-network/consensus/rpc/coretypes"
	"github.com/sidiora-labs/paxeer-network/consensus/types"
	"github.com/sidiora-labs/paxeer-network/custodyproof"
	custodytypes "github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
)

const lightTimeout = 60 * time.Second

func hex32(name, value string) ([32]byte, error) {
	var out [32]byte
	decoded, err := hex.DecodeString(strings.TrimPrefix(value, "0x"))
	if err != nil || len(decoded) != 32 || bytes.Equal(decoded, out[:]) {
		return out, fmt.Errorf("--%s must be 32 non-zero bytes of hex", name)
	}
	copy(out[:], decoded)
	return out, nil
}

func signedHeader(ctx context.Context, client *rpchttp.HTTP, height int64) (*types.SignedHeader, error) {
	result, err := client.Commit(ctx, &height)
	if err != nil {
		return nil, err
	}
	if !result.CanonicalCommit || result.Header == nil || result.Commit == nil || result.Height != height {
		return nil, fmt.Errorf("height %d has no canonical commit yet", height)
	}
	return &result.SignedHeader, nil
}

func validatorsAt(ctx context.Context, client *rpchttp.HTTP, height int64) (*types.ValidatorSet, error) {
	var pages []coretypes.ResultValidators
	perPage := 100
	for page := 1; ; page++ {
		result, err := client.Validators(ctx, &height, &page, &perPage)
		if err != nil {
			return nil, err
		}
		pages = append(pages, *result)
		if page*perPage >= result.Total || page > custodyproof.MaxValidators/perPage {
			break
		}
	}
	return custodyproof.ValidatorSetFromPages(pages, height)
}

func lightProfile(arguments []string) error {
	flags := flag.NewFlagSet("light-profile", flag.ContinueOnError)
	rpc := flags.String("rpc", "", "Comet RPC URL")
	asset := flags.String("asset", "", "32-byte asset id, hex")
	network := flags.Uint("network-id", 0, "LayerX network id")
	trusted := flags.Int64("trusted-height", 0, "trusted header height")
	output := flags.String("output", "", "profile file to write")
	if err := flags.Parse(arguments); err != nil {
		return err
	}
	if flags.NArg() != 0 || *rpc == "" || *output == "" || *trusted < 1 || *network == 0 || *network > 0xffffffff {
		return errors.New("light-profile needs --rpc, --asset, --network-id, --trusted-height and --output")
	}
	assetID, err := hex32("asset", *asset)
	if err != nil {
		return err
	}
	client, err := rpchttp.New(*rpc)
	if err != nil {
		return err
	}
	ctx, cancel := context.WithTimeout(context.Background(), lightTimeout)
	defer cancel()
	header, err := signedHeader(ctx, client, *trusted)
	if err != nil {
		return err
	}
	validators, err := validatorsAt(ctx, client, *trusted)
	if err != nil {
		return err
	}
	if !bytes.Equal(validators.Hash(), header.ValidatorsHash) {
		return errors.New("trusted header validators hash")
	}
	if err := validators.VerifyCommitLightAllSignatures(header.ChainID, header.Commit.BlockID, header.Height,
		header.Commit); err != nil {
		return fmt.Errorf("trusted header quorum: %w", err)
	}
	profile, err := custodyproof.BuildLightProfile(header, assetID, uint32(*network))
	if err != nil {
		return err
	}
	return os.WriteFile(*output, profile, 0o644)
}

func lightCredit(arguments []string) error {
	flags := flag.NewFlagSet("light-credit", flag.ContinueOnError)
	rpc := flags.String("rpc", "", "Comet RPC URL")
	profilePath := flags.String("profile", "", "LXBC3 profile file")
	deposit := flags.String("deposit-id", "", "32-byte deposit id, hex")
	owner := flags.String("owner-key", "", "32-byte beneficiary owner ed25519 public key, hex")
	height := flags.Int64("height", 0, "header height N; the record is proven at N-1 (default: latest canonical commit)")
	trustedHeight := flags.Int64("trusted-validators-height", 0, "trusted height T whose next validator set (height T+1) is carried when needed")
	output := flags.String("output", "", "credit file to write")
	if err := flags.Parse(arguments); err != nil {
		return err
	}
	if flags.NArg() != 0 || *rpc == "" || *profilePath == "" || *output == "" || *height < 0 || *trustedHeight < 0 {
		return errors.New("light-credit needs --rpc, --profile, --deposit-id, --owner-key and --output")
	}
	depositID, err := hex32("deposit-id", *deposit)
	if err != nil {
		return err
	}
	ownerKey, err := hex32("owner-key", *owner)
	if err != nil {
		return err
	}
	profileBytes, err := os.ReadFile(*profilePath)
	if err != nil {
		return err
	}
	profile, err := custodyproof.DecodeLightProfile(profileBytes)
	if err != nil {
		return err
	}
	client, err := rpchttp.New(*rpc)
	if err != nil {
		return err
	}
	ctx, cancel := context.WithTimeout(context.Background(), lightTimeout)
	defer cancel()
	if *height == 0 {
		status, err := client.Status(ctx)
		if err != nil {
			return err
		}
		*height = status.SyncInfo.LatestBlockHeight - 1
	}
	if *height < 2 {
		return errors.New("header height must be at least 2")
	}
	evidence := &custodyproof.LightEvidence{}
	if evidence.Header, err = signedHeader(ctx, client, *height); err != nil {
		return err
	}
	if evidence.Validators, err = validatorsAt(ctx, client, *height); err != nil {
		return err
	}
	if *trustedHeight != 0 {
		if uint64(*trustedHeight) != profile.TrustedHeight {
			return errors.New("--trusted-validators-height must be the profile's trusted height")
		}
		if *height > *trustedHeight+1 && !bytes.Equal(evidence.Header.ValidatorsHash, profile.TrustedHash[:]) {
			if evidence.Trusted, err = validatorsAt(ctx, client, *trustedHeight+1); err != nil {
				return err
			}
		}
	}
	query, err := client.ABCIQueryWithOptions(ctx, "/store/"+custodytypes.StoreKey+"/key", custodytypes.DepositKey(depositID),
		rpcclient.ABCIQueryOptions{Height: *height - 1, Prove: true})
	if err != nil {
		return err
	}
	evidence.Deposit = &query.Response
	credit, err := custodyproof.BuildLightCredit(profileBytes, ownerKey, profile.NetworkID, evidence)
	if err != nil {
		return err
	}
	if err := os.WriteFile(*output, credit, 0o644); err != nil {
		return err
	}
	nullifier := custodyproof.LightNullifier(depositID)
	return os.WriteFile(*output+".nullifier", []byte(hex.EncodeToString(nullifier[:])+"\n"), 0o644)
}

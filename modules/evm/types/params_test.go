package types_test

import (
	"bytes"
	"compress/gzip"
	"io"
	"strings"
	"testing"

	"github.com/gogo/protobuf/jsonpb"
	"github.com/gogo/protobuf/proto"
	"github.com/gogo/protobuf/protoc-gen-gogo/descriptor"
	tmproto "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/sidiora-labs/paxeer-network/sdk/codec"
	codectypes "github.com/sidiora-labs/paxeer-network/sdk/codec/types"
	"github.com/sidiora-labs/paxeer-network/sdk/store"
	paramtypes "github.com/sidiora-labs/paxeer-network/sdk/x/params/types"
	dbm "github.com/tendermint/tm-db"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"

	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/stretchr/testify/require"
)

func TestDefaultParams(t *testing.T) {
	require.Equal(t, types.Params{
		AllowedFeeDenoms:                       types.DefaultAllowedFeeDenoms,
		MaxFeeTokenSpread:                      types.DefaultMaxFeeTokenSpread,
		FeeTokenEnabled:                        types.DefaultFeeTokenEnabled,
		PriorityNormalizer:                     types.DefaultPriorityNormalizer,
		BaseFeePerGas:                          types.DefaultBaseFeePerGas,
		MinimumFeePerGas:                       types.DefaultMinFeePerGas,
		MaximumFeePerGas:                       types.DefaultMaxFeePerGas,
		DeliverTxHookWasmGasLimit:              types.DefaultDeliverTxHookWasmGasLimit,
		WhitelistedCwCodeHashesForDelegateCall: types.DefaultWhitelistedCwCodeHashesForDelegateCall,
		MaxDynamicBaseFeeUpwardAdjustment:      types.DefaultMaxDynamicBaseFeeUpwardAdjustment,
		MaxDynamicBaseFeeDownwardAdjustment:    types.DefaultMaxDynamicBaseFeeDownwardAdjustment,
		TargetGasUsedPerBlock:                  types.DefaultTargetGasUsedPerBlock,
		PaxSstoreSetGasEip2200:                 types.DefaultPaxSstoreSetGasEIP2200,
	}, types.DefaultParams())
	require.Nil(t, types.DefaultParams().Validate())
}

func TestValidateParamsInvalidPriorityNormalizer(t *testing.T) {
	params := types.DefaultParams()
	params.PriorityNormalizer = sdk.NewDec(-1) // Set to invalid negative value

	err := params.Validate()
	require.Error(t, err)
	require.Contains(t, err.Error(), "nonpositive priority normalizer")
}

func TestValidateParamsNegativeBaseFeePerGas(t *testing.T) {
	params := types.DefaultParams()
	params.BaseFeePerGas = sdk.NewDec(-1) // Set to invalid negative value

	err := params.Validate()
	require.Error(t, err)
	require.Contains(t, err.Error(), "negative base fee per gas")
}

func TestBaseFeeMinimumFee(t *testing.T) {
	params := types.DefaultParams()
	params.MinimumFeePerGas = sdk.NewDec(1)
	params.BaseFeePerGas = params.MinimumFeePerGas.Add(sdk.NewDec(1))
	err := params.Validate()
	require.Error(t, err)
	require.Contains(t, err.Error(), "minimum fee cannot be lower than base fee")
}

func TestValidateParamsInvalidMaxDynamicBaseFeeUpwardAdjustment(t *testing.T) {
	params := types.DefaultParams()
	params.MaxDynamicBaseFeeUpwardAdjustment = sdk.NewDec(-1) // Set to invalid negative value

	err := params.Validate()
	require.Error(t, err)
	require.Contains(t, err.Error(), "negative base fee adjustment")

	params.MaxDynamicBaseFeeUpwardAdjustment = sdk.NewDec(2)
	err = params.Validate()
	require.Error(t, err)
	require.Contains(t, err.Error(), "base fee adjustment must be less than or equal to 1")
}

func TestValidateParamsInvalidMaxDynamicBaseFeeDownwardAdjustment(t *testing.T) {
	params := types.DefaultParams()
	params.MaxDynamicBaseFeeDownwardAdjustment = sdk.NewDec(-1) // Set to invalid negative value

	err := params.Validate()
	require.Error(t, err)
	require.Contains(t, err.Error(), "negative base fee adjustment")

	params.MaxDynamicBaseFeeDownwardAdjustment = sdk.NewDec(2)
	err = params.Validate()
	require.Error(t, err)
	require.Contains(t, err.Error(), "base fee adjustment must be less than or equal to 1")
}

func TestValidateParamsInvalidDeliverTxHookWasmGasLimit(t *testing.T) {
	params := types.DefaultParams()
	params.DeliverTxHookWasmGasLimit = 0 // Set to invalid value (0)

	err := params.Validate()
	require.Error(t, err)
	require.Contains(t, err.Error(), "invalid deliver_tx_hook_wasm_gas_limit: must be greater than 0")
}

func TestValidateParamsInvalidMaxFeePerGas(t *testing.T) {
	params := types.DefaultParams()
	params.MaximumFeePerGas = sdk.NewDec(-1) // Set to invalid negative value

	err := params.Validate()
	require.Error(t, err)
	require.Contains(t, err.Error(), "negative max fee per gas")
}

func TestValidateParamsValidDeliverTxHookWasmGasLimit(t *testing.T) {
	params := types.DefaultParams()

	require.Equal(t, params.DeliverTxHookWasmGasLimit, types.DefaultDeliverTxHookWasmGasLimit)

	params.DeliverTxHookWasmGasLimit = 100000 // Set to valid value

	err := params.Validate()
	require.NoError(t, err)
}

func TestValidateParamsInvalidPaxSstoreSetGasEip2200(t *testing.T) {
	params := types.DefaultParams()
	params.PaxSstoreSetGasEip2200 = 0 // Set to invalid value (0)

	err := params.Validate()
	require.Error(t, err)
	require.Contains(t, err.Error(), "invalid pax sstore set gas eip2200: must be greater than 0")
}

func TestFeeTokenParamsDefaults(t *testing.T) {
	params := types.DefaultParams()
	require.Empty(t, params.AllowedFeeDenoms)
	require.False(t, params.FeeTokenEnabled)
	require.Equal(t, sdk.NewDecWithPrec(5, 2), params.MaxFeeTokenSpread)
	require.NoError(t, params.Validate())
}

func TestFeeTokenParamsValidators(t *testing.T) {
	params := types.DefaultParams()
	validators := make(map[string]paramtypes.ValueValidatorFn)
	for _, pair := range params.ParamSetPairs() {
		validators[string(pair.Key)] = pair.ValidatorFn
	}
	tests := []struct {
		name    string
		key     []byte
		value   interface{}
		invalid string
	}{
		{"empty list", types.KeyAllowedFeeDenoms, []types.AllowedFeeDenom(nil), ""},
		{"valid entries", types.KeyAllowedFeeDenoms, []types.AllowedFeeDenom{{Denom: "usid", OraclePair: "SID/PAX"}, {Denom: "ibc/ABC123", OraclePair: "USD/PAX"}}, ""},
		{"empty denom", types.KeyAllowedFeeDenoms, []types.AllowedFeeDenom{{OraclePair: "SID/PAX"}}, "allowed_fee_denoms"},
		{"malformed denom", types.KeyAllowedFeeDenoms, []types.AllowedFeeDenom{{Denom: "bad denom", OraclePair: "SID/PAX"}}, "bad denom"},
		{"duplicate denom", types.KeyAllowedFeeDenoms, []types.AllowedFeeDenom{{Denom: "usid", OraclePair: "SID/PAX"}, {Denom: "usid", OraclePair: "USD/PAX"}}, "duplicate denom"},
		{"missing pair", types.KeyAllowedFeeDenoms, []types.AllowedFeeDenom{{Denom: "usid"}}, "oracle_pair"},
		{"blank pair", types.KeyAllowedFeeDenoms, []types.AllowedFeeDenom{{Denom: "usid", OraclePair: "  "}}, "oracle_pair"},
		{"network denom", types.KeyAllowedFeeDenoms, []types.AllowedFeeDenom{{Denom: "uhpx", OraclePair: "PAX/PAX"}}, "uhpx"},
		{"wrong list type", types.KeyAllowedFeeDenoms, []string{"usid"}, "allowed_fee_denoms"},
		{"zero spread", types.KeyMaxFeeTokenSpread, sdk.ZeroDec(), ""},
		{"positive spread", types.KeyMaxFeeTokenSpread, sdk.NewDecWithPrec(5, 2), ""},
		{"below one spread", types.KeyMaxFeeTokenSpread, sdk.OneDec().Sub(sdk.SmallestDec()), ""},
		{"negative spread", types.KeyMaxFeeTokenSpread, sdk.NewDecWithPrec(-1, 18), "max_fee_token_spread"},
		{"one spread", types.KeyMaxFeeTokenSpread, sdk.OneDec(), "max_fee_token_spread"},
		{"above one spread", types.KeyMaxFeeTokenSpread, sdk.NewDec(2), "max_fee_token_spread"},
		{"nil spread", types.KeyMaxFeeTokenSpread, sdk.Dec{}, "max_fee_token_spread"},
		{"wrong spread type", types.KeyMaxFeeTokenSpread, "0.05", "max_fee_token_spread"},
		{"disabled", types.KeyFeeTokenEnabled, false, ""},
		{"enabled", types.KeyFeeTokenEnabled, true, ""},
		{"wrong switch type", types.KeyFeeTokenEnabled, "true", "fee_token_enabled"},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			validate, ok := validators[string(tc.key)]
			require.True(t, ok)
			err := validate(tc.value)
			if tc.invalid == "" {
				require.NoError(t, err)
			} else {
				require.ErrorContains(t, err, tc.invalid)
			}
			candidate := types.DefaultParams()
			switch value := tc.value.(type) {
			case []types.AllowedFeeDenom:
				candidate.AllowedFeeDenoms = value
			case sdk.Dec:
				candidate.MaxFeeTokenSpread = value
			case bool:
				candidate.FeeTokenEnabled = value
			default:
				return
			}
			if tc.invalid == "" {
				require.NoError(t, candidate.Validate())
			} else {
				require.ErrorContains(t, candidate.Validate(), tc.invalid)
			}
		})
	}
}

func TestFeeTokenParamsStoreRoundTrip(t *testing.T) {
	db := dbm.NewMemDB()
	ms := store.NewCommitMultiStore(db)
	key := sdk.NewKVStoreKey(paramtypes.StoreKey)
	tkey := sdk.NewTransientStoreKey(paramtypes.TStoreKey)
	ms.MountStoreWithDB(key, sdk.StoreTypeIAVL, db)
	ms.MountStoreWithDB(tkey, sdk.StoreTypeTransient, db)
	require.NoError(t, ms.LoadLatestVersion())
	ctx := sdk.NewContext(ms, tmproto.Header{}, false)
	ss := paramtypes.NewSubspace(codec.NewProtoCodec(codectypes.NewInterfaceRegistry()), codec.NewLegacyAmino(), key, tkey, types.ModuleName).WithKeyTable(types.ParamKeyTable())
	expected := types.DefaultParams()
	expected.AllowedFeeDenoms = []types.AllowedFeeDenom{{Denom: "usid", OraclePair: "SID/PAX"}, {Denom: "uasset", OraclePair: "ASSET/PAX"}}
	expected.MaxFeeTokenSpread = sdk.NewDecWithPrec(7, 2)
	expected.FeeTokenEnabled = true
	ss.SetParamSet(ctx, &expected)
	var actual types.Params
	ss.GetParamSet(ctx, &actual)
	require.Equal(t, expected, actual)
	require.Error(t, ss.Update(ctx, types.KeyAllowedFeeDenoms, []byte(`[{"denom":"usid","oracle_pair":""}]`)))
	ss.GetParamSet(ctx, &actual)
	require.Equal(t, expected, actual)
	require.NoError(t, ss.Update(ctx, types.KeyMaxFeeTokenSpread, []byte(`"0.000000000000000000"`)))
	var spread sdk.Dec
	ss.Get(ctx, types.KeyMaxFeeTokenSpread, &spread)
	require.Equal(t, sdk.ZeroDec(), spread)
}

func TestFeeTokenParamsProtoRoundTrip(t *testing.T) {
	expected := types.DefaultParams()
	expected.AllowedFeeDenoms = []types.AllowedFeeDenom{{Denom: "usid", OraclePair: "SID/PAX"}, {Denom: "uasset", OraclePair: "ASSET/PAX"}}
	expected.FeeTokenEnabled = true
	data, err := proto.Marshal(&expected)
	require.NoError(t, err)
	require.Len(t, data, expected.Size())
	var actual types.Params
	require.NoError(t, proto.Unmarshal(data, &actual))
	require.Equal(t, expected, actual)
	var json bytes.Buffer
	require.NoError(t, (&jsonpb.Marshaler{OrigName: true}).Marshal(&json, &expected))
	require.Contains(t, json.String(), `"oracle_pair":"SID/PAX"`)
	actual.Reset()
	require.NoError(t, jsonpb.Unmarshal(strings.NewReader(json.String()), &actual))
	require.Equal(t, expected, actual)
	var old types.ParamsPreV606
	require.NoError(t, old.Unmarshal(data))
	require.Equal(t, expected.PriorityNormalizer, old.PriorityNormalizer)
	require.Equal(t, expected.MaximumFeePerGas, old.MaximumFeePerGas)
	oldData, err := old.Marshal()
	require.NoError(t, err)
	actual.Reset()
	require.NoError(t, actual.Unmarshal(oldData))
	require.Empty(t, actual.AllowedFeeDenoms)
	require.False(t, actual.FeeTokenEnabled)
	require.True(t, actual.MaxFeeTokenSpread.IsNil())
}

func TestFeeTokenParamsProtoDescriptor(t *testing.T) {
	compressed, _ := (&types.Params{}).Descriptor()
	reader, err := gzip.NewReader(bytes.NewReader(compressed))
	require.NoError(t, err)
	data, err := io.ReadAll(reader)
	require.NoError(t, err)
	require.NoError(t, reader.Close())
	var file descriptor.FileDescriptorProto
	require.NoError(t, proto.Unmarshal(data, &file))
	fields := file.MessageType[0].Field
	for i, name := range []string{"allowed_fee_denoms", "max_fee_token_spread", "fee_token_enabled"} {
		field := fields[len(fields)-3+i]
		require.Equal(t, name, field.GetName())
		require.Equal(t, int32(16+i), field.GetNumber())
	}
	require.Equal(t, ".paxprotocol.paxchain.evm.AllowedFeeDenom", fields[len(fields)-3].GetTypeName())
	require.Equal(t, "AllowedFeeDenom", file.MessageType[5].GetName())
}

func TestFeeTokenParamsMalformedProto(t *testing.T) {
	for _, data := range [][]byte{
		{0x80, 0x01, 0x01},
		{0x82, 0x01, 0x04, 0x0a, 0x01},
		{0x82, 0x01, 0x02, 0x08, 0x01},
		{0x88, 0x01, 0x00},
		{0x8a, 0x01, 0x01},
		{0x92, 0x01, 0x00},
		{0x90, 0x01, 0x80},
	} {
		var params types.Params
		require.Error(t, params.Unmarshal(data))
	}
}

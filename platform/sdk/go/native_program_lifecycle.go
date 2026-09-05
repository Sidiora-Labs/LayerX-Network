package layerx

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
)

type NativeProgramDeploy struct {
	ProgramID [32]byte
	GuestABI  uint16
	Policy    byte
	Authority [32]byte
	NewHash   [32]byte
	Interface []byte
	Wasm      []byte
}

type NativeProgramUpgrade struct {
	ProgramID      [32]byte
	GuestABI       uint16
	OldHash        [32]byte
	NewHash        [32]byte
	MigrationHook  []byte
	ClearInterface bool
	Interface      []byte
	Wasm           []byte
}

type NativeProgramWindDown struct {
	ProgramID     [32]byte
	Operation     byte
	Account       [32]byte
	Asset         [32]byte
	Destination   [32]byte
	Seed          []byte
	ExitProgram   [32]byte
	DeadlineBatch uint64
}

func lifecycleInvalid() error { return newSDKError(ErrorInvalidArgument, RetryNever) }

func lifecycleCodeValid(program [32]byte, abi uint16, hash [32]byte, wasm []byte) bool {
	return program != ([32]byte{}) && abi >= 1 && abi <= 3 && len(wasm) >= 8 && len(wasm) <= 1048576 && bytes.Equal(wasm[:8], []byte{0, 97, 115, 109, 1, 0, 0, 0}) && sha256.Sum256(wasm) == hash
}

func EncodeNativeProgramDeploy(value NativeProgramDeploy) ([]byte, error) {
	if !lifecycleCodeValid(value.ProgramID, value.GuestABI, value.NewHash, value.Wasm) || value.Policy > 1 || (value.Policy == 0) != (value.Authority == ([32]byte{})) || len(value.Interface) > 952 || (value.Interface != nil && len(value.Interface) == 0) {
		return nil, lifecycleInvalid()
	}
	fixed := 104
	if value.Interface != nil {
		fixed = 108
	}
	out := make([]byte, fixed)
	copy(out, value.ProgramID[:])
	binary.BigEndian.PutUint16(out[32:], value.GuestABI)
	out[34] = value.Policy
	copy(out[36:], value.Authority[:])
	copy(out[68:], value.NewHash[:])
	binary.BigEndian.PutUint32(out[100:], uint32(len(value.Wasm)))
	if value.Interface != nil {
		binary.BigEndian.PutUint32(out[104:], uint32(len(value.Interface)))
		out = append(out, value.Interface...)
	}
	return append(out, value.Wasm...), nil
}

func DecodeNativeProgramDeploy(payload []byte) (NativeProgramDeploy, error) {
	var value NativeProgramDeploy
	if len(payload) < 104 || payload[35] != 0 {
		return value, lifecycleInvalid()
	}
	copy(value.ProgramID[:], payload[:32])
	value.GuestABI = binary.BigEndian.Uint16(payload[32:])
	value.Policy = payload[34]
	copy(value.Authority[:], payload[36:68])
	copy(value.NewHash[:], payload[68:100])
	wasmLength := uint64(binary.BigEndian.Uint32(payload[100:]))
	offset := 104
	if wasmLength != uint64(len(payload)-104) {
		if len(payload) < 108 {
			return value, lifecycleInvalid()
		}
		length := uint64(binary.BigEndian.Uint32(payload[104:]))
		if length == 0 || length > 952 || 108+length+wasmLength != uint64(len(payload)) {
			return value, lifecycleInvalid()
		}
		value.Interface = append([]byte{}, payload[108:108+int(length)]...)
		offset = 108 + int(length)
	}
	value.Wasm = append([]byte{}, payload[offset:]...)
	encoded, err := EncodeNativeProgramDeploy(value)
	if err != nil || !bytes.Equal(encoded, payload) {
		return NativeProgramDeploy{}, lifecycleInvalid()
	}
	return value, nil
}

func EncodeNativeProgramUpgrade(value NativeProgramUpgrade) ([]byte, error) {
	if !lifecycleCodeValid(value.ProgramID, value.GuestABI, value.NewHash, value.Wasm) || len(value.MigrationHook) > 65535 || len(value.Interface) > 952 || (value.ClearInterface && value.Interface == nil) || (value.Interface != nil && len(value.Interface) == 0 && !value.ClearInterface) {
		return nil, lifecycleInvalid()
	}
	fixed := 106
	if value.Interface != nil {
		fixed = 110
	}
	out := make([]byte, fixed)
	copy(out, value.ProgramID[:])
	binary.BigEndian.PutUint16(out[32:], value.GuestABI)
	if len(value.MigrationHook) != 0 {
		out[34] = 1
	}
	if value.ClearInterface {
		out[34] |= 2
	}
	copy(out[36:], value.OldHash[:])
	copy(out[68:], value.NewHash[:])
	binary.BigEndian.PutUint16(out[100:], uint16(len(value.MigrationHook)))
	binary.BigEndian.PutUint32(out[102:], uint32(len(value.Wasm)))
	if value.Interface != nil {
		binary.BigEndian.PutUint32(out[106:], uint32(len(value.Interface)))
	}
	out = append(out, value.MigrationHook...)
	out = append(out, value.Interface...)
	return append(out, value.Wasm...), nil
}

func DecodeNativeProgramUpgrade(payload []byte) (NativeProgramUpgrade, error) {
	var value NativeProgramUpgrade
	if len(payload) < 106 || payload[35] != 0 || payload[34]&0xfc != 0 {
		return value, lifecycleInvalid()
	}
	copy(value.ProgramID[:], payload[:32])
	value.GuestABI = binary.BigEndian.Uint16(payload[32:])
	copy(value.OldHash[:], payload[36:68])
	copy(value.NewHash[:], payload[68:100])
	value.ClearInterface = payload[34]&2 != 0
	hook := uint64(binary.BigEndian.Uint16(payload[100:]))
	wasm := uint64(binary.BigEndian.Uint32(payload[102:]))
	offset := 106
	if (payload[34]&1 == 0) != (hook == 0) {
		return value, lifecycleInvalid()
	}
	interfaceLength := uint64(0)
	if value.ClearInterface || hook+wasm != uint64(len(payload)-106) {
		if len(payload) < 110 {
			return value, lifecycleInvalid()
		}
		interfaceLength = uint64(binary.BigEndian.Uint32(payload[106:]))
		offset = 110
		if interfaceLength > 952 || (interfaceLength == 0 && !value.ClearInterface) || 110+hook+interfaceLength+wasm != uint64(len(payload)) {
			return value, lifecycleInvalid()
		}
	}
	value.MigrationHook = append([]byte{}, payload[offset:offset+int(hook)]...)
	offset += int(hook)
	if offset-int(hook) == 110 {
		value.Interface = append([]byte{}, payload[offset:offset+int(interfaceLength)]...)
		offset += int(interfaceLength)
	}
	value.Wasm = append([]byte{}, payload[offset:]...)
	encoded, err := EncodeNativeProgramUpgrade(value)
	if err != nil || !bytes.Equal(encoded, payload) {
		return NativeProgramUpgrade{}, lifecycleInvalid()
	}
	return value, nil
}

func EncodeNativeProgramWindDown(value NativeProgramWindDown) ([]byte, error) {
	if value.ProgramID == ([32]byte{}) {
		return nil, lifecycleInvalid()
	}
	out := append([]byte{}, value.ProgramID[:]...)
	out = append(out, value.Operation)
	switch value.Operation {
	case 1:
		if len(value.Seed) > 128 || value.ExitProgram != ([32]byte{}) || value.DeadlineBatch != 0 {
			return nil, lifecycleInvalid()
		}
		out = append(out, value.Account[:]...)
		out = append(out, value.Asset[:]...)
		out = append(out, value.Destination[:]...)
		out = binary.BigEndian.AppendUint16(out, uint16(len(value.Seed)))
		out = append(out, value.Seed...)
	case 2:
		if value.Account != ([32]byte{}) || value.Asset != ([32]byte{}) || value.Destination != ([32]byte{}) || len(value.Seed) != 0 {
			return nil, lifecycleInvalid()
		}
		out = append(out, value.ExitProgram[:]...)
		out = binary.BigEndian.AppendUint64(out, value.DeadlineBatch)
	case 3, 4:
		if value.Asset != ([32]byte{}) || value.Destination != ([32]byte{}) || value.ExitProgram != ([32]byte{}) || value.DeadlineBatch != 0 || len(value.Seed) != 0 || (value.Operation == 3 && value.Account != ([32]byte{})) {
			return nil, lifecycleInvalid()
		}
		if value.Operation == 4 {
			out = append(out, value.Account[:]...)
		}
	default:
		return nil, lifecycleInvalid()
	}
	return out, nil
}

func DecodeNativeProgramWindDown(payload []byte) (NativeProgramWindDown, error) {
	var value NativeProgramWindDown
	if len(payload) < 33 {
		return value, lifecycleInvalid()
	}
	copy(value.ProgramID[:], payload[:32])
	value.Operation = payload[32]
	switch value.Operation {
	case 1:
		if len(payload) < 131 || int(binary.BigEndian.Uint16(payload[129:]))+131 != len(payload) {
			return value, lifecycleInvalid()
		}
		copy(value.Account[:], payload[33:65])
		copy(value.Asset[:], payload[65:97])
		copy(value.Destination[:], payload[97:129])
		value.Seed = append([]byte{}, payload[131:]...)
	case 2:
		if len(payload) != 73 {
			return value, lifecycleInvalid()
		}
		copy(value.ExitProgram[:], payload[33:65])
		value.DeadlineBatch = binary.BigEndian.Uint64(payload[65:])
	case 3:
		if len(payload) != 33 {
			return value, lifecycleInvalid()
		}
	case 4:
		if len(payload) != 65 {
			return value, lifecycleInvalid()
		}
		copy(value.Account[:], payload[33:])
	default:
		return value, lifecycleInvalid()
	}
	encoded, err := EncodeNativeProgramWindDown(value)
	if err != nil || !bytes.Equal(encoded, payload) {
		return NativeProgramWindDown{}, lifecycleInvalid()
	}
	return value, nil
}

type NativeProgramLifecycleRequest struct {
	ordinal        uint16
	payload        []byte
	signedActivity []byte
	binding        programCallBinding
}

func NewNativeProgramLifecycleRequest(ordinal uint16, payload, signedActivity []byte) (NativeProgramLifecycleRequest, error) {
	var err error
	switch ordinal {
	case 1:
		_, err = DecodeNativeProgramDeploy(payload)
	case 2:
		_, err = DecodeNativeProgramUpgrade(payload)
	case 7:
		_, err = DecodeNativeProgramWindDown(payload)
	default:
		err = lifecycleInvalid()
	}
	if err != nil {
		return NativeProgramLifecycleRequest{}, err
	}
	binding, err := bindNativeProgramActivity(ordinal, payload, signedActivity)
	if err != nil {
		return NativeProgramLifecycleRequest{}, err
	}
	return NativeProgramLifecycleRequest{ordinal, append([]byte{}, payload...), append([]byte{}, signedActivity...), binding}, nil
}

func bindNativeProgramActivity(ordinal uint16, expected, signed []byte) (programCallBinding, error) {
	invalid := func() (programCallBinding, error) { return programCallBinding{}, lifecycleInvalid() }
	if len(signed) == 0 || len(signed) > 1048576 {
		return invalid()
	}
	decoder := wireDecoder{value: signed}
	if decoder.u16() != 3 || decoder.u16() != 0x1001 || decoder.u8() != 12 || decoder.u8() != 1 || decoder.u16() != 3 || decoder.u8() != 2 {
		return invalid()
	}
	_ = decoder.u32()
	if decoder.u8() != 3 || decoder.u32() != 0x00090000|uint32(ordinal) || decoder.u8() != 4 {
		return invalid()
	}
	_ = decoder.bounded(255)
	if decoder.u8() != 5 {
		return invalid()
	}
	_ = decoder.bounded(524288)
	if decoder.u8() != 6 {
		return invalid()
	}
	_ = decoder.u64()
	if decoder.u8() != 7 {
		return invalid()
	}
	before := decoder.u64()
	after := decoder.u64()
	if after < before || decoder.u8() != 8 {
		return invalid()
	}
	key := decoder.array32()
	if decoder.u8() != 9 {
		return invalid()
	}
	_ = decoder.u128()
	if decoder.u8() != 10 {
		return invalid()
	}
	hash := decoder.array32()
	if decoder.u8() != 11 {
		return invalid()
	}
	payload := decoder.bounded(524288)
	if decoder.u8() != 12 {
		return invalid()
	}
	_ = decoder.bounded(128)
	if decoder.failed || decoder.offset != len(signed) || !bytes.Equal(payload, expected) || domainDigest([]byte("LXP/v1/payload-hash\x00"), payload) != hash {
		return invalid()
	}
	return programCallBinding{ActivityID: domainDigest([]byte("LXP/v1/activity-id\x00"), signed), IdempotencyKey: key, NotBefore: before, NotAfter: after}, nil
}

func (programs *Programs) Deploy(ctx context.Context, value NativeProgramDeploy, signed []byte, key IdempotencyKey) (VerifiedReceipt, error) {
	payload, err := EncodeNativeProgramDeploy(value)
	if err != nil {
		return VerifiedReceipt{}, err
	}
	return programs.lifecycle(ctx, 1, payload, signed, key)
}

func (programs *Programs) Upgrade(ctx context.Context, value NativeProgramUpgrade, signed []byte, key IdempotencyKey) (VerifiedReceipt, error) {
	payload, err := EncodeNativeProgramUpgrade(value)
	if err != nil {
		return VerifiedReceipt{}, err
	}
	return programs.lifecycle(ctx, 2, payload, signed, key)
}

func (programs *Programs) WindDown(ctx context.Context, value NativeProgramWindDown, signed []byte, key IdempotencyKey) (VerifiedReceipt, error) {
	payload, err := EncodeNativeProgramWindDown(value)
	if err != nil {
		return VerifiedReceipt{}, err
	}
	return programs.lifecycle(ctx, 7, payload, signed, key)
}

func (programs *Programs) lifecycle(ctx context.Context, ordinal uint16, payload, signed []byte, key IdempotencyKey) (VerifiedReceipt, error) {
	request, err := NewNativeProgramLifecycleRequest(ordinal, payload, signed)
	if err != nil {
		return VerifiedReceipt{}, err
	}
	if programs.protocolVersion != 3 || !canonicalProgramKey(key) || key.String() != hex.EncodeToString(request.binding.IdempotencyKey[:]) {
		return VerifiedReceipt{}, lifecycleInvalid()
	}
	operation := map[uint16]string{1: "program.deploy", 2: "program.upgrade", 7: "program.wind-down"}[ordinal]
	var raw json.RawMessage
	err = programs.client.Agent(ctx, AgentOperation(operation), map[string]string{"signed_activity": hex.EncodeToString(request.signedActivity), "payload": hex.EncodeToString(request.payload)}, &raw, CallOptions{IdempotencyKey: key})
	if err != nil {
		return VerifiedReceipt{}, err
	}
	return decodeLifecycleSubmission(raw, request.binding.ActivityID, programs.trustedSequencerPublicKey)
}

func decodeLifecycleSubmission(raw json.RawMessage, activity, sequencer [32]byte) (VerifiedReceipt, error) {
	invalid := func() (VerifiedReceipt, error) {
		return VerifiedReceipt{}, newSDKError(ErrorUnknownOutcome, RetryUnknownOutcome)
	}
	var fields map[string]json.RawMessage
	if decodeStrict(raw, &fields) != nil || !exactFields(fields, "state", "activity_id", "receipt", "terminal_payload", "call_graph") {
		return invalid()
	}
	values := map[string]string{}
	for name, field := range fields {
		var value string
		if json.Unmarshal(field, &value) != nil {
			return invalid()
		}
		values[name] = value
	}
	if values["activity_id"] != hex.EncodeToString(activity[:]) || values["terminal_payload"] != "" || values["call_graph"] != "" || (values["state"] != "executed" && values["state"] != "refused") || !canonicalLowerHex(values["receipt"], len(values["receipt"])/2) {
		return invalid()
	}
	canonical, err := hex.DecodeString(values["receipt"])
	if err != nil {
		return invalid()
	}
	verified, err := VerifyProgramLifecycleReceipt(canonical, activity, sequencer)
	if err != nil || (values["state"] == "executed") != (verified.Receipt.ResultCode == 0) {
		return invalid()
	}
	return verified, nil
}

func (programs *Programs) LifecycleReceipt(ctx context.Context, request NativeProgramLifecycleRequest) (VerifiedReceipt, error) {
	bound, err := NewNativeProgramLifecycleRequest(request.ordinal, request.payload, request.signedActivity)
	if err != nil || programs.protocolVersion != 3 {
		return VerifiedReceipt{}, lifecycleInvalid()
	}
	key := hex.EncodeToString(bound.binding.IdempotencyKey[:])
	activity := hex.EncodeToString(bound.binding.ActivityID[:])
	raw, err := programs.raw(ctx, "program.receipt", false, map[string]string{"idempotency_key": key, "expected_activity_id": activity, "requested_verification_level": "sequencer-signed"}, CallOptions{PathParameters: map[string]string{"idempotency_key": key}})
	if err != nil {
		return VerifiedReceipt{}, err
	}
	var fields map[string]json.RawMessage
	var actual, receipt string
	if decodeStrict(raw, &fields) != nil || !exactFields(fields, "activity_id", "receipt") || json.Unmarshal(fields["activity_id"], &actual) != nil || actual != activity || json.Unmarshal(fields["receipt"], &receipt) != nil || len(receipt) > 2097152 || !canonicalLowerHex(receipt, len(receipt)/2) {
		return VerifiedReceipt{}, newSDKError(ErrorVerificationFailure, RetryNever)
	}
	canonical, err := hex.DecodeString(receipt)
	if err != nil {
		return VerifiedReceipt{}, err
	}
	return VerifyProgramLifecycleReceipt(canonical, bound.binding.ActivityID, programs.trustedSequencerPublicKey)
}

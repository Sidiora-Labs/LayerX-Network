package layerx

import "encoding/binary"

type NativeProgramCall struct {
	ProgramID         [32]byte
	GuestABI          uint16
	Entrypoint        string
	Calldata          []byte
	Capabilities      []byte
	AccessDeclaration []byte
	ResponseCapacity  uint32
	Resources         [7]uint64
}

func EncodeNativeProgramCall(call NativeProgramCall) ([]byte, error) {
	if call.ProgramID == ([32]byte{}) || (call.GuestABI != 1 && call.GuestABI != 2) || len(call.Entrypoint) == 0 || len(call.Entrypoint) > 128 || len(call.Calldata) > 1048576 || len(call.Capabilities) > 65535 || len(call.AccessDeclaration) > 1048576 || call.ResponseCapacity > 1048576 {
		return nil, newSDKError(ErrorInvalidArgument, RetryNever)
	}
	for _, b := range []byte(call.Entrypoint) {
		if !(b >= 'a' && b <= 'z' || b >= 'A' && b <= 'Z' || b >= '0' && b <= '9' || b == '_' || b == '.') {
			return nil, newSDKError(ErrorInvalidArgument, RetryNever)
		}
	}
	out := make([]byte, 106, 106+len(call.Entrypoint)+len(call.Calldata)+len(call.Capabilities)+len(call.AccessDeclaration))
	copy(out, call.ProgramID[:])
	binary.BigEndian.PutUint16(out[32:], call.GuestABI)
	binary.BigEndian.PutUint16(out[34:], uint16(len(call.Entrypoint)))
	binary.BigEndian.PutUint32(out[36:], uint32(len(call.Calldata)))
	binary.BigEndian.PutUint16(out[40:], uint16(len(call.Capabilities)))
	binary.BigEndian.PutUint32(out[42:], uint32(len(call.AccessDeclaration)))
	binary.BigEndian.PutUint32(out[46:], call.ResponseCapacity)
	for i, v := range call.Resources {
		binary.BigEndian.PutUint64(out[50+i*8:], v)
	}
	for _, body := range [][]byte{[]byte(call.Entrypoint), call.Calldata, call.Capabilities, call.AccessDeclaration} {
		out = append(out, body...)
	}
	return out, nil
}

func DecodeNativeProgramCall(payload []byte) (NativeProgramCall, error) {
	var call NativeProgramCall
	invalid := newSDKError(ErrorInvalidArgument, RetryNever)
	if len(payload) < 106 {
		return call, invalid
	}
	lengths := []uint64{uint64(binary.BigEndian.Uint16(payload[34:])), uint64(binary.BigEndian.Uint32(payload[36:])), uint64(binary.BigEndian.Uint16(payload[40:])), uint64(binary.BigEndian.Uint32(payload[42:]))}
	total := uint64(106)
	for _, n := range lengths {
		total += n
	}
	if total != uint64(len(payload)) {
		return call, invalid
	}
	copy(call.ProgramID[:], payload[:32])
	call.GuestABI = binary.BigEndian.Uint16(payload[32:])
	call.ResponseCapacity = binary.BigEndian.Uint32(payload[46:])
	for i := range call.Resources {
		call.Resources[i] = binary.BigEndian.Uint64(payload[50+i*8:])
	}
	bodies := make([][]byte, 4)
	offset := 106
	for i, n := range lengths {
		bodies[i] = append([]byte(nil), payload[offset:offset+int(n)]...)
		offset += int(n)
	}
	call.Entrypoint = string(bodies[0])
	call.Calldata = bodies[1]
	call.Capabilities = bodies[2]
	call.AccessDeclaration = bodies[3]
	if _, err := EncodeNativeProgramCall(call); err != nil {
		return NativeProgramCall{}, err
	}
	return call, nil
}

type NativeProgramRequest struct {
	Call           NativeProgramCall
	FeeLimit       Uint128
	SignedActivity []byte
}

func (request NativeProgramRequest) ProgramCall() (ProgramCall, error) {
	if _, err := EncodeNativeProgramCall(request.Call); err != nil {
		return ProgramCall{}, err
	}
	call := ProgramCall{NativeCall: &request.Call, ProgramID: request.Call.ProgramID, Calldata: request.Call.Calldata, Budget: ProgramBudget{Fuel: request.Call.Resources[0], FeeLimit: request.FeeLimit}, SignedActivity: request.SignedActivity}
	if err := validateProgramCall(call); err != nil {
		return ProgramCall{}, err
	}
	return call, nil
}

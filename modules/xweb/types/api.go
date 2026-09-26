package types

// The api payload codec.
//
// A request of kind KindApi carries one api payload: the HTTP call a contract
// wants made, its public headers, an optional body, the RFC 6901 JSON
// pointers that select the attested fields, the attestation level and one
// credential envelope per attestor that should hold the credential. The bytes
// are Solidity abi.encodePacked of these fields in this order, every length
// and count an unsigned big-endian integer, with no padding and nothing after
// the last envelope:
//
//	uint8   version        1
//	uint8   method         1 GET, 2 POST
//	uint8   level          0 majority, 1 single
//	address attestor       20 bytes: the named attestor under single, zero under majority
//	uint16  urlLength      then urlLength bytes of URL
//	uint8   headerCount    then per header: uint16 nameLength, name, uint16 valueLength, value
//	uint16  bodyLength     then bodyLength bytes of body
//	uint8   pointerCount   then per pointer: uint16 length, the pointer's UTF-8 bytes
//	uint8   envelopeCount  then per envelope: uint16 length, the envelope (envelope.go)
//
// Rules, each refused by name:
//   - the URL is 1 to MaxApiURLBytes printable ASCII bytes with no '#',
//     starts with "https://", and its authority (the bytes after "https://" up
//     to the first '/' or '?') is non-empty and made of lower-case letters,
//     digits and ".-:[]" only; "https://" plus the authority is the origin a
//     credential envelope is bound to;
//   - at most MaxApiHeaders public headers, each name 1 to
//     MaxApiHeaderNameBytes RFC 9110 token characters, never repeated in any
//     letter case and never Host, Content-Length, Transfer-Encoding,
//     Connection, Keep-Alive, TE, Trailer or Upgrade, each value at most
//     MaxApiHeaderValueBytes of printable ASCII, space or tab;
//   - the body is at most MaxApiBodyBytes and empty under GET;
//   - at most MaxApiPointers pointers, each at most MaxApiPointerBytes of
//     valid UTF-8, empty or starting with '/', every '~' followed by '0' or
//     '1', never repeated; an empty pointer list attests the raw bounded
//     body;
//   - under single the attestor is non-zero and every envelope is addressed
//     to it; under majority the attestor is zero;
//   - at most MaxAttestors envelopes, each well formed, no two addressed to
//     the same attestor.
//
// At request time the keeper also refuses a named attestor or an envelope
// address outside the registered set, and more envelopes than registered
// attestors (CheckAttestors). The level reaches the attestation through the
// payload hash: the preimage keeps its 188 bytes. testdata/api-vectors.json
// pins the bytes and their keccak256 for every other implementation.

import (
	"fmt"
	"math"
	"net/url"
	"strings"
	"unicode/utf8"
)

const (
	// ApiVersion is the first byte of every api payload.
	ApiVersion uint8 = 1

	MethodGet  uint8 = 1
	MethodPost uint8 = 2

	MaxApiURLBytes         = 2048
	MaxApiHeaders          = 16
	MaxApiHeaderNameBytes  = 64
	MaxApiHeaderValueBytes = 1024
	MaxApiBodyBytes        = 4096
	MaxApiPointers         = 16
	MaxApiPointerBytes     = 256

	// MaxCredentialHeaders bounds the headers one credential carries.
	MaxCredentialHeaders = 8

	httpsPrefix = "https://"
)

// restrictedHeaders are the header names the sidecar sets itself.
var restrictedHeaders = map[string]bool{
	"host": true, "content-length": true, "transfer-encoding": true, "connection": true,
	"keep-alive": true, "te": true, "trailer": true, "upgrade": true,
}

// ApiHeader is one HTTP request header.
type ApiHeader struct {
	Name  string
	Value string
}

// ApiPayload is the decoded payload of a KindApi request.
type ApiPayload struct {
	Method    uint8
	Level     uint8
	Attestor  Address20
	URL       string
	Headers   []ApiHeader
	Body      []byte
	Pointers  []string
	Envelopes []Envelope
}

func apiError(format string, args ...interface{}) error {
	return ErrInvalidApi.Wrapf(format, args...)
}

// MethodName is "GET" or "POST".
func MethodName(method uint8) (string, error) {
	switch method {
	case MethodGet:
		return "GET", nil
	case MethodPost:
		return "POST", nil
	default:
		return "", apiError("method %d, want 1 (GET) or 2 (POST)", method)
	}
}

// Validate checks every rule of the codec that needs no chain state.
func (p ApiPayload) Validate() error {
	if _, err := MethodName(p.Method); err != nil {
		return err
	}
	if err := validateLevel(KindApi, p.Level, p.Attestor); err != nil {
		return err
	}
	if _, err := apiOrigin(p.URL); err != nil {
		return err
	}
	if err := validateHeaders("header", p.Headers, MaxApiHeaders); err != nil {
		return err
	}
	if len(p.Body) > MaxApiBodyBytes {
		return apiError("body is %d bytes, bound %d", len(p.Body), MaxApiBodyBytes)
	}
	if p.Method == MethodGet && len(p.Body) != 0 {
		return apiError("GET carries a %d-byte body", len(p.Body))
	}
	if err := validatePointers(p.Pointers); err != nil {
		return err
	}
	if len(p.Envelopes) > MaxAttestors {
		return apiError("%d envelopes, bound %d", len(p.Envelopes), MaxAttestors)
	}
	addressed := map[Address20]bool{}
	for index, envelope := range p.Envelopes {
		if err := envelope.validate(); err != nil {
			return apiError("envelope %d: %v", index, err)
		}
		if addressed[envelope.Attestor] {
			return apiError("envelope %d repeats attestor %s", index, envelope.Attestor.Hex())
		}
		addressed[envelope.Attestor] = true
		if p.Level == LevelSingle && envelope.Attestor != p.Attestor {
			return apiError("envelope %d is addressed to %s, the single level names %s", index,
				envelope.Attestor.Hex(), p.Attestor.Hex())
		}
	}
	return nil
}

// Origin is "https://" and the URL's authority: what every envelope of the
// payload is bound to.
func (p ApiPayload) Origin() (string, error) { return apiOrigin(p.URL) }

func apiOrigin(raw string) (string, error) {
	if len(raw) == 0 || len(raw) > MaxApiURLBytes {
		return "", apiError("url is %d bytes, want 1 to %d", len(raw), MaxApiURLBytes)
	}
	for i := 0; i < len(raw); i++ {
		if raw[i] < 0x21 || raw[i] > 0x7e {
			return "", apiError("url byte %d is 0x%02x, not printable ASCII", i, raw[i])
		}
	}
	if strings.IndexByte(raw, '#') >= 0 {
		return "", apiError("url carries a fragment")
	}
	if !strings.HasPrefix(raw, httpsPrefix) {
		return "", apiError("url %q is not https", raw)
	}
	rest := raw[len(httpsPrefix):]
	end := strings.IndexAny(rest, "/?")
	if end < 0 {
		end = len(rest)
	}
	authority := rest[:end]
	if authority == "" {
		return "", apiError("url %q has no host", raw)
	}
	for i := 0; i < len(authority); i++ {
		c := authority[i]
		if !(c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || strings.IndexByte(".-:[]", c) >= 0) {
			return "", apiError("url authority %q holds %q: only lower-case letters, digits and .-:[] are accepted",
				authority, c)
		}
	}
	parsed, err := url.Parse(raw)
	if err != nil {
		return "", apiError("url %q: %v", raw, err)
	}
	if parsed.Hostname() == "" {
		return "", apiError("url %q has no host", raw)
	}
	return httpsPrefix + authority, nil
}

func isTokenChar(c byte) bool {
	return c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' ||
		strings.IndexByte("!#$%&'*+-.^_`|~", c) >= 0
}

// validateHeaders checks names, values, repeats and the count; what names
// the refused part ("header" or "credential header").
func validateHeaders(what string, headers []ApiHeader, bound int) error {
	if len(headers) > bound {
		return apiError("%d %ss, bound %d", len(headers), what, bound)
	}
	seen := map[string]bool{}
	for index, header := range headers {
		if len(header.Name) == 0 || len(header.Name) > MaxApiHeaderNameBytes {
			return apiError("%s %d name is %d bytes, want 1 to %d", what, index, len(header.Name), MaxApiHeaderNameBytes)
		}
		for i := 0; i < len(header.Name); i++ {
			if !isTokenChar(header.Name[i]) {
				return apiError("%s %d name %q holds %q, not a token character", what, index, header.Name, header.Name[i])
			}
		}
		lower := strings.ToLower(header.Name)
		if restrictedHeaders[lower] {
			return apiError("%s %d is %s, which the sidecar sets itself", what, index, header.Name)
		}
		if seen[lower] {
			return apiError("%s %d repeats %s", what, index, header.Name)
		}
		seen[lower] = true
		if len(header.Value) > MaxApiHeaderValueBytes {
			return apiError("%s %s value is %d bytes, bound %d", what, header.Name, len(header.Value),
				MaxApiHeaderValueBytes)
		}
		for i := 0; i < len(header.Value); i++ {
			if c := header.Value[i]; c != '\t' && (c < 0x20 || c > 0x7e) {
				return apiError("%s %s value byte %d is 0x%02x", what, header.Name, i, c)
			}
		}
	}
	return nil
}

func validatePointers(pointers []string) error {
	if len(pointers) > MaxApiPointers {
		return apiError("%d pointers, bound %d", len(pointers), MaxApiPointers)
	}
	seen := map[string]bool{}
	for index, pointer := range pointers {
		if len(pointer) > MaxApiPointerBytes {
			return apiError("pointer %d is %d bytes, bound %d", index, len(pointer), MaxApiPointerBytes)
		}
		if !utf8.ValidString(pointer) {
			return apiError("pointer %d is not UTF-8", index)
		}
		if pointer != "" && pointer[0] != '/' {
			return apiError("pointer %q does not start with /", pointer)
		}
		for i := 0; i < len(pointer); i++ {
			if pointer[i] == '~' && (i+1 == len(pointer) || (pointer[i+1] != '0' && pointer[i+1] != '1')) {
				return apiError("pointer %q has a ~ not followed by 0 or 1", pointer)
			}
		}
		if seen[pointer] {
			return apiError("pointer %q is repeated", pointer)
		}
		seen[pointer] = true
	}
	return nil
}

// CheckAttestors refuses a payload whose named attestor or envelope address is
// not in the registered set, or that carries more envelopes than the set has
// attestors.
func (p ApiPayload) CheckAttestors(set AttestorSet) error {
	if p.Level == LevelSingle && !set.Has(p.Attestor) {
		return ErrUnknownAttestor.Wrapf("the single level names %s", p.Attestor.Hex())
	}
	if len(p.Envelopes) > len(set.Attestors) {
		return apiError("%d envelopes for %d registered attestors", len(p.Envelopes), len(set.Attestors))
	}
	for index, envelope := range p.Envelopes {
		if !set.Has(envelope.Attestor) {
			return ErrUnknownAttestor.Wrapf("envelope %d is addressed to %s", index, envelope.Attestor.Hex())
		}
	}
	return nil
}

type apiWriter struct {
	out []byte
	err error
}

func (w *apiWriter) u8(field string, n int) {
	if w.err == nil && (n < 0 || n > math.MaxUint8) {
		w.err = apiError("%s count %d does not fit one byte", field, n)
		return
	}
	w.out = append(w.out, byte(n))
}

func (w *apiWriter) field(name string, data []byte) {
	if w.err != nil {
		return
	}
	if len(data) > math.MaxUint16 {
		w.err = apiError("%s is %d bytes, over the uint16 length", name, len(data))
		return
	}
	w.out = append(w.out, byte(len(data)>>8), byte(len(data)&0xff))
	w.out = append(w.out, data...)
}

func (w *apiWriter) headers(what string, headers []ApiHeader) {
	w.u8(what+"s", len(headers))
	for _, header := range headers {
		w.field(what+" name", []byte(header.Name))
		w.field(what+" value", []byte(header.Value))
	}
}

// Encode validates p and returns its api payload bytes.
func (p ApiPayload) Encode() ([]byte, error) {
	if err := p.Validate(); err != nil {
		return nil, err
	}
	w := &apiWriter{}
	w.out = append(w.out, ApiVersion, p.Method, p.Level)
	w.out = append(w.out, p.Attestor[:]...)
	w.field("url", []byte(p.URL))
	w.headers("header", p.Headers)
	w.field("body", p.Body)
	w.u8("pointers", len(p.Pointers))
	for _, pointer := range p.Pointers {
		w.field("pointer", []byte(pointer))
	}
	w.u8("envelopes", len(p.Envelopes))
	for _, envelope := range p.Envelopes {
		w.field("envelope", envelope.Bytes())
	}
	return w.out, w.err
}

type apiReader struct {
	data []byte
	at   int
}

func (r *apiReader) take(field string, n int) ([]byte, error) {
	if len(r.data)-r.at < n {
		return nil, apiError("payload ends inside %s at byte %d", field, r.at)
	}
	out := r.data[r.at : r.at+n]
	r.at += n
	return out, nil
}

func (r *apiReader) u8(field string) (int, error) {
	b, err := r.take(field, 1)
	if err != nil {
		return 0, err
	}
	return int(b[0]), nil
}

func (r *apiReader) field(name string) ([]byte, error) {
	length, err := r.take(name+" length", 2)
	if err != nil {
		return nil, err
	}
	return r.take(name, int(length[0])<<8|int(length[1]))
}

func (r *apiReader) headers(what string) ([]ApiHeader, error) {
	count, err := r.u8(what + " count")
	if err != nil {
		return nil, err
	}
	headers := make([]ApiHeader, 0, count)
	for i := 0; i < count; i++ {
		name, err := r.field(fmt.Sprintf("%s %d name", what, i))
		if err != nil {
			return nil, err
		}
		value, err := r.field(fmt.Sprintf("%s %d value", what, i))
		if err != nil {
			return nil, err
		}
		headers = append(headers, ApiHeader{Name: string(name), Value: string(value)})
	}
	return headers, nil
}

// DecodeApiPayload decodes raw and validates it; nothing may follow the last
// envelope.
func DecodeApiPayload(raw []byte) (ApiPayload, error) {
	r := &apiReader{data: raw}
	head, err := r.take("the version, method, level and attestor", 3+20)
	if err != nil {
		return ApiPayload{}, err
	}
	if head[0] != ApiVersion {
		return ApiPayload{}, apiError("version %d, want %d", head[0], ApiVersion)
	}
	p := ApiPayload{Method: head[1], Level: head[2]}
	copy(p.Attestor[:], head[3:23])
	rawURL, err := r.field("url")
	if err != nil {
		return ApiPayload{}, err
	}
	p.URL = string(rawURL)
	if p.Headers, err = r.headers("header"); err != nil {
		return ApiPayload{}, err
	}
	body, err := r.field("body")
	if err != nil {
		return ApiPayload{}, err
	}
	p.Body = append([]byte(nil), body...)
	count, err := r.u8("pointer count")
	if err != nil {
		return ApiPayload{}, err
	}
	for i := 0; i < count; i++ {
		pointer, err := r.field(fmt.Sprintf("pointer %d", i))
		if err != nil {
			return ApiPayload{}, err
		}
		p.Pointers = append(p.Pointers, string(pointer))
	}
	if count, err = r.u8("envelope count"); err != nil {
		return ApiPayload{}, err
	}
	for i := 0; i < count; i++ {
		raw, err := r.field(fmt.Sprintf("envelope %d", i))
		if err != nil {
			return ApiPayload{}, err
		}
		envelope, err := ParseEnvelope(raw)
		if err != nil {
			return ApiPayload{}, apiError("envelope %d: %v", i, err)
		}
		p.Envelopes = append(p.Envelopes, envelope)
	}
	if rest := len(r.data) - r.at; rest != 0 {
		return ApiPayload{}, apiError("%d bytes follow the last envelope", rest)
	}
	if err := p.Validate(); err != nil {
		return ApiPayload{}, err
	}
	return p, nil
}

// EncodeCredential is the plaintext a credential envelope carries: uint8
// count, then per header uint16 nameLength, name, uint16 valueLength, value,
// under the public header rules, with 1 to MaxCredentialHeaders headers and
// at most MaxCredentialBytes in all.
func EncodeCredential(headers []ApiHeader) ([]byte, error) {
	if len(headers) == 0 {
		return nil, apiError("credential carries no header")
	}
	if err := validateHeaders("credential header", headers, MaxCredentialHeaders); err != nil {
		return nil, err
	}
	w := &apiWriter{}
	w.headers("credential header", headers)
	if w.err != nil {
		return nil, w.err
	}
	if len(w.out) > MaxCredentialBytes {
		return nil, apiError("credential is %d bytes, bound %d", len(w.out), MaxCredentialBytes)
	}
	return w.out, nil
}

// DecodeCredential decodes an opened envelope's plaintext and refuses a
// credential header whose name repeats one of the payload's public headers.
func DecodeCredential(plaintext []byte, public []ApiHeader) ([]ApiHeader, error) {
	if len(plaintext) > MaxCredentialBytes {
		return nil, apiError("credential is %d bytes, bound %d", len(plaintext), MaxCredentialBytes)
	}
	r := &apiReader{data: plaintext}
	headers, err := r.headers("credential header")
	if err != nil {
		return nil, err
	}
	if rest := len(r.data) - r.at; rest != 0 {
		return nil, apiError("%d bytes follow the last credential header", rest)
	}
	if len(headers) == 0 {
		return nil, apiError("credential carries no header")
	}
	if err := validateHeaders("credential header", headers, MaxCredentialHeaders); err != nil {
		return nil, err
	}
	for _, header := range headers {
		for _, other := range public {
			if strings.EqualFold(header.Name, other.Name) {
				return nil, apiError("credential header %s repeats a public header", header.Name)
			}
		}
	}
	return headers, nil
}

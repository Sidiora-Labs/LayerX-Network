// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {XWEB_CONTRACT} from "../precompiles/IXWeb.sol";

/// An HTTP call a contract asks the xweb precompile to make, being built.
struct ApiCall {
    uint8 method;
    uint8 level;
    address attestor;
    bytes url;
    uint8 headerCount;
    bytes headers;
    bytes body;
    uint8 pointerCount;
    bytes pointers;
    uint8 envelopeCount;
    bytes envelopes;
}

using XWebApi for ApiCall global;

/// Builds an api request (kind 3) and submits it through the xweb precompile:
///
///     uint64 id = XWebApi.get("https://paxeer.app/api/v1/price?asset=PAX").select("/data/price").submit(200_000);
///
/// The payload is abi.encodePacked of: uint8 version 1, uint8 method (1 GET,
/// 2 POST), uint8 level (0 majority, 1 single), the 20-byte named attestor
/// (zero under majority), uint16 url length and url, uint8 header count and
/// per header uint16 name length, name, uint16 value length, value, uint16 body
/// length and body, uint8 pointer count and per pointer uint16 length and the
/// pointer, uint8 envelope count and per envelope uint16 length and the
/// envelope; every length big-endian. It is exactly what modules/xweb decodes;
/// the module refuses a payload that breaks one of its rules at request time.
///
/// select takes an RFC 6901 JSON pointer; with no pointer the attested answer
/// is the raw bounded body. withCredential takes an envelope sealed off chain
/// to one attestor's public key from getAttestors. single names the one
/// attestor that makes the call and signs the answer; without it a majority
/// of the attestors must agree.
library XWebApi {
    uint8 internal constant KIND_API = 3;
    uint8 internal constant VERSION = 1;
    uint8 internal constant METHOD_GET = 1;
    uint8 internal constant METHOD_POST = 2;
    uint8 internal constant LEVEL_MAJORITY = 0;
    uint8 internal constant LEVEL_SINGLE = 1;

    error FieldTooLong(string field, uint256 length);
    error TooMany(string field);
    error ZeroAttestor();

    function get(string memory url) internal pure returns (ApiCall memory call) {
        call.method = METHOD_GET;
        call.url = bytes(url);
    }

    function post(string memory url, bytes memory body) internal pure returns (ApiCall memory call) {
        call.method = METHOD_POST;
        call.url = bytes(url);
        call.body = body;
    }

    /// Adds a public request header. A secret belongs in withCredential.
    function header(ApiCall memory call, string memory name, string memory value)
        internal
        pure
        returns (ApiCall memory)
    {
        call.headerCount = bump(call.headerCount, "headers");
        call.headers = abi.encodePacked(
            call.headers,
            length16(bytes(name).length, "header name"),
            name,
            length16(bytes(value).length, "header value"),
            value
        );
        return call;
    }

    /// Selects one JSON field of the response, by RFC 6901 pointer, for the
    /// attested answer.
    function select(ApiCall memory call, string memory pointer) internal pure returns (ApiCall memory) {
        call.pointerCount = bump(call.pointerCount, "pointers");
        call.pointers = abi.encodePacked(call.pointers, length16(bytes(pointer).length, "pointer"), pointer);
        return call;
    }

    /// Adds one credential envelope, sealed to one attestor.
    function withCredential(ApiCall memory call, bytes memory envelope) internal pure returns (ApiCall memory) {
        call.envelopeCount = bump(call.envelopeCount, "envelopes");
        call.envelopes = abi.encodePacked(call.envelopes, length16(envelope.length, "envelope"), envelope);
        return call;
    }

    /// Has attestor alone make the call and sign the answer.
    function single(ApiCall memory call, address attestor) internal pure returns (ApiCall memory) {
        if (attestor == address(0)) revert ZeroAttestor();
        call.level = LEVEL_SINGLE;
        call.attestor = attestor;
        return call;
    }

    /// The api payload bytes.
    function encode(ApiCall memory call) internal pure returns (bytes memory) {
        return abi.encodePacked(
            abi.encodePacked(VERSION, call.method, call.level, call.attestor),
            abi.encodePacked(length16(call.url.length, "url"), call.url, call.headerCount, call.headers),
            abi.encodePacked(length16(call.body.length, "body"), call.body),
            abi.encodePacked(call.pointerCount, call.pointers, call.envelopeCount, call.envelopes)
        );
    }

    /// Requests the call, paying the precompile's current fee from this
    /// contract's balance, and returns the request id.
    function submit(ApiCall memory call, uint64 callbackGas) internal returns (uint64) {
        return XWEB_CONTRACT.request{value: XWEB_CONTRACT.fee()}(KIND_API, encode(call), callbackGas);
    }

    function length16(uint256 length, string memory field) private pure returns (bytes2) {
        if (length > type(uint16).max) revert FieldTooLong(field, length);
        return bytes2(uint16(length));
    }

    function bump(uint8 count, string memory field) private pure returns (uint8) {
        if (count == type(uint8).max) revert TooMany(field);
        return count + 1;
    }
}

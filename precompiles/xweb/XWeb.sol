// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

address constant XWEB_PRECOMPILE_ADDRESS = 0x0000000000000000000000000000000000001019;

IXWeb constant XWEB_CONTRACT = IXWeb(XWEB_PRECOMPILE_ADDRESS);

/// Attested web data for contracts on Paxeer X Network.
///
/// A contract calls request with a kind (1 fetch, 2 search), the payload (the
/// URL or the query) and the gas its callback may use, paying exactly fee()
/// in wei. Attestors fetch independently and sign the 188-byte PAXEERX_WEB_V1
/// preimage; a submitter calls fulfil once a majority of the registered
/// attestors signed, in strictly ascending signer order. The precompile then
/// stores at most 4096 bytes of the response with its content digest and full
/// length, pays the fee to the signers' payout accounts and calls
/// onXWebResponse on the requester with at most min(callbackGas, the module's
/// maximum callback gas). A callback that reverts or runs out of gas is
/// recorded in the result and does not undo the fulfilment. A request not
/// fulfilled by its timeout height is refundable to its requester once.
///
/// Amounts are in wei; the fee is a whole number of base units (1 unit =
/// 1e12 wei). Gas = base + 16 * len(calldata after the selector), where base
/// is 3000 for views and 30000 for request, fulfil and refund; fulfil adds
/// 8000 per signature and then the callback's gas used plus 10000 to record
/// it, and refuses unless the gas left covers the callback's full bound.
interface IXWeb {
    struct Request {
        uint64 id;
        address requester;
        uint8 kind;
        bytes32 payloadHash;
        uint64 callbackGas;
        uint256 fee;
        uint64 height;
        uint64 timeoutHeight;
        uint8 status;
    }

    struct Result {
        uint64 requestId;
        bytes response;
        bytes32 contentDigest;
        uint32 fullLength;
        address[] signers;
        uint64 height;
        uint8 callback;
        uint64 callbackGasUsed;
    }

    struct Attestor {
        address signer;
        string payout;
    }

    struct Params {
        uint256 fee;
        uint32 maxPayloadBytes;
        uint64 maxCallbackGas;
        uint64 timeoutBlocks;
        bool paused;
    }

    event XWebRequested(
        uint64 indexed requestId,
        address indexed requester,
        uint8 kind,
        bytes payload,
        uint64 callbackGas,
        uint256 paid,
        uint64 timeoutHeight
    );
    event XWebFulfilled(
        uint64 indexed requestId,
        address indexed requester,
        bytes32 contentDigest,
        uint32 fullLength,
        uint8 callback,
        uint64 callbackGasUsed
    );
    event XWebRefunded(uint64 indexed requestId, address indexed requester, uint256 refunded);

    function request(uint8 kind, bytes calldata payload, uint64 callbackGas) external payable returns (uint64 requestId);

    function fulfil(
        uint64 requestId,
        bytes calldata response,
        bytes32 contentDigest,
        uint32 fullLength,
        bytes[] calldata signatures
    ) external returns (uint8 callback, uint64 callbackGasUsed);

    function refund(uint64 requestId) external;

    function getRequest(uint64 requestId) external view returns (Request memory);

    function getResult(uint64 requestId) external view returns (Result memory);

    function getAttestors() external view returns (Attestor[] memory attestors, uint32 required);

    function threshold() external view returns (uint32);

    function fee() external view returns (uint256);

    function getParams() external view returns (Params memory);
}

/// The callback a requester implements. The precompile calls it from
/// XWEB_PRECOMPILE_ADDRESS after a fulfilment.
interface IXWebConsumer {
    function onXWebResponse(uint64 requestId, bytes32 contentDigest, uint32 fullLength, bytes calldata response)
        external;
}

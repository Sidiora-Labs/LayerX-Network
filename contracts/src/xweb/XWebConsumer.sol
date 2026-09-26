// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {IXWeb, IXWebConsumer, XWEB_CONTRACT, XWEB_PRECOMPILE_ADDRESS} from "../precompiles/IXWeb.sol";

/// A contract that asks the xweb precompile for attested web data and keeps
/// the answer. Only the precompile may deliver an answer, only for a request
/// this contract issued, and only once.
contract XWebConsumer is IXWebConsumer {
    struct Answer {
        bool answered;
        bytes32 contentDigest;
        uint32 fullLength;
        bytes response;
    }

    error NotXWeb(address caller);
    error UnknownRequest(uint64 requestId);
    error AlreadyAnswered(uint64 requestId);

    event Asked(uint64 indexed requestId, uint8 kind, bytes payload, uint64 callbackGas);
    event Answered(uint64 indexed requestId, bytes32 contentDigest, uint32 fullLength, bytes response);

    mapping(uint64 => bool) public issued;
    mapping(uint64 => Answer) private answers;

    /// Requests kind (1 fetch, 2 search) of payload, forwarding msg.value as
    /// the fee, and returns the request id.
    function ask(uint8 kind, bytes calldata payload, uint64 callbackGas) external payable returns (uint64 requestId) {
        requestId = XWEB_CONTRACT.request{value: msg.value}(kind, payload, callbackGas);
        issued[requestId] = true;
        emit Asked(requestId, kind, payload, callbackGas);
    }

    function onXWebResponse(uint64 requestId, bytes32 contentDigest, uint32 fullLength, bytes calldata response)
        external
    {
        if (msg.sender != XWEB_PRECOMPILE_ADDRESS) revert NotXWeb(msg.sender);
        if (!issued[requestId]) revert UnknownRequest(requestId);
        Answer storage answer = answers[requestId];
        if (answer.answered) revert AlreadyAnswered(requestId);
        answer.answered = true;
        answer.contentDigest = contentDigest;
        answer.fullLength = fullLength;
        answer.response = response;
        emit Answered(requestId, contentDigest, fullLength, response);
    }

    function answer(uint64 requestId) external view returns (Answer memory) {
        return answers[requestId];
    }
}

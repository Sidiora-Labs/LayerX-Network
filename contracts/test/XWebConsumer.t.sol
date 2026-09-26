// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {Test} from "forge-std/Test.sol";
import {IXWeb, IXWebConsumer, XWEB_PRECOMPILE_ADDRESS} from "../src/precompiles/IXWeb.sol";
import {XWebConsumer} from "../src/xweb/XWebConsumer.sol";

/// The xweb precompile only exists in the node, so its request return is
/// mocked here; the precompile's own behaviour is covered by the Go suite.
contract XWebConsumerTest is Test {
    uint256 internal constant FEE = 1_000_003 * 1e12;
    uint64 internal constant CALLBACK_GAS = 200_000;
    uint8 internal constant KIND_FETCH = 1;
    bytes internal constant PAYLOAD = "https://paxeer.app/";
    bytes32 internal constant DIGEST = bytes32(uint256(0x2222));
    bytes internal constant RESPONSE = "Paxeer X Network";

    XWebConsumer internal consumer;

    event Asked(uint64 indexed requestId, uint8 kind, bytes payload, uint64 callbackGas);
    event Answered(uint64 indexed requestId, bytes32 contentDigest, uint32 fullLength, bytes response);

    function setUp() public {
        consumer = new XWebConsumer();
        vm.deal(address(this), 10 * FEE);
    }

    function ask(uint64 id) internal returns (uint64) {
        vm.mockCall(
            XWEB_PRECOMPILE_ADDRESS,
            FEE,
            abi.encodeCall(IXWeb.request, (KIND_FETCH, PAYLOAD, CALLBACK_GAS)),
            abi.encode(id)
        );
        vm.expectCall(XWEB_PRECOMPILE_ADDRESS, FEE, abi.encodeCall(IXWeb.request, (KIND_FETCH, PAYLOAD, CALLBACK_GAS)));
        vm.expectEmit(true, false, false, true, address(consumer));
        emit Asked(id, KIND_FETCH, PAYLOAD, CALLBACK_GAS);
        return consumer.ask{value: FEE}(KIND_FETCH, PAYLOAD, CALLBACK_GAS);
    }

    function test_AskForwardsTheFeeAndRecordsTheRequest() public {
        assertEq(ask(7), 7);
        assertTrue(consumer.issued(7));
        assertFalse(consumer.issued(8));
    }

    function test_PrecompileDeliversTheAnswerOnce() public {
        ask(7);
        vm.expectEmit(true, false, false, true, address(consumer));
        emit Answered(7, DIGEST, 4096, RESPONSE);
        vm.prank(XWEB_PRECOMPILE_ADDRESS);
        consumer.onXWebResponse(7, DIGEST, 4096, RESPONSE);

        XWebConsumer.Answer memory answer = consumer.answer(7);
        assertTrue(answer.answered);
        assertEq(answer.contentDigest, DIGEST);
        assertEq(answer.fullLength, 4096);
        assertEq(answer.response, RESPONSE);

        vm.prank(XWEB_PRECOMPILE_ADDRESS);
        vm.expectRevert(abi.encodeWithSelector(XWebConsumer.AlreadyAnswered.selector, uint64(7)));
        consumer.onXWebResponse(7, DIGEST, 4096, RESPONSE);
    }

    function test_RefusesAnAnswerFromAnyoneButThePrecompile() public {
        ask(7);
        address intruder = makeAddr("intruder");
        vm.prank(intruder);
        vm.expectRevert(abi.encodeWithSelector(XWebConsumer.NotXWeb.selector, intruder));
        consumer.onXWebResponse(7, DIGEST, 4096, RESPONSE);
        assertFalse(consumer.answer(7).answered);
    }

    function test_RefusesAnAnswerToARequestItDidNotIssue() public {
        ask(7);
        vm.prank(XWEB_PRECOMPILE_ADDRESS);
        vm.expectRevert(abi.encodeWithSelector(XWebConsumer.UnknownRequest.selector, uint64(8)));
        consumer.onXWebResponse(8, DIGEST, 4096, RESPONSE);
    }

    /// The Go suite checks that abi.json carries exactly these signatures.
    function test_InterfaceMatchesThePrecompileABI() public pure {
        assertEq(IXWeb.request.selector, bytes4(keccak256("request(uint8,bytes,uint64)")));
        assertEq(IXWeb.fulfil.selector, bytes4(keccak256("fulfil(uint64,bytes,bytes32,uint32,bytes[])")));
        assertEq(IXWeb.refund.selector, bytes4(keccak256("refund(uint64)")));
        assertEq(IXWeb.getRequest.selector, bytes4(keccak256("getRequest(uint64)")));
        assertEq(IXWeb.getResult.selector, bytes4(keccak256("getResult(uint64)")));
        assertEq(IXWeb.getAttestors.selector, bytes4(keccak256("getAttestors()")));
        assertEq(IXWeb.threshold.selector, bytes4(keccak256("threshold()")));
        assertEq(IXWeb.fee.selector, bytes4(keccak256("fee()")));
        assertEq(IXWeb.getParams.selector, bytes4(keccak256("getParams()")));
        assertEq(
            IXWebConsumer.onXWebResponse.selector, bytes4(keccak256("onXWebResponse(uint64,bytes32,uint32,bytes)"))
        );
        assertEq(
            IXWeb.XWebRequested.selector, keccak256("XWebRequested(uint64,address,uint8,bytes,uint64,uint256,uint64)")
        );
        assertEq(IXWeb.XWebFulfilled.selector, keccak256("XWebFulfilled(uint64,address,bytes32,uint32,uint8,uint64)"));
        assertEq(IXWeb.XWebRefunded.selector, keccak256("XWebRefunded(uint64,address,uint256)"));
    }
}

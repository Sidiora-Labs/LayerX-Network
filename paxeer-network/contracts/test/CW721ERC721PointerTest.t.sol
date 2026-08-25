// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.0;

import {Test, console2} from "forge-std/Test.sol";
import {CW721ERC721Pointer} from "../src/CW721ERC721Pointer.sol";
import {IWasmd} from "../src/precompiles/IWasmd.sol";
import {IJson} from "../src/precompiles/IJson.sol";
import {IAddr} from "../src/precompiles/IAddr.sol";

address constant WASMD_PRECOMPILE_ADDRESS = 0x0000000000000000000000000000000000001002;
address constant JSON_PRECOMPILE_ADDRESS = 0x0000000000000000000000000000000000001003;
address constant ADDR_PRECOMPILE_ADDRESS = 0x0000000000000000000000000000000000001004;

address constant MockCallerEVMAddr = 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266;
address constant MockOperatorEVMAddr = 0xF39fD6e51Aad88F6f4CE6AB8827279CFffb92267;
string constant MockCallerPaxAddr = "pax19zhelek4q5lt4zam8mcarmgv92vzgqd3d9ay88";
string constant MockOperatorPaxAddr = "pax1vldxw5dy5k68hqr4d744rpg9w8cqs54xy7ur4s";
string constant MockCWContractAddress = "pax14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9snf99un";

contract MockWasmd is IWasmd {

    // Transactions
    function instantiate(
        uint64,
        string memory,
        bytes memory,
        string memory,
        bytes memory
    ) external pure returns (string memory, bytes memory) {
        return (MockCWContractAddress, bytes(""));
    }

    function execute(
        string memory contractAddress,
        bytes memory,
        bytes memory
    ) external pure returns (bytes memory) {
        require(keccak256(abi.encodePacked(contractAddress)) == keccak256(abi.encodePacked(MockCWContractAddress)), "wrong CW contract address");
        return bytes("");
    }

    // Queries
    function query(string memory, bytes memory) external pure returns (bytes memory) {
        return bytes("");
    }
}

contract MockJson is IJson {
    function extractAsBytes(bytes memory, string memory) external pure returns (bytes memory) {
        return bytes("extracted bytes");
    }

    function extractAsBytesList(bytes memory, string memory) external pure returns (bytes[] memory) {
        return new bytes[](0);
    }

    function extractAsUint256(bytes memory input, string memory key) external view returns (uint256 response) {
        return 0;
    }
}

contract MockAddr is IAddr {
    function getPaxAddr(address addr) external pure returns (string memory) {
        if (addr == MockCallerEVMAddr) {
            return MockCallerPaxAddr;
        }
        return MockOperatorPaxAddr;
    }

    function getEvmAddr(string memory addr) external pure returns (address) {
        if (keccak256(abi.encodePacked(addr)) == keccak256(abi.encodePacked(MockCallerPaxAddr))) {
            return MockCallerEVMAddr;
        }
        return MockOperatorEVMAddr;
    }
}

contract CW721ERC721PointerTest is Test {
    event Transfer(address indexed from, address indexed to, uint256 indexed tokenId);
    event Approval(address indexed owner, address indexed approved, uint256 indexed tokenId);
    event ApprovalForAll(address indexed owner, address indexed operator, bool approved);

    CW721ERC721Pointer pointer;
    MockWasmd mockWasmd;
    MockJson mockJson;
    MockAddr mockAddr;

    function setUp() public {
        pointer = new CW721ERC721Pointer(MockCWContractAddress, "name", "symbol");
        mockWasmd = new MockWasmd();
        mockJson = new MockJson();
        mockAddr = new MockAddr();
        vm.etch(WASMD_PRECOMPILE_ADDRESS, address(mockWasmd).code);
        vm.etch(JSON_PRECOMPILE_ADDRESS, address(mockJson).code);
        vm.etch(ADDR_PRECOMPILE_ADDRESS, address(mockAddr).code);
    }

    function testName() public {
        assertEq(pointer.name(), "name");
    }

    function testSymbol() public {
        assertEq(pointer.symbol(), "symbol");
    }

    function testBalanceOf() public {
        vm.mockCall(
            WASMD_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("query(string,bytes)", MockCWContractAddress, bytes("{\"tokens\":{\"limit\":1000,\"owner\":\"pax19zhelek4q5lt4zam8mcarmgv92vzgqd3d9ay88\"}}")),
            abi.encode("{\"tokens\":[\"a\",\"b\"]}")
        );
        vm.mockCall(
            WASMD_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("query(string,bytes)", MockCWContractAddress, bytes("{\"tokens\":{\"limit\":1000,\"owner\":\"pax19zhelek4q5lt4zam8mcarmgv92vzgqd3d9ay88\",\"start_after\":\"b\"}}")),
            abi.encode("{\"tokens\":[\"c\",\"d\"]}")
        );
        vm.mockCall(
            WASMD_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("query(string,bytes)", MockCWContractAddress, bytes("{\"tokens\":{\"limit\":1000,\"owner\":\"pax19zhelek4q5lt4zam8mcarmgv92vzgqd3d9ay88\",\"start_after\":\"d\"}}")),
            abi.encode("{\"tokens\":[]}")
        );
        bytes[] memory resp1 = new bytes[](2);
        bytes[] memory resp2 = new bytes[](2);
        bytes[] memory resp3 = new bytes[](0);
        resp1[0] = bytes("\"a\"");
        resp1[1] = bytes("\"b\"");
        resp2[0] = bytes("\"c\"");
        resp2[1] = bytes("\"d\"");
        vm.mockCall(
            JSON_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("extractAsBytesList(bytes,string)", bytes("{\"tokens\":[\"a\",\"b\"]}"), "tokens"),
            abi.encode(resp1)
        );
        vm.mockCall(
            JSON_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("extractAsBytesList(bytes,string)", bytes("{\"tokens\":[\"c\",\"d\"]}"), "tokens"),
            abi.encode(resp2)
        );
        vm.mockCall(
            JSON_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("extractAsBytesList(bytes,string)", bytes("{\"tokens\":[]}"), "tokens"),
            abi.encode(resp3)
        );
        assertEq(pointer.balanceOf(MockCallerEVMAddr), 4);
    }

    function testOwnerOf() public {
        vm.mockCall(
            WASMD_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("query(string,bytes)", MockCWContractAddress, bytes("{\"owner_of\":{\"token_id\":\"1\"}}")),
            abi.encode("{\"owner\":\"pax19zhelek4q5lt4zam8mcarmgv92vzgqd3d9ay88\"}")
        );
        vm.mockCall(
            JSON_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("extractAsBytes(bytes,string)", bytes("{\"owner\":\"pax19zhelek4q5lt4zam8mcarmgv92vzgqd3d9ay88\"}"), "owner"),
            abi.encode(bytes("pax19zhelek4q5lt4zam8mcarmgv92vzgqd3d9ay88"))
        );
        assertEq(pointer.ownerOf(1), 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266);
    }

    function testTotalSupply() public {
        vm.mockCall(
            WASMD_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("query(string,bytes)", MockCWContractAddress, bytes("{\"num_tokens\":{}}")),
            abi.encode("{\"count\":100}")
        );
        vm.mockCall(
            JSON_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("extractAsUint256(bytes,string)", bytes("{\"count\":100}"), "count"),
            abi.encode(100)
        );
        assertEq(pointer.totalSupply(), 100);
    }

    function testGetApproved() public {
        vm.mockCall(
            WASMD_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("query(string,bytes)", MockCWContractAddress, bytes("{\"approvals\":{\"token_id\":\"1\"}}")),
            abi.encode("{\"approvals\":[{\"spender\":\"pax1vldxw5dy5k68hqr4d744rpg9w8cqs54xy7ur4s\"}]}")
        );
        bytes[] memory response = new bytes[](1);
        response[0] = bytes("{\"spender\":\"pax1vldxw5dy5k68hqr4d744rpg9w8cqs54xy7ur4s\"}");
        vm.mockCall(
            JSON_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("extractAsBytesList(bytes,string)", bytes("{\"approvals\":[{\"spender\":\"pax1vldxw5dy5k68hqr4d744rpg9w8cqs54xy7ur4s\"}]}"), "approvals"),
            abi.encode(response)
        );
        vm.mockCall(
            JSON_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("extractAsBytes(bytes,string)", bytes("{\"spender\":\"pax1vldxw5dy5k68hqr4d744rpg9w8cqs54xy7ur4s\"}}"), "spender"),
            abi.encode(bytes("pax1vldxw5dy5k68hqr4d744rpg9w8cqs54xy7ur4s"))
        );
        vm.startPrank(0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266);
        assertEq(pointer.getApproved(1), 0xF39fD6e51Aad88F6f4CE6AB8827279CFffb92267);
        vm.stopPrank();
    }

    function testIsApprovedForAll() public {
        vm.mockCall(
            WASMD_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("query(string,bytes)", MockCWContractAddress, bytes("{\"all_operators\":{\"owner\":\"pax19zhelek4q5lt4zam8mcarmgv92vzgqd3d9ay88\"}}")),
            abi.encode("{\"operators\":[{\"spender\":\"pax1vldxw5dy5k68hqr4d744rpg9w8cqs54xy7ur4s\"}]}")
        );
        bytes[] memory response = new bytes[](1);
        response[0] = bytes("{\"spender\":\"pax1vldxw5dy5k68hqr4d744rpg9w8cqs54xy7ur4s\"}");
        vm.mockCall(
            JSON_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("extractAsBytesList(bytes,string)", bytes("{\"operators\":[{\"spender\":\"pax1vldxw5dy5k68hqr4d744rpg9w8cqs54xy7ur4s\"}]}"), "operators"),
            abi.encode(response)
        );
        vm.mockCall(
            JSON_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("extractAsBytes(bytes,string)", bytes("{\"spender\":\"pax1vldxw5dy5k68hqr4d744rpg9w8cqs54xy7ur4s\"}}"), "spender"),
            abi.encode(bytes("pax1vldxw5dy5k68hqr4d744rpg9w8cqs54xy7ur4s"))
        );
        assertEq(pointer.isApprovedForAll(0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266, 0xF39fD6e51Aad88F6f4CE6AB8827279CFffb92267), true);
    }

    function testRoyaltyInfo() public {
        vm.mockCall(
            WASMD_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("query(string,bytes)", MockCWContractAddress, bytes("{\"extension\":{\"msg\":{\"check_royalties\":{}}}}")),
            abi.encode("{\"royalty_payments\":true}")
        );
        vm.mockCall(
            JSON_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("extractAsBytes(bytes,string)", bytes("{\"royalty_payments\":true}"), "royalty_payments"),
            abi.encode("true")
        );
        vm.mockCall(
            WASMD_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("query(string,bytes)", MockCWContractAddress, bytes("{\"extension\":{\"msg\":{\"royalty_info\":{\"token_id\":\"1\",\"sale_price\":\"1000\"}}}}")),
            abi.encode("{\"address\":\"pax19zhelek4q5lt4zam8mcarmgv92vzgqd3d9ay88\",\"royalty_amount\":\"10\"}")
        );
        vm.mockCall(
            JSON_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("extractAsBytes(bytes,string)", bytes("{\"address\":\"pax19zhelek4q5lt4zam8mcarmgv92vzgqd3d9ay88\",\"royalty_amount\":\"10\"}"), "address"),
            abi.encode("pax19zhelek4q5lt4zam8mcarmgv92vzgqd3d9ay88")
        );
        vm.mockCall(
            JSON_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("extractAsUint256(bytes,string)", bytes("{\"address\":\"pax19zhelek4q5lt4zam8mcarmgv92vzgqd3d9ay88\",\"royalty_amount\":\"10\"}"), "royalty_amount"),
            abi.encode(10)
        );
        vm.mockCall(
            ADDR_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("getEvmAddr(string)", "pax19zhelek4q5lt4zam8mcarmgv92vzgqd3d9ay88"),
            abi.encode(address(MockCallerEVMAddr))
        );
        (address recipient, uint256 royalties) = pointer.royaltyInfo(1, 1000);
        assertEq(recipient, address(MockCallerEVMAddr));
        assertEq(royalties, 10);
    }

    function testTransferFrom() public {
        vm.mockCall(
            WASMD_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("query(string,bytes)", MockCWContractAddress, bytes("{\"owner_of\":{\"token_id\":\"1\"}}")),
            abi.encode("{\"owner\":\"pax1vldxw5dy5k68hqr4d744rpg9w8cqs54xy7ur4s\"}")
        );
        vm.mockCall(
            JSON_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("extractAsBytes(bytes,string)", bytes("{\"owner\":\"pax1vldxw5dy5k68hqr4d744rpg9w8cqs54xy7ur4s\"}"), "owner"),
            abi.encode(bytes("pax1vldxw5dy5k68hqr4d744rpg9w8cqs54xy7ur4s"))
        );
        vm.mockCall(
            ADDR_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("getEvmAddr(string)", "pax1vldxw5dy5k68hqr4d744rpg9w8cqs54xy7ur4s"),
            abi.encode(address(0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266))
        );
        vm.mockCall(
            WASMD_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("execute(string,bytes,bytes)", MockCWContractAddress, bytes("{\"transfer_nft\":{\"recipient\":\"pax1vldxw5dy5k68hqr4d744rpg9w8cqs54xy7ur4s\",\"token_id\":\"1\"}}"), bytes("[]")),
            abi.encode(bytes(""))
        );
        vm.startPrank(0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266);
        //vm.expectEmit();
        //emit Transfer(0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266, 0xF39fD6e51Aad88F6f4CE6AB8827279CFffb92267, 1);
        pointer.transferFrom(0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266, 0xF39fD6e51Aad88F6f4CE6AB8827279CFffb92267, 1);
        vm.stopPrank();
    }

    function testApprove() public {
        vm.mockCall(
            WASMD_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("execute(string,bytes,bytes)", MockCWContractAddress, bytes("{\"approve\":{\"spender\":\"pax1vldxw5dy5k68hqr4d744rpg9w8cqs54xy7ur4s\",\"token_id\":\"1\"}}"), bytes("[]")),
            abi.encode(bytes(""))
        );
        vm.mockCall(
            WASMD_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("query(string,bytes)", MockCWContractAddress, bytes("{\"owner_of\":{\"token_id\":\"1\"}}")),
            abi.encode("{\"owner\":\"pax19zhelek4q5lt4zam8mcarmgv92vzgqd3d9ay88\"}")
        );
        vm.mockCall(
            JSON_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("extractAsBytes(bytes,string)", bytes("{\"owner\":\"pax19zhelek4q5lt4zam8mcarmgv92vzgqd3d9ay88\"}"), "owner"),
            abi.encode(bytes("pax19zhelek4q5lt4zam8mcarmgv92vzgqd3d9ay88"))
        );
        vm.startPrank(0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266);
        //vm.expectEmit();
        //emit Approval(0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266, 0xF39fD6e51Aad88F6f4CE6AB8827279CFffb92267, 1);
        pointer.approve(0xF39fD6e51Aad88F6f4CE6AB8827279CFffb92267, 1);
        vm.stopPrank();
    }

    function testSetApprovalForAll() public {
        vm.mockCall(
            WASMD_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("execute(string,bytes,bytes)", MockCWContractAddress, bytes("{\"approval_all\":{\"operator\":\"pax1vldxw5dy5k68hqr4d744rpg9w8cqs54xy7ur4s\"}}"), bytes("[]")),
            abi.encode(bytes(""))
        );
        vm.startPrank(0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266);
        //vm.expectEmit();
        //emit ApprovalForAll(0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266, 0xF39fD6e51Aad88F6f4CE6AB8827279CFffb92267, true);
        pointer.setApprovalForAll(0xF39fD6e51Aad88F6f4CE6AB8827279CFffb92267, true);
        vm.stopPrank();
    }

    function testSetRevokeForAll() public {
        vm.mockCall(
            WASMD_PRECOMPILE_ADDRESS,
            abi.encodeWithSignature("execute(string,bytes,bytes)", MockCWContractAddress, bytes("{\"revoke_all\":{\"operator\":\"pax1vldxw5dy5k68hqr4d744rpg9w8cqs54xy7ur4s\"}}"), bytes("[]")),
            abi.encode(bytes(""))
        );
        vm.startPrank(0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266);
        //vm.expectEmit();
        //emit ApprovalForAll(0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266, 0xF39fD6e51Aad88F6f4CE6AB8827279CFffb92267, false);
        pointer.setApprovalForAll(0xF39fD6e51Aad88F6f4CE6AB8827279CFffb92267, false);
        vm.stopPrank();
    }
}